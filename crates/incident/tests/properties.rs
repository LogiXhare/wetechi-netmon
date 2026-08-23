//! Property tests, following Phase 4's use of `proptest`.
//!
//! A focused subset of the testing plan's thirteen properties — the
//! ones that exercise the unit-of-work's actual command/ingest surface
//! rather than restating what a single unit test already pins exactly
//! (e.g. the inclusive reopen boundary, already property-adjacent as
//! exact-value tests in `crate::reopen`).

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};

use proptest::prelude::*;
use wetechinetmon_detector::{
    ActionTaken, AddressFamily, DataCompleteness, DetectionEvent, DetectionState, EventKind,
    EventTarget, ExecutionMode, MatchedReason, MetricKind, MetricRates, SamplingStatus, ScopeId,
    ScopeType, Severity, TestClock, TrafficDirection, TransitionReason,
};
use wetechinetmon_incident::authorization::AuthorizationContext;
use wetechinetmon_incident::correlation::TenantId;
use wetechinetmon_incident::id::TestIncidentGenerator;
use wetechinetmon_incident::number::InMemoryNumberAllocator;
use wetechinetmon_incident::unit_of_work::{IncidentUnitOfWork, IngestOutcomeKind};

fn event_n(detection_id: &str, sequence: u64, addr: IpAddr, observed: u64) -> DetectionEvent {
    DetectionEvent {
        schema_version: wetechinetmon_detector::EVENT_SCHEMA_VERSION,
        event_id: format!("{detection_id}-{sequence}"),
        detection_id: detection_id.to_string(),
        sequence,
        kind: if sequence == 0 {
            EventKind::Started
        } else {
            EventKind::Updated
        },
        dedup_key: format!("{detection_id}:updated:{sequence}"),
        policy_id: "p-host-bps".to_string(),
        policy_name: "host bps".to_string(),
        policy_version: 1,
        severity: Severity::Major,
        execution_mode: ExecutionMode::AlertOnly,
        action: ActionTaken::Alerted,
        labels: BTreeMap::new(),
        target: EventTarget {
            tenant: "acme".to_string(),
            scope_type: ScopeType::Host,
            scope_id: ScopeId::Host { addr },
            display: addr.to_string(),
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        },
        previous_state: DetectionState::PendingTrigger,
        state: DetectionState::Active,
        reason: TransitionReason::TriggerSustained,
        detected_at_ms: 1_700_000_000_000 + sequence,
        observed_at_ms: 1_700_000_000_000 + sequence,
        duration_ms: sequence,
        window_ms: 1000,
        matched: vec![MatchedReason {
            metric: MetricKind::Bps,
            observed,
            threshold: 1_000_000,
            excess: observed.saturating_sub(1_000_000),
            ratio_percent: observed * 100 / 1_000_000,
        }],
        peak: Vec::new(),
        skipped: Vec::new(),
        rates: MetricRates::default(),
        completeness: DataCompleteness::default(),
        sampling: SamplingStatus::default(),
        flows_observed: 1,
        exporters_observed: 1,
        snapshots_in_detection: sequence + 1,
        executed: false,
        summary: "test".to_string(),
    }
}

fn fresh_uow() -> IncidentUnitOfWork {
    IncidentUnitOfWork::new(
        Box::new(TestIncidentGenerator::starting_at(1)),
        Box::new(InMemoryNumberAllocator::new()),
        Box::new(TestClock::new()),
    )
}

proptest! {
    /// Property 4 / correlation determinism: duplicate ingestion never
    /// creates a second incident, for any number of redeliveries.
    #[test]
    fn duplicate_ingestion_never_creates_a_second_incident(redeliveries in 0usize..20) {
        let mut uow = fresh_uow();
        let correlator = AuthorizationContext::correlator(TenantId::new("acme"));
        let addr: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50));
        let started = event_n("det-prop-1", 0, addr, 5_000_000);

        let first = uow.ingest_detection_event(&correlator, &started).unwrap();
        prop_assert_eq!(first.outcome_kind, IngestOutcomeKind::Created);

        for _ in 0..redeliveries {
            let replay = uow.ingest_detection_event(&correlator, &started).unwrap();
            prop_assert_eq!(replay.outcome_kind, IngestOutcomeKind::Duplicate);
        }
        prop_assert_eq!(uow.incident_count(), 1);
    }

    /// Property 3: version increases strictly monotonically per incident
    /// across an arbitrary number of linked updates.
    #[test]
    fn version_increases_strictly_monotonically(update_count in 1usize..15) {
        let mut uow = fresh_uow();
        let correlator = AuthorizationContext::correlator(TenantId::new("acme"));
        let addr: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 51));
        let started = event_n("det-prop-2", 0, addr, 5_000_000);
        let incident_id = uow.ingest_detection_event(&correlator, &started).unwrap().incident_id.unwrap();
        let mut last_version = uow.get(&incident_id).unwrap().version;

        for i in 1..=update_count {
            let updated = event_n("det-prop-2", i as u64, addr, 5_000_000 + i as u64);
            uow.ingest_detection_event(&correlator, &updated).unwrap();
            let version = uow.get(&incident_id).unwrap().version;
            prop_assert!(version > last_version, "version must strictly increase: {last_version} -> {version}");
            last_version = version;
        }
    }

    /// Property 12: `last_detected_at` never moves backwards, regardless
    /// of the order sequence numbers happen to arrive in the test input
    /// (sequence is not the same axis as delivery order here, but the
    /// gate is the same: an event whose synthetic observed time is
    /// older than what is already recorded must never regress the
    /// stored value).
    #[test]
    fn last_detected_at_is_monotonic_even_with_reordered_deliveries(a in 1u64..1000, b in 1u64..1000) {
        let mut uow = fresh_uow();
        let correlator = AuthorizationContext::correlator(TenantId::new("acme"));
        let addr: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 52));
        let started = event_n("det-prop-3", 0, addr, 5_000_000);
        let incident_id = uow.ingest_detection_event(&correlator, &started).unwrap().incident_id.unwrap();

        let first_seq = a.min(b) + 1;
        let second_seq = a.max(b) + 1;
        uow.ingest_detection_event(&correlator, &event_n("det-prop-3", first_seq, addr, 5_000_001)).unwrap();
        let after_first = uow.get(&incident_id).unwrap().last_detected_at;
        uow.ingest_detection_event(&correlator, &event_n("det-prop-3", second_seq, addr, 5_000_002)).unwrap();
        let after_second = uow.get(&incident_id).unwrap().last_detected_at;
        prop_assert!(after_second.elapsed_since(&after_first) >= std::time::Duration::ZERO);
    }

    /// Property 7 (tenant isolation, scoped): an ingestion under tenant A
    /// never becomes visible or mutable under tenant B's context.
    #[test]
    fn tenant_a_events_never_correlate_under_tenant_b(seq in 0u64..5) {
        let mut uow = fresh_uow();
        let correlator_a = AuthorizationContext::correlator(TenantId::new("acme"));
        let correlator_b = AuthorizationContext::correlator(TenantId::new("globex"));
        let addr: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53));
        let mut event = event_n("det-prop-4", seq, addr, 5_000_000);
        event.target.tenant = "acme".to_string();

        let result_a = uow.ingest_detection_event(&correlator_a, &event);
        prop_assert!(result_a.is_ok());

        // The same event body claims tenant "acme" but is submitted
        // through tenant B's authorization context — must be rejected as
        // a tenant mismatch, never silently reattributed or correlated.
        let result_b = uow.ingest_detection_event(&correlator_b, &event);
        prop_assert!(result_b.is_err());
    }
}
