//! WetechiNetMon Telemetry Collector.
//!
//! Binds a UDP socket, decodes incoming IPFIX messages via
//! `wetechinetmon-protocol-ipfix`, normalizes Data Records into
//! `NormalizedFlow`s (Phase 3), classifies their direction
//! (`wetechinetmon-classifier`), and aggregates them
//! (`wetechinetmon-aggregator`). Exposes Prometheus metrics on a
//! separate HTTP port. See docs/functional-requirements.md (FR-1, FR-2,
//! FR-3) — NetFlow v9/v5 and sFlow v5 support are later phases.
//!
//! **Pipeline shape (ADR 0004):** the UDP receive loop and the
//! classify/aggregate stage run as two tasks connected by a bounded
//! in-process channel — not a separate OS process, not NATS (yet). The
//! channel's bounded capacity is both the backpressure control and the
//! `queue_depth` metric.

pub mod clickhouse_export;
pub mod config;
pub mod exporter;
pub mod metrics;
pub mod metrics_server;
pub mod normalize;
pub mod pipeline;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use wetechinetmon_classifier::{build_registry, classify, Direction};
use wetechinetmon_protocol_ipfix::{decode_message, DecodedSet};

pub use config::Config;
pub use exporter::ExporterRegistry;
pub use metrics::Metrics;
pub use pipeline::{Pipeline, SamplingConfig};

/// Maximum UDP datagram size we'll attempt to read. IPFIX messages are
/// bounded by the 16-bit Length field in the header (max 65535), and
/// real-world exporters targeting UDP stay well under the common 1500
/// byte path MTU to avoid IP fragmentation — 65535 is a safe upper bound
/// that costs one fixed-size buffer per receive, not a per-connection
/// allocation.
const MAX_DATAGRAM_SIZE: usize = 65535;

/// How often the aggregator sweeps for inactivity expiration.
const EXPIRATION_INTERVAL: Duration = Duration::from_secs(30);

/// How often the aggregator's current state is snapshotted and pushed
/// toward ClickHouse (only relevant when ClickHouse export is
/// configured).
const CLICKHOUSE_EXPORT_INTERVAL: Duration = Duration::from_secs(15);

/// Runs the collector until the process is asked to stop (Ctrl+C or, on
/// Unix, SIGTERM) or an unrecoverable I/O error occurs binding a socket.
pub async fn run(config: Config) -> std::io::Result<()> {
    let (metrics, registry) =
        Metrics::new().expect("metric registration should never collide at startup");
    let registry = Arc::new(registry);

    let metrics_addr = config.metrics_bind;
    let metrics_server = tokio::spawn(async move {
        if let Err(err) = metrics_server::serve(metrics_addr, registry).await {
            tracing::error!(error = %err, "metrics server exited with an error");
        }
    });

    let socket = Arc::new(UdpSocket::bind(config.bind).await?);
    tracing::info!(bind = %config.bind, "IPFIX collector listening");

    let prefix_report = build_registry(&config.local_prefixes).unwrap_or_else(|errors| {
        // A prefix-config error is a startup-quality problem, but Phase 3
        // treats it as "run with an empty registry" (Direction::Unknown
        // for everything) rather than refusing to start entirely — the
        // collector's job (decode/normalize/aggregate) is still useful
        // even with direction classification degraded. Every error is
        // still logged loudly.
        for e in &errors {
            tracing::error!(error = %e, "invalid local-prefix configuration entry, ignoring it");
        }
        wetechinetmon_classifier::build_registry(&[]).expect("empty prefix list always validates")
    });
    for warning in &prefix_report.warnings {
        tracing::warn!(message = %warning.message, "local-prefix overlap");
    }

    let aggregator_config = wetechinetmon_aggregator::AggregatorConfig {
        max_hosts: config.max_hosts,
        max_networks: config.max_networks,
        max_hostgroups: config.max_hostgroups,
        max_asns: config.max_asns,
        inactivity_ttl: Duration::from_secs(config.inactivity_ttl_secs),
        ..Default::default()
    };
    let sampling_config = SamplingConfig {
        global_default: config.sampling_global_default,
        per_exporter: Default::default(),
    };
    let mut pipeline = Pipeline::new(
        prefix_report.registry,
        aggregator_config,
        sampling_config,
        Instant::now(),
    );

    let (tx, mut rx) = tokio::sync::mpsc::channel::<(Vec<u8>, SocketAddr)>(config.queue_capacity);

    let receiver_task = tokio::spawn(receive_loop(
        Arc::clone(&socket),
        tx,
        metrics.datagrams_received_total.clone(),
        metrics.queue_depth.clone(),
    ));

    let mut exporters = ExporterRegistry::new();
    let mut expire_interval = tokio::time::interval(EXPIRATION_INTERVAL);

    // ClickHouse export is entirely optional — see
    // docs/integrations/clickhouse.md. A connection/migration failure at
    // startup disables export for this run rather than crashing the
    // collector; decode/normalize/aggregate remains useful without it.
    let mut clickhouse_exporters = match &config.clickhouse_url {
        Some(url) => {
            let client = wetechinetmon_storage::Client::default().with_url(url);
            match wetechinetmon_storage::run_migrations(&client).await {
                Ok(()) => {
                    tracing::info!(url, "ClickHouse export enabled, migrations applied");
                    Some(clickhouse_export::ClickHouseExporters::new(
                        client,
                        Instant::now(),
                    ))
                }
                Err(err) => {
                    tracing::error!(url, error = %err, "ClickHouse migration failed; export disabled for this run");
                    None
                }
            }
        }
        None => None,
    };
    let mut clickhouse_interval = tokio::time::interval(CLICKHOUSE_EXPORT_INTERVAL);

    loop {
        tokio::select! {
            received = rx.recv() => {
                match received {
                    Some((bytes, src)) => {
                        metrics.queue_depth.dec();
                        process_datagram(&bytes, src, &mut exporters, &mut pipeline, &metrics);
                    }
                    None => {
                        tracing::warn!("UDP receive task ended; stopping collector");
                        break;
                    }
                }
            }
            _ = expire_interval.tick() => {
                let expired = pipeline.aggregator.expire_inactive(Instant::now());
                if expired > 0 {
                    metrics.expired_entries_total.inc_by(expired as u64);
                    tracing::debug!(expired, "swept inactive aggregation entries");
                }
            }
            _ = clickhouse_interval.tick(), if clickhouse_exporters.is_some() => {
                if let Some(exporters) = clickhouse_exporters.as_mut() {
                    let now = Instant::now();
                    let ts = time::OffsetDateTime::now_utc();
                    exporters.snapshot(&pipeline.aggregator, ts);
                    let report = exporters.tick(now).await;
                    metrics.clickhouse_rows_written_total.inc_by(report.rows_written as u64);
                    metrics.clickhouse_write_failures_total.inc_by(report.write_failures as u64);
                    metrics.clickhouse_retry_queue_dropped_total.inc_by(report.retry_queue_dropped as u64);
                    if report.permanently_dropped_batches > 0 {
                        tracing::error!(
                            batches = report.permanently_dropped_batches,
                            "ClickHouse batches permanently dropped after exhausting retries"
                        );
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl+C received, shutting down");
                break;
            }
            _ = wait_for_terminate() => {
                tracing::info!("SIGTERM received, shutting down");
                break;
            }
        }
    }

    receiver_task.abort();
    metrics_server.abort();
    Ok(())
}

async fn receive_loop(
    socket: Arc<UdpSocket>,
    tx: tokio::sync::mpsc::Sender<(Vec<u8>, SocketAddr)>,
    datagrams_received_total: prometheus::IntCounter,
    queue_depth: prometheus::IntGauge,
) {
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, src)) => {
                datagrams_received_total.inc();
                queue_depth.inc();
                // A bounded channel makes `send` apply real backpressure
                // (ADR 0004 / FR-2.4 "queue limits"): if the
                // classify/aggregate stage falls behind, this await
                // blocks here rather than growing memory without bound.
                if tx.send((buf[..len].to_vec(), src)).await.is_err() {
                    break; // receiver dropped — shutting down
                }
            }
            Err(err) => {
                tracing::error!(error = %err, "UDP recv_from failed");
            }
        }
    }
}

/// On Unix, resolves when SIGTERM is received (the signal container
/// runtimes and systemd send for graceful shutdown). On other platforms,
/// never resolves — Ctrl+C handling above is the only shutdown signal
/// there. Platform-specific by necessity (Phase 2 follow-up item).
#[cfg(unix)]
async fn wait_for_terminate() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut stream) => {
            stream.recv().await;
        }
        Err(err) => {
            tracing::error!(error = %err, "failed to install SIGTERM handler");
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(not(unix))]
async fn wait_for_terminate() {
    std::future::pending::<()>().await;
}

/// Decodes one received datagram, normalizes/classifies/aggregates any
/// Data Records within it, and updates metrics/logs accordingly. Split
/// out from `run` specifically so it can be unit-tested without a real
/// socket (see the tests below).
fn process_datagram(
    bytes: &[u8],
    src: std::net::SocketAddr,
    registry: &mut ExporterRegistry,
    pipeline: &mut Pipeline,
    metrics: &Metrics,
) {
    // Peek the header first (cheap — 16 bytes, no allocation) purely to
    // get the sequence number for restart detection *before* we hand the
    // (possibly stale) template cache to the full decoder.
    let header = match wetechinetmon_protocol_ipfix::MessageHeader::parse(bytes) {
        Ok(h) => h,
        Err(err) => {
            metrics.parser_failures_total.inc();
            tracing::warn!(%src, error = %err, "failed to parse IPFIX message header");
            return;
        }
    };

    let exporter = registry.get_or_create(src);
    if exporter.observe_sequence(header.sequence_number) {
        metrics.exporter_restarts_total.inc();
        tracing::warn!(%src, sequence_number = header.sequence_number, "exporter restart detected, template cache cleared");
    }

    match decode_message(bytes, &mut exporter.template_cache) {
        Ok(message) => {
            for set in &message.sets {
                match set {
                    DecodedSet::Templates(templates) => {
                        metrics
                            .sets_by_kind_total
                            .with_label_values(&["templates"])
                            .inc();
                        tracing::debug!(%src, count = templates.len(), "received Template Set");
                    }
                    DecodedSet::OptionsTemplates(templates) => {
                        metrics
                            .sets_by_kind_total
                            .with_label_values(&["options_templates"])
                            .inc();
                        tracing::debug!(%src, count = templates.len(), "received Options Template Set");
                    }
                    DecodedSet::Data { records, .. } => {
                        metrics
                            .sets_by_kind_total
                            .with_label_values(&["data"])
                            .inc();
                        metrics
                            .parsed_flow_records_total
                            .inc_by(records.len() as u64);

                        let options_sampling = exporter.template_cache.sampling();
                        let external_sampling = pipeline.sampling.for_exporter(src.ip());

                        for record in records {
                            if record.fields.iter().any(|f| f.value.len() == 4) {
                                metrics.ipv4_flows_total.inc();
                            }
                            if record.fields.iter().any(|f| f.value.len() == 16) {
                                metrics.ipv6_flows_total.inc();
                            }

                            let start = Instant::now();
                            match normalize::normalize_ipfix_record(
                                record,
                                src.ip(),
                                header.observation_domain_id,
                                options_sampling,
                                external_sampling,
                            ) {
                                Ok(outcome) => {
                                    metrics.normalized_flows_total.inc();
                                    if outcome.flow.sampling_rate.get() > 1 {
                                        metrics.corrected_samples_total.inc();
                                    }
                                    if outcome.zero_rate_skipped {
                                        metrics.sampling_errors_total.inc();
                                    }
                                    if matches!(
                                        outcome.flow.protocol,
                                        Some(wetechinetmon_common::Protocol::Other(_))
                                    ) {
                                        metrics.unsupported_protocol_fields_total.inc();
                                    }

                                    let classification =
                                        classify(&pipeline.prefixes, &outcome.flow);
                                    let direction_label = match classification.direction {
                                        Direction::Incoming => "incoming",
                                        Direction::Outgoing => "outgoing",
                                        Direction::Internal => "internal",
                                        Direction::Other => "other",
                                        Direction::Unknown => "unknown",
                                    };
                                    metrics
                                        .classified_flows_by_direction_total
                                        .with_label_values(&[direction_label])
                                        .inc();
                                    if classification.direction == Direction::Unknown {
                                        metrics.prefix_lookup_failures_total.inc();
                                    }

                                    let now = Instant::now();
                                    let ingest_report = pipeline.aggregator.ingest(
                                        &outcome.flow,
                                        &classification,
                                        now,
                                    );
                                    metrics
                                        .evicted_entries_total
                                        .inc_by(ingest_report.evictions as u64);

                                    metrics
                                        .aggregation_latency_seconds
                                        .observe(start.elapsed().as_secs_f64());
                                }
                                Err(err) => {
                                    metrics.incomplete_records_total.inc();
                                    tracing::debug!(%src, error = %err, "record could not be normalized");
                                }
                            }
                        }
                    }
                    DecodedSet::UnknownTemplate { template_id } => {
                        metrics
                            .sets_by_kind_total
                            .with_label_values(&["unknown_template"])
                            .inc();
                        metrics.unknown_templates_total.inc();
                        tracing::debug!(%src, template_id, "data set references unknown template");
                    }
                    DecodedSet::ReservedSetId { set_id } => {
                        metrics
                            .sets_by_kind_total
                            .with_label_values(&["reserved_set_id"])
                            .inc();
                        metrics.reserved_set_id_total.inc();
                        tracing::warn!(%src, set_id, "received reserved/invalid Set ID");
                    }
                }
            }
        }
        Err(err) => {
            metrics.parser_failures_total.inc();
            tracing::warn!(%src, error = %err, "failed to decode IPFIX message body");
        }
    }

    metrics
        .template_cache_size
        .set(registry.total_template_count() as i64);
    metrics
        .active_exporters
        .set(registry.exporter_count() as i64);
    metrics
        .active_hosts
        .set(pipeline.aggregator.active_hosts() as i64);
    metrics
        .active_networks
        .set(pipeline.aggregator.active_networks() as i64);
    metrics
        .active_hostgroups
        .set(pipeline.aggregator.active_hostgroups() as i64);
    metrics
        .active_asns
        .set(pipeline.aggregator.active_asns() as i64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wetechinetmon_classifier::PrefixConfigEntry;

    fn addr() -> std::net::SocketAddr {
        "127.0.0.1:1".parse().unwrap()
    }

    fn message_header_bytes(length: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&0x000au16.to_be_bytes());
        b.extend_from_slice(&length.to_be_bytes());
        b.extend_from_slice(&1_700_000_000u32.to_be_bytes());
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&7u32.to_be_bytes());
        b
    }

    fn test_pipeline() -> Pipeline {
        let report = build_registry(&[PrefixConfigEntry {
            network: "10.0.0.0".parse().unwrap(),
            prefix_len: 8,
            tenant: "wetechi".to_string(),
            hostgroup: Some("core".to_string()),
        }])
        .unwrap();
        Pipeline::new(
            report.registry,
            wetechinetmon_aggregator::AggregatorConfig::default(),
            SamplingConfig::default(),
            Instant::now(),
        )
    }

    fn template_set_bytes(ie_len_pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&256u16.to_be_bytes());
        record.extend_from_slice(&(ie_len_pairs.len() as u16).to_be_bytes());
        for (ie, len) in ie_len_pairs {
            record.extend_from_slice(&ie.to_be_bytes());
            record.extend_from_slice(&len.to_be_bytes());
        }
        let mut set = Vec::new();
        set.extend_from_slice(&2u16.to_be_bytes());
        set.extend_from_slice(&((4 + record.len()) as u16).to_be_bytes());
        set.extend_from_slice(&record);
        set
    }

    #[test]
    fn malformed_header_increments_parser_failures_without_panicking() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut exporters = ExporterRegistry::new();
        let mut pipeline = test_pipeline();
        process_datagram(
            &[0xFF, 0xFF, 0xFF],
            addr(),
            &mut exporters,
            &mut pipeline,
            &metrics,
        );
        assert_eq!(metrics.parser_failures_total.get(), 1);
    }

    #[test]
    fn a_full_ipfix_flow_is_normalized_classified_and_aggregated() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut exporters = ExporterRegistry::new();
        let mut pipeline = test_pipeline();

        // Template: sourceIPv4Address(8,4), destinationIPv4Address(12,4),
        // octetDeltaCount(1,8), packetDeltaCount(2,8).
        let set = template_set_bytes(&[(8, 4), (12, 4), (1, 8), (2, 8)]);
        let mut msg1 = message_header_bytes((16 + set.len()) as u16);
        msg1.extend_from_slice(&set);
        process_datagram(&msg1, addr(), &mut exporters, &mut pipeline, &metrics);
        assert_eq!(metrics.parser_failures_total.get(), 0);

        // Data: source 203.0.113.1 (external) -> destination 10.0.0.5 (local) => Incoming.
        let mut record_bytes = Vec::new();
        record_bytes.extend_from_slice(&[203, 0, 113, 1]);
        record_bytes.extend_from_slice(&[10, 0, 0, 5]);
        record_bytes.extend_from_slice(&1000u64.to_be_bytes());
        record_bytes.extend_from_slice(&10u64.to_be_bytes());
        let mut data_set = Vec::new();
        data_set.extend_from_slice(&256u16.to_be_bytes());
        data_set.extend_from_slice(&((4 + record_bytes.len()) as u16).to_be_bytes());
        data_set.extend_from_slice(&record_bytes);
        let mut msg2 = message_header_bytes((16 + data_set.len()) as u16);
        msg2.extend_from_slice(&data_set);

        process_datagram(&msg2, addr(), &mut exporters, &mut pipeline, &metrics);

        assert_eq!(metrics.normalized_flows_total.get(), 1);
        assert_eq!(metrics.incomplete_records_total.get(), 0);
        assert_eq!(
            metrics
                .classified_flows_by_direction_total
                .with_label_values(&["incoming"])
                .get(),
            1
        );
        assert_eq!(pipeline.aggregator.total_counters().bytes, 1000);
        assert_eq!(pipeline.aggregator.active_hosts(), 2); // source + destination
    }

    #[test]
    fn a_record_missing_addresses_increments_incomplete_records() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut exporters = ExporterRegistry::new();
        let mut pipeline = test_pipeline();

        // Template with only octetDeltaCount — no address fields.
        let set = template_set_bytes(&[(1, 8)]);
        let mut msg1 = message_header_bytes((16 + set.len()) as u16);
        msg1.extend_from_slice(&set);
        process_datagram(&msg1, addr(), &mut exporters, &mut pipeline, &metrics);

        let record_bytes = 100u64.to_be_bytes().to_vec();
        let mut data_set = Vec::new();
        data_set.extend_from_slice(&256u16.to_be_bytes());
        data_set.extend_from_slice(&((4 + record_bytes.len()) as u16).to_be_bytes());
        data_set.extend_from_slice(&record_bytes);
        let mut msg2 = message_header_bytes((16 + data_set.len()) as u16);
        msg2.extend_from_slice(&data_set);

        process_datagram(&msg2, addr(), &mut exporters, &mut pipeline, &metrics);
        assert_eq!(metrics.incomplete_records_total.get(), 1);
        assert_eq!(metrics.normalized_flows_total.get(), 0);
    }

    #[test]
    fn sequence_number_regression_increments_restart_metric() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut exporters = ExporterRegistry::new();
        let mut pipeline = test_pipeline();

        let mut msg1 = message_header_bytes(16);
        msg1[8..12].copy_from_slice(&100u32.to_be_bytes());
        process_datagram(&msg1, addr(), &mut exporters, &mut pipeline, &metrics);

        let mut msg2 = message_header_bytes(16);
        msg2[8..12].copy_from_slice(&3u32.to_be_bytes());
        process_datagram(&msg2, addr(), &mut exporters, &mut pipeline, &metrics);

        assert_eq!(metrics.exporter_restarts_total.get(), 1);
    }

    #[test]
    fn unknown_template_data_set_increments_unknown_template_metric() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut exporters = ExporterRegistry::new();
        let mut pipeline = test_pipeline();

        let mut data_set = Vec::new();
        data_set.extend_from_slice(&999u16.to_be_bytes());
        data_set.extend_from_slice(&8u16.to_be_bytes());
        data_set.extend_from_slice(&[0, 0, 0, 0]);
        let mut msg = message_header_bytes((16 + data_set.len()) as u16);
        msg.extend_from_slice(&data_set);

        process_datagram(&msg, addr(), &mut exporters, &mut pipeline, &metrics);
        assert_eq!(metrics.unknown_templates_total.get(), 1);
        assert_eq!(metrics.parser_failures_total.get(), 0);
    }

    #[test]
    fn empty_prefix_registry_classifies_as_unknown_and_counts_lookup_failure() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut exporters = ExporterRegistry::new();
        let mut pipeline = Pipeline::new(
            build_registry(&[]).unwrap().registry,
            wetechinetmon_aggregator::AggregatorConfig::default(),
            SamplingConfig::default(),
            Instant::now(),
        );

        let set = template_set_bytes(&[(8, 4), (12, 4), (1, 8), (2, 8)]);
        let mut msg1 = message_header_bytes((16 + set.len()) as u16);
        msg1.extend_from_slice(&set);
        process_datagram(&msg1, addr(), &mut exporters, &mut pipeline, &metrics);

        let mut record_bytes = Vec::new();
        record_bytes.extend_from_slice(&[203, 0, 113, 1]);
        record_bytes.extend_from_slice(&[10, 0, 0, 5]);
        record_bytes.extend_from_slice(&1000u64.to_be_bytes());
        record_bytes.extend_from_slice(&10u64.to_be_bytes());
        let mut data_set = Vec::new();
        data_set.extend_from_slice(&256u16.to_be_bytes());
        data_set.extend_from_slice(&((4 + record_bytes.len()) as u16).to_be_bytes());
        data_set.extend_from_slice(&record_bytes);
        let mut msg2 = message_header_bytes((16 + data_set.len()) as u16);
        msg2.extend_from_slice(&data_set);
        process_datagram(&msg2, addr(), &mut exporters, &mut pipeline, &metrics);

        assert_eq!(metrics.prefix_lookup_failures_total.get(), 1);
        assert_eq!(
            metrics
                .classified_flows_by_direction_total
                .with_label_values(&["unknown"])
                .get(),
            1
        );
    }
}
