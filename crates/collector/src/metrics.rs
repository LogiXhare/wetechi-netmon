//! Prometheus metrics for the Telemetry Collector.
//!
//! Names and the set of metrics tracked here follow
//! docs/functional-requirements.md (FR-1.8) and the collector-relevant
//! subset of the metric list in `prompts/CLAUDE_MASTER_PROMPT.md` §20.
//! Phase 2 implements the metrics that are meaningful without an event
//! transport or downstream consumer yet (aggregation/detection latency
//! metrics are added when those components exist in Phase 3/4).
//!
//! **Known limitation:** `udp_receive_buffer_errors_total` from the
//! master-prompt list is not implemented in Phase 2 — reading the
//! kernel's UDP socket drop counter portably (Windows vs. Linux) needs
//! platform-specific code this phase doesn't need yet. Tracked as a
//! follow-up rather than faked with a metric that never increments.

use prometheus::{IntCounter, IntCounterVec, IntGauge, Opts, Registry};

pub struct Metrics {
    pub datagrams_received_total: IntCounter,
    pub parsed_flow_records_total: IntCounter,
    pub ipv4_flows_total: IntCounter,
    pub ipv6_flows_total: IntCounter,
    pub parser_failures_total: IntCounter,
    pub unknown_templates_total: IntCounter,
    pub exporter_restarts_total: IntCounter,
    pub reserved_set_id_total: IntCounter,
    pub template_cache_size: IntGauge,
    pub active_exporters: IntGauge,
    /// Labeled by decoded Set kind (`templates`, `options_templates`,
    /// `data`, `unknown_template`, `reserved_set_id`) — lets an operator
    /// see the traffic mix without needing per-kind counters wired up by
    /// hand for every future Set kind.
    pub sets_by_kind_total: IntCounterVec,
}

impl Metrics {
    /// Builds every collector metric and registers it with a fresh
    /// `Registry`, returned separately so callers can move `Metrics`
    /// into their receive loop and the `Registry` into the metrics HTTP
    /// server independently (see `metrics_server::serve`).
    pub fn new() -> Result<(Self, Registry), prometheus::Error> {
        let registry = Registry::new();

        let datagrams_received_total = IntCounter::new(
            "wetechinetmon_collector_flow_datagrams_received_total",
            "Total UDP datagrams received by the collector, before parsing.",
        )?;
        let parsed_flow_records_total = IntCounter::new(
            "wetechinetmon_collector_parsed_flow_records_total",
            "Total Data Records successfully decoded across all exporters.",
        )?;
        let ipv4_flows_total = IntCounter::new(
            "wetechinetmon_collector_ipv4_flows_total",
            "Total decoded Data Records containing at least one IPv4 address field.",
        )?;
        let ipv6_flows_total = IntCounter::new(
            "wetechinetmon_collector_ipv6_flows_total",
            "Total decoded Data Records containing at least one IPv6 address field.",
        )?;
        let parser_failures_total = IntCounter::new(
            "wetechinetmon_collector_parser_failures_total",
            "Total datagrams that failed to decode as a valid IPFIX message.",
        )?;
        let unknown_templates_total = IntCounter::new(
            "wetechinetmon_collector_unknown_templates_total",
            "Total Data Sets received referencing a template not yet known for that exporter.",
        )?;
        let exporter_restarts_total = IntCounter::new(
            "wetechinetmon_collector_exporter_restarts_total",
            "Total detected exporter restarts (sequence-number regression), which clear that exporter's template cache.",
        )?;
        let reserved_set_id_total = IntCounter::new(
            "wetechinetmon_collector_reserved_set_id_total",
            "Total Sets received using a reserved (invalid) Set ID.",
        )?;
        let template_cache_size = IntGauge::new(
            "wetechinetmon_collector_template_cache_size",
            "Current total number of cached templates across all known exporters.",
        )?;
        let active_exporters = IntGauge::new(
            "wetechinetmon_collector_active_exporters",
            "Current number of exporters (source addresses) with at least one cached template.",
        )?;
        let sets_by_kind_total = IntCounterVec::new(
            Opts::new(
                "wetechinetmon_collector_sets_by_kind_total",
                "Total decoded Sets, labeled by kind.",
            ),
            &["kind"],
        )?;

        registry.register(Box::new(datagrams_received_total.clone()))?;
        registry.register(Box::new(parsed_flow_records_total.clone()))?;
        registry.register(Box::new(ipv4_flows_total.clone()))?;
        registry.register(Box::new(ipv6_flows_total.clone()))?;
        registry.register(Box::new(parser_failures_total.clone()))?;
        registry.register(Box::new(unknown_templates_total.clone()))?;
        registry.register(Box::new(exporter_restarts_total.clone()))?;
        registry.register(Box::new(reserved_set_id_total.clone()))?;
        registry.register(Box::new(template_cache_size.clone()))?;
        registry.register(Box::new(active_exporters.clone()))?;
        registry.register(Box::new(sets_by_kind_total.clone()))?;

        let metrics = Metrics {
            datagrams_received_total,
            parsed_flow_records_total,
            ipv4_flows_total,
            ipv6_flows_total,
            parser_failures_total,
            unknown_templates_total,
            exporter_restarts_total,
            reserved_set_id_total,
            template_cache_size,
            active_exporters,
            sets_by_kind_total,
        };

        Ok((metrics, registry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_without_error_and_starts_at_zero() {
        let (metrics, registry) = Metrics::new().expect("metric registration should not collide");
        assert_eq!(metrics.datagrams_received_total.get(), 0);
        assert_eq!(metrics.template_cache_size.get(), 0);
        assert!(!registry.gather().is_empty());
    }

    #[test]
    fn counters_are_independently_incrementable() {
        let (metrics, _registry) = Metrics::new().unwrap();
        metrics.datagrams_received_total.inc();
        metrics.parser_failures_total.inc_by(3);
        metrics
            .sets_by_kind_total
            .with_label_values(&["data"])
            .inc();
        assert_eq!(metrics.datagrams_received_total.get(), 1);
        assert_eq!(metrics.parser_failures_total.get(), 3);
        assert_eq!(
            metrics
                .sets_by_kind_total
                .with_label_values(&["data"])
                .get(),
            1
        );
    }
}
