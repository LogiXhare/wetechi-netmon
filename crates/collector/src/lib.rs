//! WetechiNetMon Telemetry Collector (Phase 2: IPFIX only).
//!
//! Binds a UDP socket, decodes incoming IPFIX messages via
//! `wetechinetmon-protocol-ipfix`, maintains one template cache per
//! exporter, and exposes Prometheus metrics on a separate HTTP port. See
//! docs/functional-requirements.md (FR-1) for the full target scope —
//! NetFlow v9/v5 and sFlow v5 support are later phases, not this one.

pub mod config;
pub mod exporter;
pub mod metrics;
pub mod metrics_server;

use std::sync::Arc;

use tokio::net::UdpSocket;
use wetechinetmon_protocol_ipfix::{decode_message, DecodedSet};

pub use config::Config;
pub use exporter::ExporterRegistry;
pub use metrics::Metrics;

/// Maximum UDP datagram size we'll attempt to read. IPFIX messages are
/// bounded by the 16-bit Length field in the header (max 65535), and
/// real-world exporters targeting UDP stay well under the common 1500
/// byte path MTU to avoid IP fragmentation — 65535 is a safe upper bound
/// that costs one fixed-size buffer per receive, not a per-connection
/// allocation.
const MAX_DATAGRAM_SIZE: usize = 65535;

/// Runs the collector until the process is asked to stop (Ctrl+C) or an
/// unrecoverable I/O error occurs binding a socket.
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

    let socket = UdpSocket::bind(config.bind).await?;
    tracing::info!(bind = %config.bind, "IPFIX collector listening");

    let mut registry_state = ExporterRegistry::new();
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];

    loop {
        tokio::select! {
            recv = socket.recv_from(&mut buf) => {
                let (len, src) = recv?;
                metrics.datagrams_received_total.inc();
                process_datagram(&buf[..len], src, &mut registry_state, &metrics);
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutdown signal received, stopping collector");
                metrics_server.abort();
                return Ok(());
            }
        }
    }
}

/// Decodes one received datagram and updates metrics/logs accordingly.
/// Split out from `run` specifically so it can be unit-tested without a
/// real socket (see the tests below).
fn process_datagram(
    bytes: &[u8],
    src: std::net::SocketAddr,
    registry: &mut ExporterRegistry,
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
                        for record in records {
                            if record.fields.iter().any(|f| f.value.len() == 4) {
                                metrics.ipv4_flows_total.inc();
                            }
                            if record.fields.iter().any(|f| f.value.len() == 16) {
                                metrics.ipv6_flows_total.inc();
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn malformed_header_increments_parser_failures_without_panicking() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut registry = ExporterRegistry::new();
        process_datagram(&[0xFF, 0xFF, 0xFF], addr(), &mut registry, &metrics);
        assert_eq!(metrics.parser_failures_total.get(), 1);
    }

    #[test]
    fn a_template_set_followed_by_a_data_set_updates_expected_metrics() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut registry = ExporterRegistry::new();

        // Template Set: template 256 with one 4-byte field.
        let mut record = Vec::new();
        record.extend_from_slice(&256u16.to_be_bytes());
        record.extend_from_slice(&1u16.to_be_bytes());
        record.extend_from_slice(&8u16.to_be_bytes());
        record.extend_from_slice(&4u16.to_be_bytes());
        let mut set = Vec::new();
        set.extend_from_slice(&2u16.to_be_bytes());
        set.extend_from_slice(&((4 + record.len()) as u16).to_be_bytes());
        set.extend_from_slice(&record);
        let mut msg1 = message_header_bytes((16 + set.len()) as u16);
        msg1.extend_from_slice(&set);

        process_datagram(&msg1, addr(), &mut registry, &metrics);
        assert_eq!(metrics.parser_failures_total.get(), 0);
        assert_eq!(metrics.template_cache_size.get(), 1);

        // Data Set for template 256: one 4-byte (IPv4-looking) field.
        let field_value = [10u8, 0, 0, 1];
        let mut data_set = Vec::new();
        data_set.extend_from_slice(&256u16.to_be_bytes());
        data_set.extend_from_slice(&((4 + field_value.len()) as u16).to_be_bytes());
        data_set.extend_from_slice(&field_value);
        let mut msg2 = message_header_bytes((16 + data_set.len()) as u16);
        msg2.extend_from_slice(&data_set);

        process_datagram(&msg2, addr(), &mut registry, &metrics);
        assert_eq!(metrics.parser_failures_total.get(), 0);
        assert_eq!(metrics.parsed_flow_records_total.get(), 1);
        assert_eq!(metrics.ipv4_flows_total.get(), 1);
        assert_eq!(
            metrics
                .sets_by_kind_total
                .with_label_values(&["data"])
                .get(),
            1
        );
    }

    #[test]
    fn sequence_number_regression_increments_restart_metric() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut registry = ExporterRegistry::new();

        let mut msg1 = message_header_bytes(16);
        // sequence number = 100
        msg1[8..12].copy_from_slice(&100u32.to_be_bytes());
        process_datagram(&msg1, addr(), &mut registry, &metrics);

        let mut msg2 = message_header_bytes(16);
        // sequence number regresses to 3 -> restart
        msg2[8..12].copy_from_slice(&3u32.to_be_bytes());
        process_datagram(&msg2, addr(), &mut registry, &metrics);

        assert_eq!(metrics.exporter_restarts_total.get(), 1);
    }

    #[test]
    fn unknown_template_data_set_increments_unknown_template_metric() {
        let (metrics, _registry) = Metrics::new().unwrap();
        let mut registry = ExporterRegistry::new();

        let mut data_set = Vec::new();
        data_set.extend_from_slice(&999u16.to_be_bytes()); // template never defined
        data_set.extend_from_slice(&8u16.to_be_bytes());
        data_set.extend_from_slice(&[0, 0, 0, 0]);
        let mut msg = message_header_bytes((16 + data_set.len()) as u16);
        msg.extend_from_slice(&data_set);

        process_datagram(&msg, addr(), &mut registry, &metrics);
        assert_eq!(metrics.unknown_templates_total.get(), 1);
        assert_eq!(metrics.parser_failures_total.get(), 0);
    }
}
