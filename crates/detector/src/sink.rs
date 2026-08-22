//! Where detection events go.
//!
//! The engine does not know what happens to an event after it hands it
//! over, and that is the point. A sink is the only outward-facing seam
//! in this crate, and it is deliberately narrow: one method, taking one
//! event, returning whether it was accepted.
//!
//! # Why publishing is synchronous
//!
//! [`DetectionEventSink`] is used behind `dyn`, and an `async fn` in a
//! trait is not dyn-compatible without boxing every future. A sink that
//! needs to do I/O should accept the event into a bounded queue here and
//! drain it from its own task — which is what a real transport wants
//! anyway, since a detection engine must never block on a slow consumer
//! while traffic keeps arriving.
//!
//! # What a sink must never do
//!
//! Nothing in this module may reach a router. A sink receives an event
//! and returns; it has no path to request mitigation, and no
//! implementation here or later in this crate may add one. See ADR 0007.

use std::sync::Mutex;

use crate::event::DetectionEvent;

/// Why an event was not accepted.
///
/// Every variant is a fact the caller should count, not swallow: an
/// event that vanished silently is an alert nobody got.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    #[error("sink {sink} is full; the event was dropped")]
    Full { sink: &'static str },
    #[error("sink {sink} is closed")]
    Closed { sink: &'static str },
    #[error("sink {sink} rejected the event: {detail}")]
    Backend { sink: &'static str, detail: String },
}

/// Somewhere a detection event can be published.
pub trait DetectionEventSink: Send + Sync {
    /// Accepts one event, or explains why it could not.
    ///
    /// Must not block on network I/O. An implementation that needs I/O
    /// should enqueue and return.
    fn publish(&self, event: &DetectionEvent) -> Result<(), SinkError>;

    /// A short, fixed name used in errors and metrics. Must be a
    /// constant, not derived from configuration, so it cannot become an
    /// unbounded metric label.
    fn name(&self) -> &'static str;
}

/// Discards everything, successfully.
///
/// For a deployment running entirely in observe mode, and for tests that
/// care about the engine rather than the events.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullSink;

impl DetectionEventSink for NullSink {
    fn publish(&self, _event: &DetectionEvent) -> Result<(), SinkError> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "null"
    }
}

/// Keeps events in memory, bounded, oldest dropped first.
///
/// Dropping the *oldest* is the right choice for a detector: when a
/// buffer overflows during an attack, the newest events describe what is
/// happening now, and the oldest describe what an operator has most
/// likely already seen.
#[derive(Debug)]
pub struct InMemorySink {
    capacity: usize,
    inner: Mutex<InMemoryState>,
}

#[derive(Debug, Default)]
struct InMemoryState {
    events: Vec<DetectionEvent>,
    dropped: u64,
}

impl InMemorySink {
    pub fn new(capacity: usize) -> Self {
        InMemorySink {
            capacity,
            inner: Mutex::new(InMemoryState::default()),
        }
    }

    /// Every event still held, oldest first.
    pub fn events(&self) -> Vec<DetectionEvent> {
        self.locked().events.clone()
    }

    pub fn len(&self) -> usize {
        self.locked().events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locked().events.is_empty()
    }

    /// How many events were discarded to stay within capacity.
    pub fn dropped(&self) -> u64 {
        self.locked().dropped
    }

    pub fn clear(&self) {
        let mut state = self.locked();
        state.events.clear();
        state.dropped = 0;
    }

    /// A poisoned lock means another thread panicked *while holding it*,
    /// which for a plain `Vec` push cannot leave the data inconsistent.
    /// Recovering is strictly better than propagating a panic through
    /// the detection path and taking the engine down with it.
    fn locked(&self) -> std::sync::MutexGuard<'_, InMemoryState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl DetectionEventSink for InMemorySink {
    fn publish(&self, event: &DetectionEvent) -> Result<(), SinkError> {
        if self.capacity == 0 {
            return Err(SinkError::Full { sink: "memory" });
        }
        let mut state = self.locked();
        while state.events.len() >= self.capacity {
            state.events.remove(0);
            state.dropped = state.dropped.saturating_add(1);
        }
        state.events.push(event.clone());
        Ok(())
    }

    fn name(&self) -> &'static str {
        "memory"
    }
}

/// Writes each event to the tracing subscriber, at a level matching its
/// severity.
///
/// Fields are chosen so a log line is greppable by detection id and by
/// target without needing the whole JSON body. The full event is not
/// logged: it is large, and an operator reading logs wants the summary.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingSink;

impl DetectionEventSink for TracingSink {
    fn publish(&self, event: &DetectionEvent) -> Result<(), SinkError> {
        use crate::policy::Severity;
        macro_rules! emit {
            ($level:ident) => {
                tracing::$level!(
                    detection_id = %event.detection_id,
                    event_id = %event.event_id,
                    kind = event.kind.as_str(),
                    policy = %event.policy_id,
                    policy_version = event.policy_version,
                    tenant = %event.target.tenant,
                    target = %event.target.display,
                    direction = event.target.direction.as_str(),
                    action = event.action.as_str(),
                    "{}",
                    event.summary
                )
            };
        }
        match event.severity {
            Severity::Info => emit!(info),
            Severity::Minor => emit!(info),
            Severity::Major => emit!(warn),
            Severity::Critical => emit!(error),
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "tracing"
    }
}

/// Publishes to several sinks, reporting the first failure but always
/// attempting every one.
///
/// One sink being full must not stop another from receiving the event —
/// a detection reaching the log but not the database is far better than
/// it reaching neither.
pub struct FanOutSink {
    sinks: Vec<Box<dyn DetectionEventSink>>,
}

impl FanOutSink {
    pub fn new(sinks: Vec<Box<dyn DetectionEventSink>>) -> Self {
        FanOutSink { sinks }
    }

    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }
}

impl DetectionEventSink for FanOutSink {
    fn publish(&self, event: &DetectionEvent) -> Result<(), SinkError> {
        let mut first_error = None;
        for sink in &self.sinks {
            if let Err(error) = sink.publish(event) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn name(&self) -> &'static str {
        "fanout"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::event::{ActionTaken, EventKind, EventTarget};
    use crate::input::{
        AddressFamily, DataCompleteness, MetricRates, SamplingStatus, ScopeId, ScopeType,
        TrafficDirection,
    };
    use crate::policy::{ExecutionMode, Severity};
    use crate::state::{DetectionState, TransitionReason};

    fn event(id: &str) -> DetectionEvent {
        DetectionEvent {
            schema_version: 1,
            event_id: id.to_string(),
            detection_id: "d1".to_string(),
            sequence: 0,
            kind: EventKind::Started,
            dedup_key: format!("d1:started:{id}"),
            policy_id: "p1".to_string(),
            policy_name: "p".to_string(),
            policy_version: 1,
            severity: Severity::Major,
            execution_mode: ExecutionMode::AlertOnly,
            action: ActionTaken::Alerted,
            labels: BTreeMap::new(),
            target: EventTarget {
                tenant: "acme".to_string(),
                scope_type: ScopeType::Host,
                scope_id: ScopeId::Host {
                    addr: "203.0.113.7".parse().expect("valid"),
                },
                display: "203.0.113.7".to_string(),
                direction: TrafficDirection::Incoming,
                address_family: AddressFamily::Ipv4,
            },
            previous_state: DetectionState::PendingTrigger,
            state: DetectionState::Active,
            reason: TransitionReason::TriggerSustained,
            detected_at_ms: 0,
            observed_at_ms: 0,
            duration_ms: 0,
            window_ms: 1000,
            matched: Vec::new(),
            peak: Vec::new(),
            skipped: Vec::new(),
            rates: MetricRates::default(),
            completeness: DataCompleteness::default(),
            sampling: SamplingStatus::default(),
            flows_observed: 0,
            exporters_observed: 0,
            snapshots_in_detection: 0,
            executed: false,
            summary: "test".to_string(),
        }
    }

    /// Fails every publish, to prove fan-out keeps going.
    struct AlwaysFails;

    impl DetectionEventSink for AlwaysFails {
        fn publish(&self, _event: &DetectionEvent) -> Result<(), SinkError> {
            Err(SinkError::Closed { sink: "broken" })
        }

        fn name(&self) -> &'static str {
            "broken"
        }
    }

    #[test]
    fn the_null_sink_accepts_everything() {
        assert!(NullSink.publish(&event("e1")).is_ok());
        assert_eq!(NullSink.name(), "null");
    }

    #[test]
    fn the_memory_sink_keeps_what_it_is_given() {
        let sink = InMemorySink::new(4);
        assert!(sink.is_empty());
        sink.publish(&event("e1")).expect("accepted");
        sink.publish(&event("e2")).expect("accepted");
        assert_eq!(sink.len(), 2);
        let held = sink.events();
        assert_eq!(held[0].event_id, "e1");
        assert_eq!(held[1].event_id, "e2");
        assert_eq!(sink.dropped(), 0);
    }

    #[test]
    fn the_memory_sink_drops_the_oldest_when_full() {
        let sink = InMemorySink::new(2);
        for id in ["e1", "e2", "e3"] {
            sink.publish(&event(id)).expect("accepted");
        }
        let held = sink.events();
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].event_id, "e2");
        assert_eq!(held[1].event_id, "e3");
        assert_eq!(sink.dropped(), 1);
    }

    #[test]
    fn a_zero_capacity_memory_sink_reports_full_rather_than_silently_dropping() {
        let sink = InMemorySink::new(0);
        assert_eq!(
            sink.publish(&event("e1")),
            Err(SinkError::Full { sink: "memory" })
        );
        assert!(sink.is_empty());
    }

    #[test]
    fn clearing_resets_both_the_events_and_the_drop_count() {
        let sink = InMemorySink::new(1);
        sink.publish(&event("e1")).expect("accepted");
        sink.publish(&event("e2")).expect("accepted");
        assert_eq!(sink.dropped(), 1);
        sink.clear();
        assert!(sink.is_empty());
        assert_eq!(sink.dropped(), 0);
    }

    #[test]
    fn the_tracing_sink_accepts_every_severity() {
        for severity in [
            Severity::Info,
            Severity::Minor,
            Severity::Major,
            Severity::Critical,
        ] {
            let mut sample = event("e1");
            sample.severity = severity;
            assert!(TracingSink.publish(&sample).is_ok());
        }
    }

    #[test]
    fn fan_out_reaches_every_sink_even_when_one_fails() {
        let good = InMemorySink::new(4);
        let events = std::sync::Arc::new(good);
        let fan = FanOutSink::new(vec![
            Box::new(AlwaysFails),
            Box::new(SharedMemorySink(events.clone())),
        ]);
        let result = fan.publish(&event("e1"));
        assert_eq!(result, Err(SinkError::Closed { sink: "broken" }));
        assert_eq!(events.len(), 1, "the healthy sink still received it");
    }

    #[test]
    fn fan_out_over_healthy_sinks_succeeds() {
        let fan = FanOutSink::new(vec![Box::new(NullSink), Box::new(TracingSink)]);
        assert_eq!(fan.len(), 2);
        assert!(fan.publish(&event("e1")).is_ok());
    }

    #[test]
    fn an_empty_fan_out_succeeds_without_doing_anything() {
        let fan = FanOutSink::new(Vec::new());
        assert!(fan.is_empty());
        assert!(fan.publish(&event("e1")).is_ok());
    }

    /// Lets a test hold a handle to a sink that fan-out owns.
    struct SharedMemorySink(std::sync::Arc<InMemorySink>);

    impl DetectionEventSink for SharedMemorySink {
        fn publish(&self, event: &DetectionEvent) -> Result<(), SinkError> {
            self.0.publish(event)
        }

        fn name(&self) -> &'static str {
            "memory"
        }
    }
}
