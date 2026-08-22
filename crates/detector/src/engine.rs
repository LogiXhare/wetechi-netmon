//! The engine: policies in, snapshots in, events out.
//!
//! Everything else in this crate is a component with one job. This is
//! the piece that runs them in order — select a policy, evaluate the
//! thresholds, advance the state machine, build an event, publish it,
//! count what happened — and it is deliberately the only piece that
//! knows that order.
//!
//! # It cannot mitigate
//!
//! Not by policy, by construction. This crate depends on nothing that
//! can open a socket to a router, execute a command, or announce a
//! route. [`ExecutionMode`](crate::policy::ExecutionMode) has no
//! mitigation-capable variant, and a sink receives an event and returns.
//! An operator reading `dryRun` on an event is reading "this is what a
//! later phase would have been asked to do", and the honest reason it
//! did nothing is that there is nothing here that could. See ADR 0007
//! and docs/security/detection-safety.md.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use crate::clock::{Clock, SystemClock};
use crate::evaluate::evaluate;
use crate::event::{DetectionEvent, EventFactory, EventKind};
use crate::input::DetectionSnapshot;
use crate::metrics::{DetectionMetrics, NoopMetrics};
use crate::policy::DetectionPolicy;
use crate::precedence::PolicySet;
use crate::sink::{DetectionEventSink, NullSink};
use crate::state::{
    DetectionState, Expiry, Signal, SignalRecord, StateTable, StateTableConfig, StepIgnored,
};

/// What one pass over the engine did.
///
/// Returned rather than only counted, so a caller can log a single line
/// per cycle and a test can assert without a metrics backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineReport {
    pub snapshots_seen: usize,
    /// Snapshots for which no policy applied. Not an error — most scopes
    /// have no policy most of the time.
    pub unmatched: usize,
    pub ignored: usize,
    pub transitions: usize,
    pub suppressed: usize,
    pub events_built: usize,
    /// Events that reached every sink offered. An `Observe`-mode event
    /// is built but never published, so these two differ by design.
    pub events_published: usize,
    pub events_failed: usize,
    pub detections_opened: usize,
    pub detections_closed: usize,
}

impl EngineReport {
    fn merge(&mut self, other: EngineReport) {
        self.snapshots_seen += other.snapshots_seen;
        self.unmatched += other.unmatched;
        self.ignored += other.ignored;
        self.transitions += other.transitions;
        self.suppressed += other.suppressed;
        self.events_built += other.events_built;
        self.events_published += other.events_published;
        self.events_failed += other.events_failed;
        self.detections_opened += other.detections_opened;
        self.detections_closed += other.detections_closed;
    }
}

/// What a detection engine must be able to do.
///
/// A trait because the community engine here compares against fixed
/// thresholds, and a threshold is not the only way to decide something
/// is wrong. Anything else — a learned baseline, a seasonal model — is a
/// different implementation of this same contract, consuming the same
/// snapshots and producing the same events.
pub trait DetectionEngine {
    /// Feeds a batch of snapshots through selection, evaluation, and the
    /// state machine, publishing whatever events result.
    fn evaluate(&mut self, snapshots: &[DetectionSnapshot]) -> EngineReport;

    /// Reclaims idle scopes and closes detections whose data stopped
    /// arriving. Must be called periodically; nothing else closes a
    /// detection for a scope that has gone silent.
    fn sweep(&mut self, now: Instant, wall: SystemTime) -> EngineReport;

    /// Swaps in a new policy set, closing detections whose policy no
    /// longer applies.
    fn replace_policies(&mut self, policies: PolicySet) -> EngineReport;

    /// How many detections are currently open.
    fn open_detections(&self) -> usize;
}

/// How the community engine is bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineConfig {
    pub state: StateTableConfig,
}

/// The community edition's engine: fixed thresholds, hysteresis, and the
/// four timers.
pub struct ThresholdDetectionEngine {
    policies: PolicySet,
    table: StateTable,
    factory: EventFactory,
    sink: Arc<dyn DetectionEventSink>,
    metrics: Arc<dyn DetectionMetrics>,
    clock: Arc<dyn Clock>,
}

impl ThresholdDetectionEngine {
    /// An engine that evaluates but publishes nowhere and counts
    /// nothing. Useful as a starting point, and as the shape a test
    /// wants before it attaches its own sink.
    pub fn new(config: EngineConfig) -> Self {
        ThresholdDetectionEngine {
            policies: PolicySet::new(),
            table: StateTable::new(config.state),
            factory: EventFactory::new(),
            sink: Arc::new(NullSink),
            metrics: Arc::new(NoopMetrics),
            clock: Arc::new(SystemClock),
        }
    }

    pub fn with_policies(mut self, policies: PolicySet) -> Self {
        self.policies = policies;
        self
    }

    pub fn with_sink(mut self, sink: Arc<dyn DetectionEventSink>) -> Self {
        self.sink = sink;
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<dyn DetectionMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// A factory with a fixed instance id, so a test can assert on the
    /// exact identifiers an engine produces.
    pub fn with_event_factory(mut self, factory: EventFactory) -> Self {
        self.factory = factory;
        self
    }

    pub fn policies(&self) -> &PolicySet {
        &self.policies
    }

    pub fn state(&self) -> &StateTable {
        &self.table
    }

    /// Sweeps using the engine's clock rather than a caller-supplied
    /// time. The production entry point; tests use [`DetectionEngine::sweep`]
    /// with an explicit instant.
    pub fn sweep_now(&mut self) -> EngineReport {
        let now = self.clock.monotonic();
        let wall = self.clock.wall();
        self.sweep(now, wall)
    }

    /// Publishes an event if its mode allows, counting either way.
    fn emit(&self, event: DetectionEvent, report: &mut EngineReport) {
        report.events_built += 1;
        self.metrics.event_built(event.kind);
        match event.kind {
            EventKind::Started => report.detections_opened += 1,
            EventKind::Ended => report.detections_closed += 1,
            EventKind::Updated => {}
        }
        if !event.is_publishable() {
            return;
        }
        match self.sink.publish(&event) {
            Ok(()) => {
                report.events_published += 1;
                self.metrics.event_published(event.kind);
            }
            Err(error) => {
                report.events_failed += 1;
                self.metrics.event_failed(event.kind, self.sink.name());
                // A failed publish is an alert nobody received. It is
                // logged at error level and counted, never swallowed.
                tracing::error!(
                    detection_id = %event.detection_id,
                    event_id = %event.event_id,
                    kind = event.kind.as_str(),
                    %error,
                    "detection event could not be published"
                );
            }
        }
    }

    /// Turns the expiries from a sweep or a policy swap into end events.
    ///
    /// `policy_for` looks the policy up in whichever set was in force
    /// when the detection was open — which is not always the current
    /// one, since a policy swap closes detections precisely because
    /// their policy has gone.
    fn close_expired<F>(
        &self,
        expiries: Vec<Expiry>,
        wall: SystemTime,
        report: &mut EngineReport,
        policy_for: F,
    ) where
        F: Fn(&str) -> Option<DetectionPolicy>,
    {
        for expiry in expiries {
            let (Some(transition), Some(detection)) = (expiry.transition, expiry.detection) else {
                continue;
            };
            if expiry.signal != Some(Signal::Ended) {
                continue;
            }
            report.transitions += 1;
            self.metrics
                .transition(transition.from, transition.to, transition.reason);
            let Some(policy) = policy_for(&transition.policy_id) else {
                // The policy is gone and nothing remembers its severity
                // or labels, so an event built from it would be a
                // fabrication. The state is still cleaned up; the loss
                // is one end event, and it is counted.
                tracing::warn!(
                    policy = %transition.policy_id,
                    target = %transition.key.scope_id,
                    "detection closed but its policy is no longer known; no end event was built"
                );
                report.events_failed += 1;
                continue;
            };
            let record = SignalRecord {
                signal: Signal::Ended,
                transition,
                detection,
            };
            if let Some(event) = self.factory.build_closing(&record, &policy, wall) {
                self.emit(event, report);
            }
        }
    }

    fn report_gauges(&self) {
        for state in [
            DetectionState::Idle,
            DetectionState::PendingTrigger,
            DetectionState::Active,
            DetectionState::PendingClear,
            DetectionState::Cooldown,
        ] {
            self.metrics
                .scopes_in_state(state, self.table.count_in(state));
        }
    }
}

impl DetectionEngine for ThresholdDetectionEngine {
    fn evaluate(&mut self, snapshots: &[DetectionSnapshot]) -> EngineReport {
        let mut report = EngineReport {
            snapshots_seen: snapshots.len(),
            ..EngineReport::default()
        };

        for snapshot in snapshots {
            self.metrics.snapshot_evaluated(snapshot.key.scope_type);
            let Some(policy) = self.policies.winner_for(&snapshot.key).cloned() else {
                report.unmatched += 1;
                self.metrics.snapshot_unmatched(snapshot.key.scope_type);
                continue;
            };

            let evaluation = evaluate(&policy, snapshot);
            for _ in &evaluation.skipped {
                self.metrics.metric_skipped();
            }

            let outcome = self.table.step(&policy, snapshot, &evaluation);
            if let Some(reason) = outcome.ignored {
                report.ignored += 1;
                self.metrics.snapshot_ignored(reason);
                if reason == StepIgnored::TableFull {
                    self.metrics.state_table_full();
                }
            }
            if let Some(suppression) = outcome.suppressed {
                report.suppressed += 1;
                self.metrics.suppressed(suppression);
            }
            for transition in &outcome.transitions {
                report.transitions += 1;
                self.metrics
                    .transition(transition.from, transition.to, transition.reason);
            }
            if let Some(record) = outcome.signal.as_ref() {
                if let Some(event) = self.factory.build(
                    record,
                    &policy,
                    snapshot,
                    &evaluation.matched,
                    &evaluation.skipped,
                ) {
                    self.emit(event, &mut report);
                }
            }
        }

        self.report_gauges();
        report
    }

    fn sweep(&mut self, now: Instant, wall: SystemTime) -> EngineReport {
        let mut report = EngineReport::default();
        let expiries = self.table.sweep(now, wall);
        let stale = expiries
            .iter()
            .filter(|expiry| expiry.signal == Some(Signal::Ended))
            .count();
        for _ in 0..stale {
            self.metrics.detection_stale();
        }
        let policies = self.policies.clone();
        self.close_expired(expiries, wall, &mut report, |id| policies.get(id).cloned());
        self.report_gauges();
        report
    }

    fn replace_policies(&mut self, policies: PolicySet) -> EngineReport {
        let mut report = EngineReport::default();
        let now = self.clock.monotonic();
        let wall = self.clock.wall();

        // The outgoing set is what the open detections were opened
        // under, so it — not the incoming one — is where their end
        // events must read severity and labels from.
        let outgoing = std::mem::replace(&mut self.policies, policies);
        let incoming = self.policies.clone();
        let expiries = self.table.withdraw_stale_selection(now, wall, |key| {
            incoming
                .winner_for(key)
                .map(|policy| (policy.id.clone(), policy.version))
        });
        self.close_expired(expiries, wall, &mut report, |id| outgoing.get(id).cloned());

        self.report_gauges();
        report
    }

    fn open_detections(&self) -> usize {
        self.table.open_detections().len()
    }
}

/// Runs `evaluate` and then `sweep`, merging both reports.
///
/// The order matters: evaluating first means a snapshot that arrives in
/// the same cycle as the sweep still counts, so a scope is never
/// declared stale in the same breath as the data proving it is not.
pub fn cycle<E: DetectionEngine>(
    engine: &mut E,
    snapshots: &[DetectionSnapshot],
    now: Instant,
    wall: SystemTime,
) -> EngineReport {
    let mut report = engine.evaluate(snapshots);
    report.merge(engine.sweep(now, wall));
    report
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use super::*;
    use crate::event::EventKind;
    use crate::input::{
        AddressFamily, DataCompleteness, MetricKind, MetricRates, SamplingStatus, ScopeId,
        ScopeKey, ScopeType, TrafficDirection,
    };
    use crate::metrics::CountingMetrics;
    use crate::policy::{
        ExecutionMode, PolicyDraft, PolicySelector, Severity, TenantPrefixes, Thresholds,
    };
    use crate::sink::InMemorySink;

    fn key() -> ScopeKey {
        ScopeKey {
            tenant: "acme".to_string(),
            scope_type: ScopeType::Host,
            scope_id: ScopeId::Host {
                addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
            },
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        }
    }

    fn draft(id: &str) -> PolicyDraft {
        PolicyDraft {
            id: id.to_string(),
            name: "host bps".to_string(),
            description: None,
            enabled: true,
            tenant: "acme".to_string(),
            scope_type: ScopeType::Host,
            selector: PolicySelector::Any,
            address_family: None,
            direction: TrafficDirection::Incoming,
            window: Duration::from_secs(1),
            thresholds: Thresholds::new().with(MetricKind::Bps, 1_000_000),
            clear_percent: 80,
            trigger_for: Duration::from_secs(2),
            clear_for: Duration::from_secs(2),
            cooldown: Duration::from_secs(10),
            hold_down: Duration::ZERO,
            event_update_interval: Duration::from_secs(30),
            severity: Severity::Major,
            execution_mode: ExecutionMode::AlertOnly,
            priority: 0,
            labels: BTreeMap::new(),
            version: 1,
        }
    }

    fn policy_set(drafts: Vec<PolicyDraft>) -> PolicySet {
        let policies: Vec<_> = drafts
            .into_iter()
            .map(|d| d.validate(&TenantPrefixes::new()).expect("valid"))
            .collect();
        PolicySet::from_policies(policies).expect("no duplicate ids")
    }

    fn snapshot(at: Instant, bps: u64) -> DetectionSnapshot {
        DetectionSnapshot {
            key: key(),
            window: Duration::from_secs(1),
            observed_at: at,
            observed_wall: SystemTime::UNIX_EPOCH,
            rates: MetricRates {
                bps,
                ..MetricRates::default()
            },
            completeness: DataCompleteness {
                protocol_seen: true,
                tcp_flags_seen: true,
                fragmentation_seen: true,
                forwarding_status_seen: true,
            },
            sampling: SamplingStatus::default(),
            flows_observed: 5,
            exporters_observed: 1,
        }
    }

    struct Harness {
        engine: ThresholdDetectionEngine,
        sink: Arc<InMemorySink>,
        metrics: Arc<CountingMetrics>,
        start: Instant,
    }

    fn harness(drafts: Vec<PolicyDraft>) -> Harness {
        let sink = Arc::new(InMemorySink::new(64));
        let metrics = Arc::new(CountingMetrics::new());
        let engine = ThresholdDetectionEngine::new(EngineConfig::default())
            .with_policies(policy_set(drafts))
            .with_sink(sink.clone())
            .with_metrics(metrics.clone());
        Harness {
            engine,
            sink,
            metrics,
            start: Instant::now(),
        }
    }

    impl Harness {
        fn feed(&mut self, offset_secs: u64, bps: u64) -> EngineReport {
            let snapshot = snapshot(self.start + Duration::from_secs(offset_secs), bps);
            self.engine.evaluate(&[snapshot])
        }

        fn sweep(&mut self, offset_secs: u64) -> EngineReport {
            self.engine.sweep(
                self.start + Duration::from_secs(offset_secs),
                SystemTime::UNIX_EPOCH,
            )
        }
    }

    #[test]
    fn a_snapshot_with_no_policy_is_counted_not_dropped_silently() {
        let mut h = harness(Vec::new());
        let report = h.feed(0, 900_000_000);
        assert_eq!(report.snapshots_seen, 1);
        assert_eq!(report.unmatched, 1);
        assert_eq!(report.events_built, 0);
        assert!(h.sink.is_empty());
        assert_eq!(CountingMetrics::get(&h.metrics.snapshots_unmatched), 1);
    }

    #[test]
    fn a_sustained_crossing_publishes_a_start_event() {
        let mut h = harness(vec![draft("p1")]);
        assert_eq!(h.feed(0, 5_000_000).events_built, 0);
        let report = h.feed(2, 5_000_000);
        assert_eq!(report.events_built, 1);
        assert_eq!(report.events_published, 1);
        assert_eq!(report.detections_opened, 1);
        assert_eq!(h.engine.open_detections(), 1);

        let events = h.sink.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::Started);
        assert_eq!(events[0].policy_id, "p1");
        assert_eq!(CountingMetrics::get(&h.metrics.events_published), 1);
    }

    #[test]
    fn a_full_detection_publishes_exactly_one_start_and_one_end() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        h.feed(3, 100);
        h.feed(5, 100);

        let events = h.sink.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::Started);
        assert_eq!(events[1].kind, EventKind::Ended);
        assert_eq!(events[0].detection_id, events[1].detection_id);
        assert_eq!(h.engine.open_detections(), 0);
    }

    #[test]
    fn an_observe_policy_builds_an_event_but_publishes_nothing() {
        let mut observe = draft("p1");
        observe.execution_mode = ExecutionMode::Observe;
        let mut h = harness(vec![observe]);
        h.feed(0, 5_000_000);
        let report = h.feed(2, 5_000_000);
        assert_eq!(report.events_built, 1);
        assert_eq!(report.events_published, 0);
        assert!(h.sink.is_empty());
        assert_eq!(CountingMetrics::get(&h.metrics.events_built), 1);
        assert_eq!(CountingMetrics::get(&h.metrics.events_published), 0);
    }

    #[test]
    fn a_disabled_policy_never_wins_selection() {
        let mut disabled = draft("p1");
        disabled.enabled = false;
        let mut h = harness(vec![disabled]);
        let report = h.feed(0, 900_000_000);
        assert_eq!(report.unmatched, 1);
        assert!(h.sink.is_empty());
    }

    #[test]
    fn the_more_specific_policy_wins_and_its_id_is_on_the_event() {
        let mut specific = draft("p-host");
        specific.selector = PolicySelector::Host {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
        };
        specific.thresholds = Thresholds::new().with(MetricKind::Bps, 100_000_000);
        let mut h = harness(vec![draft("p-any"), specific]);

        // Above the tenant default but below the host-specific
        // threshold, so the specific policy winning means silence.
        h.feed(0, 5_000_000);
        let report = h.feed(2, 5_000_000);
        assert_eq!(report.events_built, 0);

        h.feed(4, 200_000_000);
        let report = h.feed(6, 200_000_000);
        assert_eq!(report.events_built, 1);
        assert_eq!(h.sink.events()[0].policy_id, "p-host");
    }

    #[test]
    fn a_stale_detection_is_closed_by_the_sweep() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        assert_eq!(h.engine.open_detections(), 1);

        let report = h.sweep(200);
        assert_eq!(report.detections_closed, 1);
        assert_eq!(h.engine.open_detections(), 0);

        let events = h.sink.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].kind, EventKind::Ended);
        assert_eq!(events[1].reason, crate::state::TransitionReason::Stale);
        assert_eq!(
            events[1].rates.bps, 0,
            "a stale close must not report the last rate as if it were current"
        );
        assert_eq!(events[1].peak[0].observed, 5_000_000);
        assert_eq!(CountingMetrics::get(&h.metrics.detections_stale), 1);
    }

    #[test]
    fn a_sweep_with_nothing_stale_does_nothing() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        let report = h.sweep(3);
        assert_eq!(report, EngineReport::default());
        assert_eq!(h.engine.open_detections(), 1);
    }

    #[test]
    fn withdrawing_a_policy_closes_the_detections_it_opened() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        assert_eq!(h.engine.open_detections(), 1);

        let report = h.engine.replace_policies(PolicySet::new());
        assert_eq!(report.detections_closed, 1);
        assert_eq!(h.engine.open_detections(), 0);

        let events = h.sink.events();
        assert_eq!(events[1].kind, EventKind::Ended);
        assert_eq!(
            events[1].reason,
            crate::state::TransitionReason::PolicyWithdrawn
        );
        assert_eq!(
            events[1].severity,
            Severity::Major,
            "the end event must read severity from the policy that opened the detection"
        );
    }

    #[test]
    fn replacing_a_policy_with_one_that_still_matches_leaves_the_detection_open() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        let mut next = draft("p1");
        next.severity = Severity::Critical;
        let report = h.engine.replace_policies(policy_set(vec![next]));
        assert_eq!(report.detections_closed, 0);
        assert_eq!(h.engine.open_detections(), 1);
    }

    #[test]
    fn a_version_bump_closes_the_open_detection_at_swap_time() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);

        let mut next = draft("p1");
        next.version = 2;
        next.severity = Severity::Critical;
        let report = h.engine.replace_policies(policy_set(vec![next]));
        assert_eq!(report.detections_closed, 1);
        assert_eq!(h.engine.open_detections(), 0);

        let events = h.sink.events();
        assert_eq!(events[1].kind, EventKind::Ended);
        assert_eq!(
            events[1].reason,
            crate::state::TransitionReason::PolicyChanged
        );
        assert_eq!(
            events[1].policy_version, 1,
            "the end event belongs to the version that opened the detection"
        );
        assert_eq!(
            events[1].severity,
            Severity::Major,
            "and to that version's severity, not the replacement's"
        );
    }

    #[test]
    fn a_reopened_detection_after_a_version_bump_carries_the_new_version() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        let mut next = draft("p1");
        next.version = 2;
        h.engine.replace_policies(policy_set(vec![next]));

        h.feed(3, 5_000_000);
        let report = h.feed(5, 5_000_000);
        assert_eq!(report.detections_opened, 1);
        let events = h.sink.events();
        assert_eq!(events[2].kind, EventKind::Started);
        assert_eq!(events[2].policy_version, 2);
    }

    #[test]
    fn a_publish_failure_is_counted_and_never_swallowed() {
        let sink = Arc::new(InMemorySink::new(0));
        let metrics = Arc::new(CountingMetrics::new());
        let mut engine = ThresholdDetectionEngine::new(EngineConfig::default())
            .with_policies(policy_set(vec![draft("p1")]))
            .with_sink(sink.clone())
            .with_metrics(metrics.clone());
        let start = Instant::now();
        engine.evaluate(&[snapshot(start, 5_000_000)]);
        let report = engine.evaluate(&[snapshot(start + Duration::from_secs(2), 5_000_000)]);
        assert_eq!(report.events_built, 1);
        assert_eq!(report.events_published, 0);
        assert_eq!(report.events_failed, 1);
        assert_eq!(CountingMetrics::get(&metrics.events_failed), 1);
    }

    #[test]
    fn gauges_are_refreshed_on_every_pass() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        assert_eq!(h.metrics.active_scopes(), 0);
        h.feed(2, 5_000_000);
        assert_eq!(h.metrics.active_scopes(), 1);
        h.feed(3, 100);
        h.feed(5, 100);
        assert_eq!(h.metrics.active_scopes(), 0);
    }

    #[test]
    fn suppression_during_cooldown_is_reported() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        h.feed(3, 100);
        h.feed(5, 100);
        let report = h.feed(6, 9_000_000);
        assert_eq!(report.suppressed, 1);
        assert_eq!(report.events_built, 0);
        assert_eq!(CountingMetrics::get(&h.metrics.suppressions), 1);
    }

    #[test]
    fn an_out_of_order_snapshot_is_reported_as_ignored() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(5, 5_000_000);
        let report = h.feed(1, 5_000_000);
        assert_eq!(report.ignored, 1);
        assert_eq!(CountingMetrics::get(&h.metrics.snapshots_ignored), 1);
    }

    #[test]
    fn a_full_state_table_reports_a_dropped_detection() {
        let sink = Arc::new(InMemorySink::new(8));
        let metrics = Arc::new(CountingMetrics::new());
        let mut engine = ThresholdDetectionEngine::new(EngineConfig {
            state: StateTableConfig {
                max_entries: 1,
                ..StateTableConfig::default()
            },
        })
        .with_policies(policy_set(vec![draft("p1")]))
        .with_sink(sink)
        .with_metrics(metrics.clone());

        let start = Instant::now();
        engine.evaluate(&[snapshot(start, 5_000_000)]);
        let mut other = snapshot(start, 5_000_000);
        other.key.scope_id = ScopeId::Host {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)),
        };
        let report = engine.evaluate(&[other]);
        assert_eq!(report.ignored, 1);
        assert_eq!(CountingMetrics::get(&metrics.state_table_full), 1);
    }

    #[test]
    fn a_skipped_metric_is_counted() {
        let mut with_syn = draft("p1");
        with_syn.thresholds = Thresholds::new()
            .with(MetricKind::Bps, 1_000_000)
            .with(MetricKind::TcpSynPps, 1000);
        let mut h = harness(vec![with_syn]);
        let mut snapshot = snapshot(h.start, 5_000_000);
        snapshot.completeness.tcp_flags_seen = false;
        h.engine.evaluate(&[snapshot]);
        assert_eq!(CountingMetrics::get(&h.metrics.metrics_skipped), 1);
    }

    #[test]
    fn a_cycle_evaluates_before_it_sweeps() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);

        // Far past the stale timeout, but the snapshot in the same cycle
        // proves the scope is alive, so nothing is closed.
        let at = h.start + Duration::from_secs(400);
        let report = cycle(
            &mut h.engine,
            &[snapshot(at, 5_000_000)],
            at,
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(report.detections_closed, 0);
        assert_eq!(h.engine.open_detections(), 1);
    }

    #[test]
    fn a_cycle_with_no_snapshots_still_sweeps() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        let at = h.start + Duration::from_secs(400);
        let report = cycle(&mut h.engine, &[], at, SystemTime::UNIX_EPOCH);
        assert_eq!(report.detections_closed, 1);
    }

    #[test]
    fn an_engine_with_a_test_clock_sweeps_at_the_time_it_is_told() {
        let clock = Arc::new(crate::clock::TestClock::new());
        let sink = Arc::new(InMemorySink::new(8));
        let mut engine = ThresholdDetectionEngine::new(EngineConfig::default())
            .with_policies(policy_set(vec![draft("p1")]))
            .with_sink(sink.clone())
            .with_clock(clock.clone());

        let start = clock.monotonic();
        engine.evaluate(&[snapshot(start, 5_000_000)]);
        engine.evaluate(&[snapshot(start + Duration::from_secs(2), 5_000_000)]);
        assert_eq!(engine.open_detections(), 1);

        assert_eq!(engine.sweep_now().detections_closed, 0);
        clock.advance(Duration::from_secs(400));
        assert_eq!(engine.sweep_now().detections_closed, 1);
    }

    #[test]
    fn every_event_from_one_engine_shares_the_instance_prefix() {
        let mut h = harness(vec![draft("p1")]);
        h.feed(0, 5_000_000);
        h.feed(2, 5_000_000);
        h.feed(3, 100);
        h.feed(5, 100);
        let events = h.sink.events();
        let prefix = |id: &str| id[..16].to_string();
        assert_eq!(prefix(&events[0].event_id), prefix(&events[1].event_id));
        assert_ne!(events[0].event_id, events[1].event_id);
    }
}
