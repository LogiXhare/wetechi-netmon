//! Detection events: what leaves the engine.
//!
//! An event is the only thing anyone downstream sees, so it has to be
//! self-contained. Reading one should answer "what was detected, on
//! what, under which policy, by how much, and how sure are we" without
//! joining against configuration that may since have changed. That is
//! why the policy id *and version*, the thresholds actually compared
//! against, and the data-completeness flags all travel on the event
//! rather than being looked up later.
//!
//! # Identity
//!
//! Three identifiers, doing three different jobs:
//!
//! - `event_id` is unique per event. Nothing is keyed on it.
//! - `detection_id` is stable from the start event to the end event of
//!   one detection. This is the join key.
//! - `dedup_key` is what an at-least-once transport should collapse on.
//!   Redelivering an event produces a byte-identical key.
//!
//! None of them is a UUID. This crate has no random-number dependency
//! and adding one to produce identifiers that are never used as
//! cryptographic material would be a poor trade — see
//! docs/architecture/decisions/0009-detection-event-identity.md.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::evaluate::{MatchedReason, SkippedMetric};
use crate::input::{
    AddressFamily, DataCompleteness, DetectionSnapshot, MetricRates, SamplingStatus, ScopeId,
    ScopeType, TrafficDirection,
};
use crate::policy::{DetectionPolicy, ExecutionMode, Severity};
use crate::state::{DetectionState, Signal, SignalRecord, TransitionReason};

/// Bumped when a field changes meaning or disappears. Adding an
/// optional field does not bump it; a consumer that ignores unknown
/// fields keeps working. Stamped on every event so a consumer can refuse
/// what it does not understand instead of misreading it.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Where an event sits in a detection's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Started,
    Updated,
    Ended,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventKind::Started => "started",
            EventKind::Updated => "updated",
            EventKind::Ended => "ended",
        }
    }
}

impl From<Signal> for EventKind {
    fn from(signal: Signal) -> Self {
        match signal {
            Signal::Started => EventKind::Started,
            Signal::Updated => EventKind::Updated,
            Signal::Ended => EventKind::Ended,
        }
    }
}

/// What the engine was permitted to do about this event.
///
/// Recorded on the event because "we would have blocked this" and "we
/// blocked this" must never be indistinguishable in an audit trail.
/// Phase 4 can only ever record the first — see ADR 0007.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionTaken {
    /// Recorded only; not published to any sink.
    Observed,
    /// Published as an alert. No mitigation was requested or performed.
    Alerted,
    /// Published, and the event names the mitigation that a later phase
    /// *would* have requested. Nothing was requested, and this crate has
    /// no means to request anything.
    DryRun,
}

impl ActionTaken {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionTaken::Observed => "observed",
            ActionTaken::Alerted => "alerted",
            ActionTaken::DryRun => "dryRun",
        }
    }

    /// The action a given execution mode produces.
    ///
    /// `Disabled` produces no event at all, so it has no action.
    pub fn for_mode(mode: ExecutionMode) -> Option<Self> {
        match mode {
            ExecutionMode::Disabled => None,
            ExecutionMode::Observe => Some(ActionTaken::Observed),
            ExecutionMode::AlertOnly => Some(ActionTaken::Alerted),
            ExecutionMode::DryRun => Some(ActionTaken::DryRun),
        }
    }

    /// Whether anything was actually done to the traffic.
    ///
    /// Exhaustively `false`, and the match is deliberately not a
    /// catch-all: a later phase adding a variant that *does* act has to
    /// come here and say so, rather than inheriting `false` by default
    /// and silently reporting a mitigation as having not happened.
    pub fn executed(&self) -> bool {
        match self {
            ActionTaken::Observed => false,
            ActionTaken::Alerted => false,
            ActionTaken::DryRun => false,
        }
    }
}

/// The scope an event is about, flattened for consumers that cannot
/// handle a tagged union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTarget {
    pub tenant: String,
    pub scope_type: ScopeType,
    pub scope_id: ScopeId,
    /// Text form of `scope_id`, so a log line is readable without
    /// reconstructing the union.
    pub display: String,
    pub direction: TrafficDirection,
    pub address_family: AddressFamily,
}

/// One detection event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub detection_id: String,
    /// Zero on the start event, incrementing for each further event of
    /// the same detection.
    pub sequence: u64,
    pub kind: EventKind,
    pub dedup_key: String,

    pub policy_id: String,
    pub policy_name: String,
    pub policy_version: u32,
    pub severity: Severity,
    pub execution_mode: ExecutionMode,
    pub action: ActionTaken,
    pub labels: std::collections::BTreeMap<String, String>,

    pub target: EventTarget,

    pub previous_state: DetectionState,
    pub state: DetectionState,
    pub reason: TransitionReason,

    /// Milliseconds since the Unix epoch. Wall clock, for display and
    /// storage only — every duration on this event was measured with a
    /// monotonic clock and is reported directly rather than derived from
    /// these two fields.
    pub detected_at_ms: u64,
    pub observed_at_ms: u64,
    /// How long the detection had been open when this event was
    /// produced. Zero on the start event.
    pub duration_ms: u64,
    /// The rate window these figures were measured over.
    pub window_ms: u64,

    /// Thresholds crossed at this instant. Empty on an end event, which
    /// is the point: the detection ended because nothing was crossed.
    pub matched: Vec<MatchedReason>,
    /// The worst value seen for each crossed metric across the whole
    /// detection. This is what an operator quotes in a post-mortem.
    pub peak: Vec<MatchedReason>,
    /// Thresholds that could not be evaluated, and why.
    pub skipped: Vec<SkippedMetric>,
    pub rates: MetricRates,

    pub completeness: DataCompleteness,
    pub sampling: SamplingStatus,
    pub flows_observed: u64,
    pub exporters_observed: u32,
    pub snapshots_in_detection: u64,

    /// Whether anything was actually done to the traffic this event
    /// describes.
    ///
    /// **Always `false` in Phase 4**, and structurally so: it is derived
    /// from [`ActionTaken::executed`], and no `ActionTaken` variant
    /// returns `true`. It is written onto every event anyway, because an
    /// audit trail that states the fact is worth more than one where the
    /// fact must be inferred from the absence of a field — a consumer
    /// reading the JSON should not have to know which product version
    /// could have acted.
    ///
    /// See ADR 0007 and docs/security/detection-safety.md.
    pub executed: bool,

    /// One line, safe to put in a subject header or a chat message.
    pub summary: String,
}

impl DetectionEvent {
    /// Whether this event should be published, given its mode.
    ///
    /// `Observe` events exist — they are counted, logged at debug, and
    /// available to tests — but they do not reach a sink.
    pub fn is_publishable(&self) -> bool {
        self.execution_mode.emits_events()
    }
}

/// Mints identifiers and assembles events.
///
/// One per running engine. The instance component of every identifier
/// comes from this factory, so two engines processing the same traffic
/// cannot mint colliding ids.
#[derive(Debug)]
pub struct EventFactory {
    instance_id: u64,
    /// Fixed reference point for turning an `Instant` into a number.
    /// `Instant` has no epoch, so detection identity needs one.
    origin: Instant,
    counter: AtomicU64,
}

impl EventFactory {
    /// Seeds a factory from the ambient environment.
    pub fn new() -> Self {
        EventFactory::with_instance_id(seed_instance_id())
    }

    /// A factory with a fixed instance id, so a test can assert on the
    /// exact identifiers produced.
    pub fn with_instance_id(instance_id: u64) -> Self {
        EventFactory {
            instance_id,
            origin: Instant::now(),
            counter: AtomicU64::new(0),
        }
    }

    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// Stable for the whole life of one detection.
    ///
    /// Derived from the instance, the scope, and when the detection
    /// opened. A scope cannot open two detections at the same instant,
    /// so the triple is unique without a counter — which matters,
    /// because the end event is minted from the same inputs as the start
    /// event and must produce the same string.
    pub fn detection_id(&self, target: &EventTarget, started_at: Instant) -> String {
        let mut hasher = DefaultHasher::new();
        self.instance_id.hash(&mut hasher);
        target.tenant.hash(&mut hasher);
        target.scope_type.hash(&mut hasher);
        target.scope_id.hash(&mut hasher);
        target.direction.hash(&mut hasher);
        target.address_family.hash(&mut hasher);
        started_at
            .saturating_duration_since(self.origin)
            .as_nanos()
            .hash(&mut hasher);
        format!("{:016x}{:016x}", self.instance_id, hasher.finish())
    }

    /// Unique per event, monotonic within one engine.
    pub fn next_event_id(&self) -> String {
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{:016x}-{seq:016x}", self.instance_id)
    }

    /// Builds the event for one signal.
    ///
    /// Returns `None` when the policy's mode produces no event at all,
    /// which is only `Disabled` — and a disabled policy is never
    /// evaluated, so this is a guard against a future caller, not a
    /// path that runs today.
    pub fn build(
        &self,
        record: &SignalRecord,
        policy: &DetectionPolicy,
        snapshot: &DetectionSnapshot,
        matched: &[MatchedReason],
        skipped: &[SkippedMetric],
    ) -> Option<DetectionEvent> {
        let action = ActionTaken::for_mode(policy.execution_mode)?;
        let transition = &record.transition;
        let context = &record.detection;
        let kind = EventKind::from(record.signal);
        let target = EventTarget {
            tenant: snapshot.key.tenant.clone(),
            scope_type: snapshot.key.scope_type,
            scope_id: snapshot.key.scope_id.clone(),
            display: snapshot.key.scope_id.to_string(),
            direction: snapshot.key.direction,
            address_family: snapshot.key.address_family,
        };
        let detection_id = self.detection_id(&target, context.started_at);
        let duration = snapshot
            .observed_at
            .saturating_duration_since(context.started_at);
        let summary = summarize(&target, policy, kind, matched, &context.peak, duration);

        Some(DetectionEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            event_id: self.next_event_id(),
            dedup_key: dedup_key(&detection_id, kind, context.sequence),
            detection_id,
            sequence: context.sequence,
            kind,

            // Identity comes from the transition, not the policy
            // argument: the transition records what was actually in
            // force when the machine moved, which for a close triggered
            // by a policy rewrite is not the policy now loaded.
            policy_id: transition.policy_id.clone(),
            policy_version: transition.policy_version,
            policy_name: policy.name.clone(),
            severity: policy.severity,
            execution_mode: policy.execution_mode,
            action,
            labels: policy.labels.clone(),

            target,

            previous_state: transition.from,
            state: transition.to,
            reason: transition.reason,

            detected_at_ms: unix_millis(context.started_wall),
            observed_at_ms: unix_millis(snapshot.observed_wall),
            duration_ms: duration.as_millis().min(u64::MAX as u128) as u64,
            window_ms: snapshot.window.as_millis().min(u64::MAX as u128) as u64,

            matched: matched.to_vec(),
            peak: context.peak.clone(),
            skipped: skipped.to_vec(),
            rates: snapshot.rates,

            completeness: snapshot.completeness,
            sampling: snapshot.sampling,
            flows_observed: snapshot.flows_observed,
            exporters_observed: snapshot.exporters_observed,
            snapshots_in_detection: context.snapshots_in_detection,

            executed: action.executed(),
            summary,
        })
    }

    /// Builds an end event for a detection that was closed without a
    /// snapshot — because its data stopped arriving, or because its
    /// policy went away.
    ///
    /// Every observation field is left at its zero value rather than
    /// carrying the last figures seen. Reporting a stale rate as if it
    /// were current is how a post-mortem ends up describing traffic that
    /// had already stopped; the `reason` on the event says what actually
    /// happened, and `peak` still records how bad it got.
    pub fn build_closing(
        &self,
        record: &SignalRecord,
        policy: &DetectionPolicy,
        closed_wall: SystemTime,
    ) -> Option<DetectionEvent> {
        let action = ActionTaken::for_mode(policy.execution_mode)?;
        let transition = &record.transition;
        let context = &record.detection;
        let key = &transition.key;
        let target = EventTarget {
            tenant: key.tenant.clone(),
            scope_type: key.scope_type,
            scope_id: key.scope_id.clone(),
            display: key.scope_id.to_string(),
            direction: key.direction,
            address_family: key.address_family,
        };
        let detection_id = self.detection_id(&target, context.started_at);
        let duration = transition.at.saturating_duration_since(context.started_at);
        let summary = format!(
            "ended ({}): {} {} {} after {}s under policy {}",
            transition.reason.as_str(),
            target.direction.as_str(),
            target.scope_type.as_str(),
            target.display,
            duration.as_secs(),
            policy.id
        );

        Some(DetectionEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            event_id: self.next_event_id(),
            dedup_key: dedup_key(&detection_id, EventKind::Ended, context.sequence),
            detection_id,
            sequence: context.sequence,
            kind: EventKind::Ended,

            policy_id: transition.policy_id.clone(),
            policy_version: transition.policy_version,
            policy_name: policy.name.clone(),
            severity: policy.severity,
            execution_mode: policy.execution_mode,
            action,
            labels: policy.labels.clone(),

            target,

            previous_state: transition.from,
            state: transition.to,
            reason: transition.reason,

            detected_at_ms: unix_millis(context.started_wall),
            observed_at_ms: unix_millis(closed_wall),
            duration_ms: duration.as_millis().min(u64::MAX as u128) as u64,
            window_ms: policy.window.as_millis().min(u64::MAX as u128) as u64,

            matched: Vec::new(),
            peak: context.peak.clone(),
            skipped: Vec::new(),
            rates: MetricRates::default(),

            completeness: DataCompleteness::default(),
            sampling: SamplingStatus::default(),
            flows_observed: 0,
            exporters_observed: 0,
            snapshots_in_detection: context.snapshots_in_detection,

            executed: action.executed(),
            summary,
        })
    }
}

impl Default for EventFactory {
    fn default() -> Self {
        EventFactory::new()
    }
}

/// What an at-least-once transport collapses on.
///
/// The detection, the kind, and the position within the detection. Two
/// deliveries of the same event agree on all three; two genuinely
/// different events never do, because `sequence` advances on every
/// event the state machine produces.
pub fn dedup_key(detection_id: &str, kind: EventKind, sequence: u64) -> String {
    format!("{detection_id}:{}:{sequence}", kind.as_str())
}

/// Milliseconds since the Unix epoch, saturating at zero for a clock
/// set before 1970 rather than panicking on the negative duration.
fn unix_millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// A one-line description an operator can read at 3am.
fn summarize(
    target: &EventTarget,
    policy: &DetectionPolicy,
    kind: EventKind,
    matched: &[MatchedReason],
    peak: &[MatchedReason],
    duration: Duration,
) -> String {
    let subject = format!(
        "{} {} {}",
        target.direction.as_str(),
        target.scope_type.as_str(),
        target.display
    );
    match kind {
        EventKind::Started | EventKind::Updated => {
            let worst = matched
                .iter()
                .max_by_key(|reason| reason.ratio_percent)
                .or_else(|| peak.iter().max_by_key(|reason| reason.ratio_percent));
            match worst {
                Some(reason) => format!(
                    "{} {}: {} at {} {} ({}% of the {} threshold) under policy {}",
                    policy.severity.as_str(),
                    kind.as_str(),
                    subject,
                    reason.observed,
                    reason.metric.unit().as_str(),
                    reason.ratio_percent,
                    reason.metric.as_str(),
                    policy.id
                ),
                None => format!(
                    "{} {}: {} under policy {}",
                    policy.severity.as_str(),
                    kind.as_str(),
                    subject,
                    policy.id
                ),
            }
        }
        EventKind::Ended => {
            let worst = peak.iter().max_by_key(|reason| reason.ratio_percent);
            match worst {
                Some(reason) => format!(
                    "ended: {} after {}s, peaking at {} {} ({}% of the {} threshold) under policy {}",
                    subject,
                    duration.as_secs(),
                    reason.observed,
                    reason.metric.unit().as_str(),
                    reason.ratio_percent,
                    reason.metric.as_str(),
                    policy.id
                ),
                None => format!(
                    "ended: {} after {}s under policy {}",
                    subject,
                    duration.as_secs(),
                    policy.id
                ),
            }
        }
    }
}

/// A per-process value that two concurrently running engines are
/// unlikely to share.
///
/// Not a random number and not claimed to be one: it mixes the process
/// id, the wall clock at startup, and the address of a fresh heap
/// allocation (which ASLR moves between runs). That is enough for
/// identifiers whose only requirement is not colliding between engines
/// in the same deployment. It is not suitable for anything that needs
/// unpredictability, and nothing here needs that.
fn seed_instance_id() -> u64 {
    let mut hasher = DefaultHasher::new();
    std::process::id().hash(&mut hasher);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        .hash(&mut hasher);
    let probe = Box::new(0u8);
    (Box::as_ref(&probe) as *const u8 as usize).hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::evaluate::evaluate;
    use crate::input::{MetricKind, ScopeKey};
    use crate::policy::{PolicyDraft, PolicySelector, TenantPrefixes, Thresholds};
    use crate::state::{StateTable, StateTableConfig};

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

    fn target() -> EventTarget {
        let key = key();
        EventTarget {
            tenant: key.tenant.clone(),
            scope_type: key.scope_type,
            scope_id: key.scope_id.clone(),
            display: key.scope_id.to_string(),
            direction: key.direction,
            address_family: key.address_family,
        }
    }

    fn draft() -> PolicyDraft {
        PolicyDraft {
            id: "p-host-bps".to_string(),
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

    fn policy() -> DetectionPolicy {
        draft().validate(&TenantPrefixes::new()).expect("valid")
    }

    fn snapshot(at: Instant, bps: u64) -> DetectionSnapshot {
        DetectionSnapshot {
            key: key(),
            window: Duration::from_secs(1),
            observed_at: at,
            observed_wall: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
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
            flows_observed: 42,
            exporters_observed: 2,
        }
    }

    /// Drives a scope to whatever states the rates produce, returning
    /// every event the run emitted.
    fn run(policy: &DetectionPolicy, rates: &[(u64, u64)]) -> Vec<DetectionEvent> {
        let factory = EventFactory::with_instance_id(0xfeed_face_dead_beef);
        let mut table = StateTable::new(StateTableConfig::default());
        let start = Instant::now();
        let mut events = Vec::new();
        for (offset, bps) in rates {
            let snap = snapshot(start + Duration::from_secs(*offset), *bps);
            let evaluation = evaluate(policy, &snap);
            let outcome = table.step(policy, &snap, &evaluation);
            let Some(record) = outcome.signal.as_ref() else {
                continue;
            };
            let event = factory
                .build(
                    record,
                    policy,
                    &snap,
                    &evaluation.matched,
                    &evaluation.skipped,
                )
                .expect("an alerting policy produces an event");
            events.push(event);
        }
        events
    }

    #[test]
    fn a_detection_produces_a_start_and_an_end_sharing_one_detection_id() {
        let policy = policy();
        let events = run(
            &policy,
            &[(0, 5_000_000), (2, 5_000_000), (3, 100), (5, 100)],
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::Started);
        assert_eq!(events[1].kind, EventKind::Ended);
        assert_eq!(events[0].detection_id, events[1].detection_id);
        assert_ne!(events[0].event_id, events[1].event_id);
        assert_eq!(events[0].sequence, 0);
        assert_eq!(events[1].sequence, 1);
    }

    #[test]
    fn two_detections_on_one_scope_get_different_detection_ids() {
        let mut draft = draft();
        draft.cooldown = Duration::ZERO;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let events = run(
            &policy,
            &[
                (0, 5_000_000),
                (2, 5_000_000),
                (3, 100),
                (5, 100),
                (6, 5_000_000),
                (8, 5_000_000),
            ],
        );
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].kind, EventKind::Started);
        assert_ne!(events[0].detection_id, events[2].detection_id);
        assert_eq!(
            events[2].sequence, 0,
            "a new detection restarts the sequence"
        );
    }

    #[test]
    fn a_detection_id_is_reproducible_from_the_same_inputs() {
        let factory = EventFactory::with_instance_id(7);
        let opened = Instant::now();
        assert_eq!(
            factory.detection_id(&target(), opened),
            factory.detection_id(&target(), opened)
        );
    }

    #[test]
    fn a_different_scope_gets_a_different_detection_id() {
        let factory = EventFactory::with_instance_id(7);
        let opened = Instant::now();
        let mut other = target();
        other.scope_id = ScopeId::Host {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)),
        };
        other.display = other.scope_id.to_string();
        assert_ne!(
            factory.detection_id(&target(), opened),
            factory.detection_id(&other, opened)
        );
    }

    #[test]
    fn a_different_instance_gets_a_different_detection_id() {
        let opened = Instant::now();
        assert_ne!(
            EventFactory::with_instance_id(7).detection_id(&target(), opened),
            EventFactory::with_instance_id(8).detection_id(&target(), opened)
        );
    }

    #[test]
    fn event_ids_do_not_repeat() {
        let factory = EventFactory::with_instance_id(1);
        let ids: std::collections::BTreeSet<String> =
            (0..1000).map(|_| factory.next_event_id()).collect();
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn a_dedup_key_is_identical_for_a_redelivered_event() {
        assert_eq!(
            dedup_key("abc", EventKind::Updated, 3),
            dedup_key("abc", EventKind::Updated, 3)
        );
        assert_ne!(
            dedup_key("abc", EventKind::Updated, 3),
            dedup_key("abc", EventKind::Updated, 4)
        );
        assert_ne!(
            dedup_key("abc", EventKind::Started, 0),
            dedup_key("abc", EventKind::Ended, 0)
        );
    }

    #[test]
    fn a_start_event_carries_everything_needed_to_explain_it() {
        let policy = policy();
        let events = run(&policy, &[(0, 5_000_000), (2, 7_000_000)]);
        let start = &events[0];
        assert_eq!(start.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(start.policy_id, "p-host-bps");
        assert_eq!(start.policy_version, 1);
        assert_eq!(start.severity, Severity::Major);
        assert_eq!(start.action, ActionTaken::Alerted);
        assert_eq!(start.target.tenant, "acme");
        assert_eq!(start.target.display, "203.0.113.7");
        assert_eq!(start.previous_state, DetectionState::PendingTrigger);
        assert_eq!(start.state, DetectionState::Active);
        assert_eq!(start.reason, TransitionReason::TriggerSustained);
        assert_eq!(start.matched.len(), 1);
        assert_eq!(start.matched[0].metric, MetricKind::Bps);
        assert_eq!(start.matched[0].observed, 7_000_000);
        assert_eq!(start.matched[0].threshold, 1_000_000);
        assert_eq!(start.rates.bps, 7_000_000);
        assert_eq!(start.flows_observed, 42);
        assert_eq!(start.exporters_observed, 2);
        assert_eq!(start.duration_ms, 0);
        assert_eq!(start.window_ms, 1000);
        assert!(start.is_publishable());
    }

    #[test]
    fn an_end_event_reports_the_peak_not_the_final_quiet_rate() {
        let policy = policy();
        let events = run(
            &policy,
            &[
                (0, 5_000_000),
                (2, 5_000_000),
                (3, 40_000_000),
                (4, 100),
                (6, 100),
            ],
        );
        let end = events.last().expect("an end event");
        assert_eq!(end.kind, EventKind::Ended);
        assert!(end.matched.is_empty(), "nothing is crossed at the end");
        assert_eq!(end.peak.len(), 1);
        assert_eq!(end.peak[0].observed, 40_000_000);
        assert_eq!(end.rates.bps, 100);
        assert_eq!(end.duration_ms, 4000);
        assert!(end.summary.contains("peaking at 40000000"));
    }

    #[test]
    fn an_observe_mode_event_exists_but_is_not_publishable() {
        let mut draft = draft();
        draft.execution_mode = ExecutionMode::Observe;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let events = run(&policy, &[(0, 5_000_000), (2, 5_000_000)]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, ActionTaken::Observed);
        assert!(!events[0].is_publishable());
    }

    #[test]
    fn a_dry_run_event_is_publishable_and_says_it_did_nothing() {
        let mut draft = draft();
        draft.execution_mode = ExecutionMode::DryRun;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let events = run(&policy, &[(0, 5_000_000), (2, 5_000_000)]);
        assert_eq!(events[0].action, ActionTaken::DryRun);
        assert!(events[0].is_publishable());
    }

    #[test]
    fn a_disabled_mode_yields_no_action_and_therefore_no_event() {
        assert_eq!(ActionTaken::for_mode(ExecutionMode::Disabled), None);
    }

    #[test]
    fn no_action_this_phase_can_produce_reports_itself_as_executed() {
        for action in [
            ActionTaken::Observed,
            ActionTaken::Alerted,
            ActionTaken::DryRun,
        ] {
            assert!(
                !action.executed(),
                "{} must not report itself as executed: this crate cannot act",
                action.as_str()
            );
        }
    }

    #[test]
    fn every_execution_mode_produces_an_unexecuted_event() {
        for mode in [
            ExecutionMode::Observe,
            ExecutionMode::AlertOnly,
            ExecutionMode::DryRun,
        ] {
            let mut draft = draft();
            draft.execution_mode = mode;
            let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
            let events = run(
                &policy,
                &[(0, 5_000_000), (2, 5_000_000), (3, 100), (5, 100)],
            );
            assert!(!events.is_empty(), "{} produced no events", mode.as_str());
            for event in &events {
                assert!(
                    !event.executed,
                    "{} produced an event claiming it acted on traffic",
                    mode.as_str()
                );
            }
        }
    }

    #[test]
    fn a_dry_run_event_serializes_executed_as_false() {
        let mut draft = draft();
        draft.execution_mode = ExecutionMode::DryRun;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let events = run(&policy, &[(0, 5_000_000), (2, 5_000_000)]);
        let value: serde_json::Value = serde_json::to_value(&events[0]).expect("serializes");
        assert_eq!(value["action"], "dryRun");
        assert_eq!(
            value["executed"],
            serde_json::Value::Bool(false),
            "a dry-run event must state on the wire that nothing was done"
        );
    }

    #[test]
    fn a_closing_event_is_also_unexecuted() {
        // The stale/withdrawn close path builds events without a
        // snapshot and must carry the same guarantee.
        let mut draft = draft();
        draft.execution_mode = ExecutionMode::DryRun;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let factory = EventFactory::with_instance_id(1);
        let mut table = StateTable::new(StateTableConfig::default());
        let start = Instant::now();
        for (offset, bps) in [(0u64, 5_000_000u64), (2, 5_000_000)] {
            let snap = snapshot(start + Duration::from_secs(offset), bps);
            let evaluation = evaluate(&policy, &snap);
            table.step(&policy, &snap, &evaluation);
        }
        let expiries = table.sweep(start + Duration::from_secs(400), UNIX_EPOCH);
        let expiry = expiries.first().expect("a stale close");
        let record = crate::state::SignalRecord {
            signal: crate::state::Signal::Ended,
            transition: expiry.transition.clone().expect("a transition"),
            detection: expiry.detection.clone().expect("a context"),
        };
        let event = factory
            .build_closing(&record, &policy, UNIX_EPOCH)
            .expect("an event");
        assert_eq!(event.kind, EventKind::Ended);
        assert!(!event.executed);
    }

    #[test]
    fn an_event_round_trips_through_json() {
        let policy = policy();
        let events = run(&policy, &[(0, 5_000_000), (2, 5_000_000)]);
        let json = serde_json::to_string(&events[0]).expect("serializes");
        let back: DetectionEvent = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, events[0]);
    }

    #[test]
    fn the_json_uses_the_documented_field_and_enum_names() {
        let policy = policy();
        let events = run(&policy, &[(0, 5_000_000), (2, 5_000_000)]);
        let value: serde_json::Value = serde_json::to_value(&events[0]).expect("serializes");
        for field in [
            "schemaVersion",
            "eventId",
            "detectionId",
            "dedupKey",
            "policyId",
            "policyVersion",
            "previousState",
            "detectedAtMs",
            "observedAtMs",
            "durationMs",
            "windowMs",
            "snapshotsInDetection",
        ] {
            assert!(value.get(field).is_some(), "missing field {field}");
        }
        assert_eq!(value["kind"], "started");
        assert_eq!(value["state"], "active");
        assert_eq!(value["previousState"], "pendingTrigger");
        assert_eq!(value["reason"], "triggerSustained");
        assert_eq!(value["severity"], "major");
        assert_eq!(value["executionMode"], "alertOnly");
        assert_eq!(value["action"], "alerted");
    }

    #[test]
    fn enum_json_matches_the_as_str_used_in_logs() {
        // Two spellings of the same value is how a dashboard filter
        // silently stops matching. Assert they agree.
        for state in [
            DetectionState::Idle,
            DetectionState::PendingTrigger,
            DetectionState::Active,
            DetectionState::PendingClear,
            DetectionState::Cooldown,
        ] {
            assert_eq!(
                serde_json::to_value(state).expect("serializes"),
                serde_json::Value::String(state.as_str().to_string())
            );
        }
        for reason in [
            TransitionReason::ThresholdCrossed,
            TransitionReason::TriggerSustained,
            TransitionReason::TriggerAborted,
            TransitionReason::FellBelowClear,
            TransitionReason::ClearSustained,
            TransitionReason::ClearAborted,
            TransitionReason::CooldownExpired,
            TransitionReason::PolicyChanged,
            TransitionReason::PolicyWithdrawn,
            TransitionReason::Stale,
            TransitionReason::ManualReset,
        ] {
            assert_eq!(
                serde_json::to_value(reason).expect("serializes"),
                serde_json::Value::String(reason.as_str().to_string())
            );
        }
        for kind in [EventKind::Started, EventKind::Updated, EventKind::Ended] {
            assert_eq!(
                serde_json::to_value(kind).expect("serializes"),
                serde_json::Value::String(kind.as_str().to_string())
            );
        }
        for action in [
            ActionTaken::Observed,
            ActionTaken::Alerted,
            ActionTaken::DryRun,
        ] {
            assert_eq!(
                serde_json::to_value(action).expect("serializes"),
                serde_json::Value::String(action.as_str().to_string())
            );
        }
    }

    #[test]
    fn a_summary_names_the_target_the_metric_and_the_policy() {
        let policy = policy();
        let events = run(&policy, &[(0, 5_000_000), (2, 5_000_000)]);
        let summary = &events[0].summary;
        assert!(summary.contains("203.0.113.7"), "{summary}");
        assert!(summary.contains("incoming"), "{summary}");
        assert!(summary.contains("bps"), "{summary}");
        assert!(summary.contains("p-host-bps"), "{summary}");
        assert!(summary.contains("500%"), "{summary}");
        assert!(!summary.contains('\n'), "a summary must stay one line");
    }

    #[test]
    fn two_factories_in_one_process_do_not_share_an_instance_id() {
        assert_ne!(
            EventFactory::new().instance_id(),
            EventFactory::new().instance_id()
        );
    }

    #[test]
    fn a_pre_epoch_wall_clock_does_not_panic() {
        assert_eq!(unix_millis(UNIX_EPOCH - Duration::from_secs(10)), 0);
        assert_eq!(unix_millis(UNIX_EPOCH), 0);
        assert_eq!(unix_millis(UNIX_EPOCH + Duration::from_secs(2)), 2000);
    }
}
