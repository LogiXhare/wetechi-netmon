//! Per-exporter state: template cache, sequence-number tracking, and
//! restart detection.
//!
//! Per RFC 7011 §8.1, template IDs are only meaningful within one
//! exporter's observation domain, and an exporter that restarts may
//! reuse template IDs to mean something different than before. This
//! module is what lets `wetechinetmon-collector` keep one
//! `TemplateCache` per exporter and clear it when a restart is detected,
//! per docs/functional-requirements.md FR-1.4 ("handle exporter
//! restarts") and FR-1.5.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use wetechinetmon_protocol_ipfix::TemplateCache;

/// State tracked for one exporter, keyed by its source `SocketAddr` in
/// [`ExporterRegistry`].
pub struct ExporterState {
    pub template_cache: TemplateCache,
    pub last_sequence_number: Option<u32>,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub packets_received: u64,
}

impl ExporterState {
    fn new() -> Self {
        let now = Instant::now();
        ExporterState {
            template_cache: TemplateCache::new(),
            last_sequence_number: None,
            first_seen: now,
            last_seen: now,
            packets_received: 0,
        }
    }

    /// Records a newly received message's sequence number for this
    /// exporter, returning `true` if this looks like an exporter restart
    /// (the sequence number regressed), in which case the caller should
    /// treat any templates cached before this call as stale.
    ///
    /// **Known limitation:** this uses a plain regression check
    /// (`new < last`), not RFC 7011's full 32-bit-wraparound-aware
    /// comparison. A real wraparound (the exporter has sent 2^32 records
    /// since it started) will be misreported as a restart. Given typical
    /// export rates this takes a very long time to occur and the
    /// consequence — an unnecessary template-cache clear, causing a few
    /// seconds of "unknown template" until the exporter's next periodic
    /// template refresh — is safe, not silent data corruption. Tracked as
    /// a follow-up rather than adding wraparound-aware comparison logic
    /// this phase doesn't yet have a test fixture to validate against.
    pub fn observe_sequence(&mut self, sequence_number: u32) -> bool {
        self.last_seen = Instant::now();
        self.packets_received += 1;

        let is_restart = match self.last_sequence_number {
            Some(last) => sequence_number < last,
            None => false,
        };

        if is_restart {
            self.template_cache.clear();
        }

        self.last_sequence_number = Some(sequence_number);
        is_restart
    }
}

/// Tracks one [`ExporterState`] per source address that has sent this
/// collector at least one datagram.
#[derive(Default)]
pub struct ExporterRegistry {
    exporters: HashMap<SocketAddr, ExporterState>,
}

impl ExporterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the exporter state for `addr`, creating it if this is the
    /// first time we've seen this address.
    pub fn get_or_create(&mut self, addr: SocketAddr) -> &mut ExporterState {
        self.exporters
            .entry(addr)
            .or_insert_with(ExporterState::new)
    }

    pub fn exporter_count(&self) -> usize {
        self.exporters.len()
    }

    pub fn total_template_count(&self) -> usize {
        self.exporters
            .values()
            .map(|e| e.template_cache.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    #[test]
    fn first_sequence_number_is_never_a_restart() {
        let mut state = ExporterState::new();
        assert!(!state.observe_sequence(0));
        assert_eq!(state.packets_received, 1);
    }

    #[test]
    fn increasing_sequence_numbers_are_not_a_restart() {
        let mut state = ExporterState::new();
        state.observe_sequence(10);
        assert!(!state.observe_sequence(20));
        assert!(!state.observe_sequence(21));
    }

    #[test]
    fn a_regression_is_detected_as_a_restart_and_clears_templates() {
        let mut state = ExporterState::new();
        state.observe_sequence(1000);
        state
            .template_cache
            .insert(wetechinetmon_protocol_ipfix::Template {
                template_id: 256,
                scope_field_count: 0,
                fields: vec![],
            });
        assert_eq!(state.template_cache.len(), 1);

        let restarted = state.observe_sequence(5);
        assert!(restarted);
        assert_eq!(state.template_cache.len(), 0);
    }

    #[test]
    fn registry_creates_state_lazily_per_address() {
        let mut registry = ExporterRegistry::new();
        assert_eq!(registry.exporter_count(), 0);

        registry.get_or_create(addr(1)).observe_sequence(1);
        registry.get_or_create(addr(2)).observe_sequence(1);
        registry.get_or_create(addr(1)).observe_sequence(2);

        assert_eq!(registry.exporter_count(), 2);
    }

    #[test]
    fn total_template_count_sums_across_exporters() {
        let mut registry = ExporterRegistry::new();
        registry.get_or_create(addr(1)).template_cache.insert(
            wetechinetmon_protocol_ipfix::Template {
                template_id: 1,
                scope_field_count: 0,
                fields: vec![],
            },
        );
        registry.get_or_create(addr(2)).template_cache.insert(
            wetechinetmon_protocol_ipfix::Template {
                template_id: 2,
                scope_field_count: 0,
                fields: vec![],
            },
        );
        assert_eq!(registry.total_template_count(), 2);
    }
}
