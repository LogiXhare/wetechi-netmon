//! The bounded, in-memory `IncidentUnitOfWork`.
//!
//! One store, not many concrete repositories — the approved Phase 5A
//! shape. Every public mutation method here applies its full effect (the
//! incident, its timeline entry, its audit entry, its outbox row, its
//! version bump) or none of it: each method computes everything it needs
//! before touching `self`'s maps, so a `?` partway through never leaves a
//! partial write behind. [`IncidentUnitOfWork::inject_failure_after_incident_write`]
//! exists only for tests, to prove that claim by forcing a failure
//! between the incident write and the rest of the commit and checking
//! nothing partial survives.
//!
//! Denied commands (owner decision 9) are handled by
//! [`IncidentUnitOfWork::check_permission`], called first in every
//! command-handling method, before the incident is even read mutably: a
//! denial returns before any of the incident, timeline, or outbox state
//! is touched, and writes exactly one [`crate::audit::AuditEntry::denied`].
//!
//! Correlation conflicts (owner decision 10) are never retried inside
//! this crate — [`crate::correlation::CorrelationConflict`] is returned
//! to the caller immediately. In 5A's single-threaded model a genuine
//! conflict can only arise from a caller re-entering with stale
//! information, which is exactly the case a bounded, non-retrying return
//! is correct for.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use wetechinetmon_detector::{DetectionEvent, EventKind, MetricKind};

use crate::assignment::Assignee;
use crate::audit::{AttemptedResource, AuditEntry};
use crate::authorization::{Actor, AuthorizationContext, Permission};
use crate::category::derive_category;
use crate::clock::{Clock, Timestamp};
use crate::closure::ClosurePolicy;
use crate::command::Command;
use crate::correlation::{CorrelationConflict, CorrelationKey, TenantId};
use crate::error::IncidentError;
use crate::evidence::{EvidenceLinkType, EvidenceReference};
use crate::id::{IncidentGenerator, IncidentId};
use crate::idempotency::{
    IdempotencyCheck, IdempotencyKey, IdempotencyStore, RequestFingerprint, StoredOutcome,
};
use crate::incident::{
    validate_note_body, Incident, Note, NoteVisibility, INCIDENT_SCHEMA_VERSION,
};
use crate::limits::{AFFECTED_TARGETS_MAX, TAG_KEY_MAX_LEN, TAG_VALUE_MAX_LEN};
use crate::number::NumberAllocator;
use crate::outbox::{OutboxEvent, OutboxMessage};
use crate::reopen::ReopenPolicy;
use crate::severity::{Priority, SeveritySource};
use crate::state::IncidentState;
use crate::suppression::Suppression;
use crate::timeline::{AutomaticCause, TimelineEntry, TimelinePayload, TransitionCause};
use crate::transition;

/// Every stored piece of one incident's history, kept together because
/// 5A has exactly one backing store and splitting into per-aggregate
/// repository traits now risks guessing the seam wrong before 5B's real
/// PostgreSQL implementation exists to compare against.
pub struct IncidentUnitOfWork {
    incidents: HashMap<IncidentId, Incident>,
    open_index: HashMap<CorrelationKey, IncidentId>,
    dedup_seen: HashMap<(TenantId, String), IncidentId>,
    timeline: Vec<TimelineEntry>,
    audit: Vec<AuditEntry>,
    outbox: Vec<OutboxMessage>,
    idempotency: IdempotencyStore,
    timeline_sequence: u64,
    audit_sequence: u64,
    outbox_sequence: u64,
    incident_generator: Box<dyn IncidentGenerator>,
    number_allocator: Box<dyn NumberAllocator>,
    clock: Box<dyn Clock>,
    closure_policy: ClosurePolicy,
    reopen_policy: ReopenPolicy,
    /// Test-support seam: when set, every subsequent mutation's
    /// `maybe_fail` checkpoint returns an error instead of committing
    /// its timeline/audit/outbox writes. Always compiled (not
    /// `cfg(test)`-gated) so integration tests in `tests/` — a separate
    /// compilation unit that does not see the library's own `cfg(test)`
    /// items — can exercise it too. See the testing plan's "injected
    /// failure at each commit point" requirement.
    fail_after_incident_write: bool,
}

/// Result of successfully ingesting a detection event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestResult {
    pub outcome_kind: IngestOutcomeKind,
    pub incident_id: Option<IncidentId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcomeKind {
    Created,
    Updated,
    Reopened,
    LinkedLate,
    Duplicate,
    Quarantined,
    ObserveOnly,
}

impl IncidentUnitOfWork {
    pub fn new(
        incident_generator: Box<dyn IncidentGenerator>,
        number_allocator: Box<dyn NumberAllocator>,
        clock: Box<dyn Clock>,
    ) -> Self {
        IncidentUnitOfWork {
            incidents: HashMap::new(),
            open_index: HashMap::new(),
            dedup_seen: HashMap::new(),
            timeline: Vec::new(),
            audit: Vec::new(),
            outbox: Vec::new(),
            idempotency: IdempotencyStore::new(),
            timeline_sequence: 0,
            audit_sequence: 0,
            outbox_sequence: 0,
            incident_generator,
            number_allocator,
            clock,
            closure_policy: ClosurePolicy::approved_default(),
            reopen_policy: ReopenPolicy::approved_default(),
            fail_after_incident_write: false,
        }
    }

    pub fn with_policies(
        mut self,
        closure_policy: ClosurePolicy,
        reopen_policy: ReopenPolicy,
    ) -> Self {
        self.closure_policy = closure_policy;
        self.reopen_policy = reopen_policy;
        self
    }

    pub fn get(&self, id: &IncidentId) -> Option<&Incident> {
        self.incidents.get(id)
    }

    pub fn timeline(&self) -> &[TimelineEntry] {
        &self.timeline
    }

    pub fn audit(&self) -> &[AuditEntry] {
        &self.audit
    }

    pub fn outbox(&self) -> &[OutboxMessage] {
        &self.outbox
    }

    pub fn incident_count(&self) -> usize {
        self.incidents.len()
    }

    fn now(&self) -> Timestamp {
        Timestamp::now(self.clock.as_ref())
    }

    fn next_timeline_sequence(&mut self) -> u64 {
        let s = self.timeline_sequence;
        self.timeline_sequence += 1;
        s
    }

    fn next_audit_sequence(&mut self) -> u64 {
        let s = self.audit_sequence;
        self.audit_sequence += 1;
        s
    }

    fn next_outbox_sequence(&mut self) -> u64 {
        let s = self.outbox_sequence;
        self.outbox_sequence += 1;
        s
    }

    /// Test-support seam (see the field doc comment on
    /// `fail_after_incident_write`) — not used by any production call
    /// site in this crate.
    pub fn inject_failure_after_incident_write(&mut self, enabled: bool) {
        self.fail_after_incident_write = enabled;
    }

    fn maybe_fail(&self) -> Result<(), IncidentError> {
        if self.fail_after_incident_write {
            Err(IncidentError::InternalInvariantViolation(
                "injected test failure",
            ))
        } else {
            Ok(())
        }
    }

    /// Checks a permission and, if denied, writes exactly one denied
    /// audit record and returns `Err(Unauthorized)` — before anything
    /// else is touched. Never mutates an incident, never bumps a
    /// version, never emits an outbox event.
    fn check_permission(
        &mut self,
        auth: &AuthorizationContext,
        permission: Permission,
        resource: AttemptedResource,
    ) -> Result<(), IncidentError> {
        if auth.has(permission) {
            return Ok(());
        }
        let sequence = self.next_audit_sequence();
        self.audit.push(AuditEntry::denied(
            sequence,
            auth.tenant().clone(),
            auth.actor().clone(),
            permission,
            resource,
            "missing required permission",
        ));
        // The denial stands regardless of anything about the audit write
        // itself — in this in-memory store the push above cannot fail,
        // and the ordering here (decide denied, then audit, then return
        // denied) is deliberate: the returned result does not depend on
        // the audit push having "succeeded" in any sense beyond being a
        // Vec::push.
        Err(IncidentError::Unauthorized)
    }

    fn check_tenant(
        &self,
        auth: &AuthorizationContext,
        incident: &Incident,
    ) -> Result<(), IncidentError> {
        if &incident.tenant_id != auth.tenant() {
            return Err(IncidentError::NotFound);
        }
        Ok(())
    }

    // ---------------------------------------------------------------
    // Ingestion / correlation
    // ---------------------------------------------------------------

    /// Runs the six-outcome deterministic correlation decision procedure
    /// against one detection event and applies its effect atomically.
    pub fn ingest_detection_event(
        &mut self,
        auth: &AuthorizationContext,
        event: &DetectionEvent,
    ) -> Result<IngestResult, IncidentError> {
        let resource = AttemptedResource::Unresolved(event.detection_id.clone());
        self.check_permission(auth, Permission::IncidentIngest, resource)?;

        let tenant = TenantId::new(event.target.tenant.clone());
        if &tenant != auth.tenant() {
            return Err(IncidentError::TenantMismatch);
        }

        // 1. Schema gate.
        if event.schema_version > wetechinetmon_detector::EVENT_SCHEMA_VERSION {
            return Ok(IngestResult {
                outcome_kind: IngestOutcomeKind::Quarantined,
                incident_id: None,
            });
        }

        // 2. Duplicate gate — runs before anything mutable.
        let dedup = (tenant.clone(), event.dedup_key.clone());
        if let Some(existing) = self.dedup_seen.get(&dedup) {
            return Ok(IngestResult {
                outcome_kind: IngestOutcomeKind::Duplicate,
                incident_id: Some(*existing),
            });
        }

        // 3. Mode gate — Observe events never open or change an incident.
        if !event.is_publishable() && event.action == wetechinetmon_detector::ActionTaken::Observed
        {
            return Ok(IngestResult {
                outcome_kind: IngestOutcomeKind::ObserveOnly,
                incident_id: None,
            });
        }

        let key = CorrelationKey::new(
            tenant.clone(),
            event.target.scope_type,
            event.target.scope_id.clone(),
            event.target.direction,
            event.target.address_family,
        );

        // 4. Lookup.
        if let Some(&incident_id) = self.open_index.get(&key) {
            self.dedup_seen.insert(dedup, incident_id);
            let result = self.link_event_to_open_incident(auth, incident_id, event)?;
            // Let the state machine decide whether this event causes a
            // transition (correlation design, step 5): an `Ended` event
            // on an open, non-recovering incident automatically enters
            // `Recovering`. A late `Ended` (superseded by a newer event
            // already recorded) or one on an incident already past
            // `Open`/`Acknowledged`/`Investigating`/`Monitoring` is
            // linked as evidence only — `enter_recovering`'s own guard
            // is what makes that distinction, so a failure here is
            // expected and silently ignored rather than propagated.
            if event.kind == EventKind::Ended && result.outcome_kind == IngestOutcomeKind::Updated {
                let reason = match event.reason {
                    wetechinetmon_detector::TransitionReason::Stale => {
                        transition::DetectionEndReason::DetectorStale
                    }
                    wetechinetmon_detector::TransitionReason::PolicyWithdrawn => {
                        transition::DetectionEndReason::PolicyWithdrawn
                    }
                    wetechinetmon_detector::TransitionReason::ManualReset => {
                        transition::DetectionEndReason::DetectorReset
                    }
                    _ => transition::DetectionEndReason::TrafficCleared,
                };
                let _ = self.enter_recovering(auth, incident_id, reason);
            }
            return Ok(result);
        }

        // 5. Not found — look for a recently closed incident to reopen.
        let reopen_candidate = self
            .incidents
            .values()
            .filter(|i| i.correlation_key == key && i.state.is_terminal())
            .filter(|i| i.tenant_id == tenant)
            .max_by_key(|i| i.closed_at.map(|t| t.monotonic()));

        if let Some(candidate) = reopen_candidate {
            let now = self.now();
            if transition::evaluate_reopen(candidate, &self.reopen_policy, &now).unwrap_or(false) {
                let incident_id = candidate.incident_id;
                self.dedup_seen.insert(dedup, incident_id);
                return self.reopen_incident_internal(
                    auth,
                    incident_id,
                    event,
                    AutomaticCause::ReopenedByRecurrence,
                );
            }
        }

        // 6. Ended events with nothing to attach to create nothing.
        if event.kind == EventKind::Ended {
            return Ok(IngestResult {
                outcome_kind: IngestOutcomeKind::Quarantined,
                incident_id: None,
            });
        }

        // Create.
        let incident_id = self.create_incident_internal(auth, &tenant, &key, event)?;
        self.dedup_seen.insert(dedup, incident_id);
        Ok(IngestResult {
            outcome_kind: IngestOutcomeKind::Created,
            incident_id: Some(incident_id),
        })
    }

    fn create_incident_internal(
        &mut self,
        auth: &AuthorizationContext,
        tenant: &TenantId,
        key: &CorrelationKey,
        event: &DetectionEvent,
    ) -> Result<IncidentId, IncidentError> {
        if self.open_index.contains_key(key) {
            return Err(
                CorrelationConflict::OpenIncidentAlreadyExists(self.open_index[key]).into(),
            );
        }
        let now = self.now();
        let incident_id = self.incident_generator.generate();
        let allocation_year = 2026; // wall-clock year derivation is a display concern; fixed here since Timestamp carries no calendar API without a new dependency, and the number is provisional (FU-24) regardless.
        let incident_number = self
            .number_allocator
            .allocate(tenant.as_str(), allocation_year);

        let mut matched_metrics: BTreeSet<MetricKind> = BTreeSet::new();
        for reason in &event.matched {
            matched_metrics.insert(reason.metric);
        }
        let category = derive_category(&matched_metrics);

        let evidence_ref = EvidenceReference {
            detection_event_id: event.event_id.clone(),
            dedup_key: event.dedup_key.clone(),
            detection_id: event.detection_id.clone(),
            link_type: EvidenceLinkType::Opening,
            matched_metrics: matched_metrics.iter().copied().collect(),
        };
        let mut evidence = crate::evidence::EvidenceLedger::new();
        evidence.record(evidence_ref.clone());

        let incident = Incident {
            incident_id,
            incident_number,
            schema_version: INCIDENT_SCHEMA_VERSION,
            tenant_id: tenant.clone(),
            correlation_key: key.clone(),
            address_family: event.target.address_family,
            direction: event.target.direction,
            target_type: event.target.scope_type,
            target_identity: event.target.scope_id.clone(),
            created_by: Actor::System,
            title: event
                .summary
                .chars()
                .take(crate::limits::TITLE_MAX_LEN)
                .collect(),
            description: None,
            state: IncidentState::Open,
            severity: event.severity,
            severity_source: SeveritySource::Detection,
            priority: Priority::default_for(event.severity),
            closure_reason: None,
            state_before_recovering: None,
            suppression: None,
            version: 1,
            category,
            matched_metrics,
            first_detected_at: now,
            opened_at: now,
            last_detected_at: now,
            last_updated_at: now,
            acknowledged_at: None,
            recovering_since: None,
            resolved_at: None,
            closed_at: None,
            reopened_at: None,
            reopen_count: 0,
            assignment: crate::assignment::Assignment::unassigned(),
            updated_by: Actor::System,
            evidence,
            notes: Vec::new(),
            tags: std::collections::BTreeMap::new(),
            policy_refs: vec![crate::incident::PolicyRef {
                policy_id: event.policy_id.clone(),
                policy_version: event.policy_version,
                first_seen_sequence: event.sequence,
                last_seen_sequence: event.sequence,
            }],
        };

        self.incidents.insert(incident_id, incident);
        self.open_index.insert(key.clone(), incident_id);
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            Actor::System,
            TimelinePayload::Opened,
        ));
        let ts2 = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts2,
            incident_id,
            Actor::System,
            TimelinePayload::EventLinked {
                evidence: evidence_ref,
            },
        ));

        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            tenant.clone(),
            auth.actor().clone(),
            Permission::IncidentIngest,
            incident_id,
        ));

        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            tenant.clone(),
            incident_id,
            OutboxEvent::IncidentOpened,
        ));

        Ok(incident_id)
    }

    fn link_event_to_open_incident(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        event: &DetectionEvent,
    ) -> Result<IngestResult, IncidentError> {
        let now = self.now();
        let observed = crate::clock::Timestamp::now(self.clock.as_ref());

        let (evidence_ref, is_late, category_changed, old_category, new_category) = {
            let incident = self
                .incidents
                .get_mut(&incident_id)
                .ok_or(IncidentError::NotFound)?;
            let is_late = observed.monotonic() < incident.last_detected_at.monotonic();

            for reason in &event.matched {
                incident.matched_metrics.insert(reason.metric);
            }
            let new_category = derive_category(&incident.matched_metrics);
            let old_category = incident.category;
            let category_changed = new_category != old_category;

            let evidence_ref = EvidenceReference {
                detection_event_id: event.event_id.clone(),
                dedup_key: event.dedup_key.clone(),
                detection_id: event.detection_id.clone(),
                link_type: if is_late {
                    EvidenceLinkType::Late
                } else {
                    EvidenceLinkType::Update
                },
                matched_metrics: event.matched.iter().map(|m| m.metric).collect(),
            };
            incident.evidence.record(evidence_ref.clone());
            if !is_late {
                incident.last_detected_at = now;
            }
            incident.last_updated_at = now;
            incident.category = new_category;
            incident.version += 1;
            (
                evidence_ref,
                is_late,
                category_changed,
                old_category,
                new_category,
            )
        };
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        let payload = if is_late {
            TimelinePayload::LateEventLinked {
                evidence: evidence_ref,
            }
        } else {
            TimelinePayload::EventLinked {
                evidence: evidence_ref,
            }
        };
        self.timeline
            .push(TimelineEntry::new(ts, incident_id, Actor::System, payload));
        if category_changed {
            let ts2 = self.next_timeline_sequence();
            self.timeline.push(TimelineEntry::new(
                ts2,
                incident_id,
                Actor::System,
                TimelinePayload::CategoryChanged {
                    from: old_category,
                    to: new_category,
                },
            ));
        }
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentIngest,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentUpdated,
        ));

        Ok(IngestResult {
            outcome_kind: if is_late {
                IngestOutcomeKind::LinkedLate
            } else {
                IngestOutcomeKind::Updated
            },
            incident_id: Some(incident_id),
        })
    }

    fn reopen_incident_internal(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        event: &DetectionEvent,
        cause: AutomaticCause,
    ) -> Result<IngestResult, IncidentError> {
        let now = self.now();
        let key = {
            let incident = self
                .incidents
                .get(&incident_id)
                .ok_or(IncidentError::NotFound)?;
            if self.open_index.contains_key(&incident.correlation_key) {
                return Err(CorrelationConflict::OpenIncidentAlreadyExists(
                    self.open_index[&incident.correlation_key],
                )
                .into());
            }
            incident.correlation_key.clone()
        };

        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from_state = incident.state;
        incident.state = IncidentState::Open;
        incident.reopen_count += 1;
        incident.reopened_at = Some(now);
        incident.last_detected_at = now;
        incident.last_updated_at = now;
        incident.closure_reason = None;
        for reason in &event.matched {
            incident.matched_metrics.insert(reason.metric);
        }
        incident.category = derive_category(&incident.matched_metrics);
        let evidence_ref = EvidenceReference {
            detection_event_id: event.event_id.clone(),
            dedup_key: event.dedup_key.clone(),
            detection_id: event.detection_id.clone(),
            link_type: EvidenceLinkType::Opening,
            matched_metrics: event.matched.iter().map(|m| m.metric).collect(),
        };
        incident.evidence.record(evidence_ref);
        incident.version += 1;
        let reopen_count = incident.reopen_count;
        self.maybe_fail()?;

        self.open_index.insert(key, incident_id);

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            Actor::System,
            TimelinePayload::StateChanged {
                from: from_state,
                to: IncidentState::Open,
                cause: TransitionCause::Automatic(cause),
            },
        ));
        let ts2 = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts2,
            incident_id,
            Actor::System,
            TimelinePayload::Reopened {
                reopen_count,
                previous_incident_id: None,
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentIngest,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentReopened { reopen_count },
        ));

        Ok(IngestResult {
            outcome_kind: IngestOutcomeKind::Reopened,
            incident_id: Some(incident_id),
        })
    }

    // ---------------------------------------------------------------
    // Automatic maintenance (recovery entry / confirm / abort, auto-close)
    // ---------------------------------------------------------------

    pub fn enter_recovering(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        reason: transition::DetectionEndReason,
    ) -> Result<(), IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        self.check_tenant(auth, incident)?;
        let _ = transition::enter_recovering(incident, reason)?;
        let from = incident.state;
        let key = incident.correlation_key.clone();

        let incident = self.incidents.get_mut(&incident_id).expect("checked above");
        incident.state_before_recovering = Some(from);
        incident.state = IncidentState::Recovering;
        incident.recovering_since = Some(now);
        incident.last_updated_at = now;
        incident.version += 1;
        self.maybe_fail()?;
        // Recovering is still "open" for correlation purposes, so the
        // open_index entry is left untouched (it already points here).
        let _ = &key;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            Actor::System,
            TimelinePayload::StateChanged {
                from,
                to: IncidentState::Recovering,
                cause: TransitionCause::Automatic(reason.into()),
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentIngest,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentRecovering,
        ));
        Ok(())
    }

    pub fn confirm_recovery_if_due(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        recovery_confirmation: Duration,
    ) -> Result<bool, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        self.check_tenant(auth, incident)?;
        transition::confirm_recovery(incident)?;
        let recovering_since =
            incident
                .recovering_since
                .ok_or(IncidentError::InternalInvariantViolation(
                    "Recovering incident has no recovering_since",
                ))?;
        if now.elapsed_since(&recovering_since) < recovery_confirmation {
            return Ok(false);
        }
        self.resolve_internal(
            auth,
            incident_id,
            TransitionCause::Automatic(AutomaticCause::RecoveryConfirmed),
            None,
        )?;
        Ok(true)
    }

    pub fn abort_recovery(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
    ) -> Result<(), IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        self.check_tenant(auth, incident)?;
        let restored = transition::abort_recovery(incident)?;

        let incident = self.incidents.get_mut(&incident_id).expect("checked above");
        incident.state = restored;
        incident.recovering_since = None;
        incident.state_before_recovering = None;
        incident.last_updated_at = now;
        incident.version += 1;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            Actor::System,
            TimelinePayload::StateChanged {
                from: IncidentState::Recovering,
                to: restored,
                cause: TransitionCause::Automatic(AutomaticCause::RecoveryAborted),
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentIngest,
            incident_id,
        ));
        Ok(())
    }

    pub fn attempt_automatic_closure(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
    ) -> Result<bool, IncidentError> {
        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        self.check_tenant(auth, incident)?;
        match transition::attempt_automatic_closure(incident, &self.closure_policy) {
            Ok(()) => {
                self.close_internal(
                    auth,
                    incident_id,
                    crate::closure::ClosureReason::Resolved,
                    TransitionCause::Automatic(AutomaticCause::AutomaticClosure),
                )?;
                Ok(true)
            }
            Err(IncidentError::ManualClosureRequired) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn resolve_internal(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        cause: TransitionCause,
        resolution_note: Option<String>,
    ) -> Result<(), IncidentError> {
        let now = self.now();
        let (from, correlation_key) = {
            let incident = self
                .incidents
                .get_mut(&incident_id)
                .ok_or(IncidentError::NotFound)?;
            let from = incident.state;
            incident.state = IncidentState::Resolved;
            incident.resolved_at = Some(now);
            incident.recovering_since = None;
            incident.state_before_recovering = None;
            incident.last_updated_at = now;
            incident.version += 1;
            (from, incident.correlation_key.clone())
        };
        self.open_index.remove(&correlation_key);
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::StateChanged {
                from,
                to: IncidentState::Resolved,
                cause,
            },
        ));
        if let Some(note) = resolution_note {
            let incident = self.incidents.get_mut(&incident_id).expect("checked above");
            let idx = incident.notes.len() as u32;
            incident.notes.push(Note {
                index: idx,
                body: note,
                visibility: NoteVisibility::Internal,
                created_by: auth.actor().clone(),
            });
        }
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentResolve,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentResolved,
        ));
        Ok(())
    }

    fn close_internal(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        reason: crate::closure::ClosureReason,
        cause: TransitionCause,
    ) -> Result<(), IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from = incident.state;
        incident.state = IncidentState::Closed;
        incident.closed_at = Some(now);
        incident.closure_reason = Some(reason);
        incident.last_updated_at = now;
        incident.version += 1;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::StateChanged {
                from,
                to: IncidentState::Closed,
                cause,
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentClose,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentClosed,
        ));
        Ok(())
    }

    // ---------------------------------------------------------------
    // Operator commands
    // ---------------------------------------------------------------

    pub fn handle_command(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        command: Command,
        idempotency_key: Option<IdempotencyKey>,
    ) -> Result<u64, IncidentError> {
        let permission = command.required_permission();
        let resource = AttemptedResource::Incident(incident_id);
        self.check_permission(auth, permission, resource.clone())?;

        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        self.check_tenant(auth, incident)?;

        if let Some(key) = &idempotency_key {
            let fingerprint = RequestFingerprint::of(&command);
            match self.idempotency.check(auth.tenant(), key, &fingerprint) {
                IdempotencyCheck::Replay(StoredOutcome::Mutated { version, .. }) => {
                    return Ok(version)
                }
                IdempotencyCheck::Replay(StoredOutcome::Denied) => {
                    return Err(IncidentError::Unauthorized)
                }
                IdempotencyCheck::Conflict => return Err(IncidentError::IdempotencyConflict),
                IdempotencyCheck::New => {}
            }
        }

        if command.requires_expected_version() {
            let expected = command.expected_version().expect("checked above");
            if expected != incident.version {
                return Err(IncidentError::VersionConflict {
                    expected,
                    current: incident.version,
                    current_state: incident.state,
                });
            }
        }

        let result = self.apply_command(auth, incident_id, &command);

        if let Some(key) = idempotency_key {
            let fingerprint = RequestFingerprint::of(&command);
            let outcome = match &result {
                Ok(version) => StoredOutcome::Mutated {
                    incident_id,
                    version: *version,
                },
                Err(_) => StoredOutcome::Denied,
            };
            self.idempotency
                .record(auth.tenant().clone(), key, fingerprint, outcome);
        }

        result
    }

    fn apply_command(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        command: &Command,
    ) -> Result<u64, IncidentError> {
        match command {
            Command::AcknowledgeIncident { .. } => self.guarded_operator_transition(
                auth,
                incident_id,
                IncidentState::Acknowledged,
                |i, now| {
                    i.acknowledged_at = Some(now);
                },
            ),
            Command::BeginInvestigation { .. } => self.guarded_operator_transition(
                auth,
                incident_id,
                IncidentState::Investigating,
                |_, _| {},
            ),
            Command::MarkMonitoring { .. } => self.guarded_operator_transition(
                auth,
                incident_id,
                IncidentState::Monitoring,
                |_, _| {},
            ),
            Command::ResolveIncident {
                resolution_note, ..
            } => {
                self.resolve_internal(
                    auth,
                    incident_id,
                    TransitionCause::Operator(command.kind()),
                    resolution_note.clone(),
                )?;
                Ok(self.incidents[&incident_id].version)
            }
            Command::CloseIncident { reason, .. } => {
                let incident = self
                    .incidents
                    .get(&incident_id)
                    .ok_or(IncidentError::NotFound)?;
                if incident.state != IncidentState::Resolved {
                    return Err(IncidentError::InvalidTransition {
                        from: incident.state,
                        to: IncidentState::Closed,
                    });
                }
                self.close_internal(
                    auth,
                    incident_id,
                    *reason,
                    TransitionCause::Operator(command.kind()),
                )?;
                Ok(self.incidents[&incident_id].version)
            }
            Command::ReopenIncident { reason, .. } => {
                self.operator_reopen(auth, incident_id, reason.clone())
            }
            Command::SuppressIncident {
                reason, duration, ..
            } => self.suppress(auth, incident_id, reason.clone(), *duration),
            Command::UnsuppressIncident { .. } => self.unsuppress(auth, incident_id),
            Command::AssignIncident { assignee, .. } => {
                self.assign(auth, incident_id, Some(assignee.clone()))
            }
            Command::UnassignIncident { .. } => self.assign(auth, incident_id, None),
            Command::ChangeSeverity {
                new_severity,
                reason,
                ..
            } => self.change_severity(auth, incident_id, *new_severity, reason.clone()),
            Command::ChangePriority { new_priority, .. } => {
                self.change_priority(auth, incident_id, *new_priority)
            }
            Command::AddNote { body, visibility } => {
                self.add_note(auth, incident_id, body.clone(), visibility.clone())
            }
            Command::AddTag { key, value } => {
                self.add_tag(auth, incident_id, key.clone(), value.clone())
            }
            Command::RemoveTag { key } => self.remove_tag(auth, incident_id, key.clone()),
        }
    }

    fn guarded_operator_transition(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        to: IncidentState,
        extra: impl FnOnce(&mut Incident, Timestamp),
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from = incident.state;
        if from == to {
            return Err(IncidentError::StateUnchanged(from));
        }
        if !from.can_operator_transition_to(to) {
            return Err(IncidentError::InvalidTransition { from, to });
        }
        let incident = self.incidents.get_mut(&incident_id).expect("checked above");
        incident.state = to;
        incident.last_updated_at = now;
        incident.version += 1;
        extra(incident, now);
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::StateChanged {
                from,
                to,
                cause: TransitionCause::Operator(crate::timeline::OperatorCommandKind::Acknowledge),
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentAcknowledge,
            incident_id,
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn operator_reopen(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        reason: String,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from = incident.state;
        if !from.can_operator_transition_to(IncidentState::Open) {
            return Err(IncidentError::InvalidTransition {
                from,
                to: IncidentState::Open,
            });
        }
        let key = incident.correlation_key.clone();
        if self.open_index.contains_key(&key) {
            return Err(
                CorrelationConflict::OpenIncidentAlreadyExists(self.open_index[&key]).into(),
            );
        }
        let incident = self.incidents.get_mut(&incident_id).expect("checked above");
        incident.state = IncidentState::Open;
        incident.reopen_count += 1;
        incident.reopened_at = Some(now);
        incident.closure_reason = None;
        incident.last_updated_at = now;
        incident.version += 1;
        let reopen_count = incident.reopen_count;
        self.maybe_fail()?;
        self.open_index.insert(key, incident_id);

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::StateChanged {
                from,
                to: IncidentState::Open,
                cause: TransitionCause::Operator(crate::timeline::OperatorCommandKind::Reopen),
            },
        ));
        let ts2 = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts2,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::Reopened {
                reopen_count,
                previous_incident_id: None,
            },
        ));
        let _ = reason;
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentReopen,
            incident_id,
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn suppress(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        reason: String,
        duration: Duration,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let deadline = now.plus(duration);
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        incident.suppression = Some(Suppression::new(
            reason.clone(),
            auth.actor().clone(),
            deadline,
        ));
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::Suppressed { reason },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentSuppress,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentSuppressionChanged { suppressed: true },
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn unsuppress(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        incident.suppression = None;
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::Unsuppressed,
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentSuppress,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentSuppressionChanged { suppressed: false },
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn assign(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        assignee: Option<Assignee>,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from = incident.assignment.assignee.clone();
        incident.assignment.assignee = assignee.clone();
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::AssignmentChanged { from, to: assignee },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentAssign,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentAssignmentChanged,
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn change_severity(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        new_severity: wetechinetmon_detector::Severity,
        reason: Option<String>,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from = incident.severity;
        if (new_severity as u8) < (from as u8) && reason.is_none() {
            return Err(IncidentError::ValidationError(
                "a reason is required when lowering severity".to_string(),
            ));
        }
        incident.severity = new_severity;
        incident.severity_source = SeveritySource::Operator;
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::SeverityChanged {
                from,
                to: new_severity,
                reason,
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentSeverityChange,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentSeverityChanged {
                from,
                to: new_severity,
            },
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn change_priority(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        new_priority: Priority,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        let from = incident.priority;
        incident.priority = new_priority;
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::PriorityChanged {
                from,
                to: new_priority,
            },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentPriorityChange,
            incident_id,
        ));
        let osq = self.next_outbox_sequence();
        self.outbox.push(OutboxMessage::new(
            osq,
            auth.tenant().clone(),
            incident_id,
            OutboxEvent::IncidentPriorityChanged {
                from,
                to: new_priority,
            },
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn add_note(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        body: String,
        visibility: NoteVisibility,
    ) -> Result<u64, IncidentError> {
        if visibility == NoteVisibility::CustomerVisible {
            return Err(IncidentError::ValidationError(
                "customer-visible notes are refused in Phase 5".to_string(),
            ));
        }
        validate_note_body(&body)?;
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        if incident.notes_at_capacity() {
            return Err(IncidentError::CapacityExceeded("notes per incident"));
        }
        let index = incident.notes.len() as u32;
        incident.notes.push(Note {
            index,
            body,
            visibility,
            created_by: auth.actor().clone(),
        });
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;

        let ts = self.next_timeline_sequence();
        self.timeline.push(TimelineEntry::new(
            ts,
            incident_id,
            auth.actor().clone(),
            TimelinePayload::NoteAdded { note_index: index },
        ));
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentNoteCreate,
            incident_id,
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn add_tag(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        key: String,
        value: String,
    ) -> Result<u64, IncidentError> {
        if key.len() > TAG_KEY_MAX_LEN || value.len() > TAG_VALUE_MAX_LEN {
            return Err(IncidentError::ValidationError(
                "tag key or value exceeds its bound".to_string(),
            ));
        }
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        if !incident.tags.contains_key(&key) && incident.tags_at_capacity() {
            return Err(IncidentError::CapacityExceeded("tags per incident"));
        }
        incident.tags.insert(key, value);
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentUpdate,
            incident_id,
        ));
        Ok(self.incidents[&incident_id].version)
    }

    fn remove_tag(
        &mut self,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        key: String,
    ) -> Result<u64, IncidentError> {
        let now = self.now();
        let incident = self
            .incidents
            .get_mut(&incident_id)
            .ok_or(IncidentError::NotFound)?;
        incident.tags.remove(&key);
        incident.version += 1;
        incident.last_updated_at = now;
        self.maybe_fail()?;
        let asq = self.next_audit_sequence();
        self.audit.push(AuditEntry::allowed(
            asq,
            auth.tenant().clone(),
            auth.actor().clone(),
            Permission::IncidentUpdate,
            incident_id,
        ));
        Ok(self.incidents[&incident_id].version)
    }
}

/// Never referenced outside this module — retained purely so
/// `AFFECTED_TARGETS_MAX` participates in at least one compiled path
/// until a future milestone tracks affected-target counts on the
/// aggregate itself.
#[allow(dead_code)]
const _: usize = AFFECTED_TARGETS_MAX;
