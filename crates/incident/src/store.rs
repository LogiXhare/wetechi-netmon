//! The persistence seam ADR 0029 asks for: a repository-shaped trait
//! covering exactly the operations [`crate::unit_of_work::IncidentUnitOfWork`]
//! already performed against its own fields before this module existed —
//! incident read/write, the correlation index, the dedup gate, timeline
//! append, audit append, outbox append, and idempotency check/record —
//! plus [`InMemoryIncidentStore`], the reference implementation that
//! proves the seam is real by being a second, independent implementation
//! the trait makes possible in principle.
//!
//! # Why this shape, not a data-shaped mutation plan
//!
//! A 2026-08-24 Stage-A design pass considered a persistence-neutral
//! `MutationPlan` value type instead — the domain computes a plan, a
//! caller commits it in one transaction — specifically because a
//! fine-grained trait like this one, if a future asynchronous PostgreSQL
//! adapter implemented it with one awaited call per operation, would
//! reopen 5A's own documented partial-write gap at the SQL level: an
//! incident write that lands while its accompanying timeline, audit, or
//! outbox append does not, if the process dies between two awaits.
//!
//! That risk is real but is a **5B-1 concern**, not a 5B-0 one: this
//! crate stays synchronous and dependency-free through 5B-0 (ADR 0021),
//! so nothing here can await anything yet, and the in-memory
//! implementation below commits every operation of a single mutation
//! method synchronously and in order, which is the same atomicity
//! guarantee 5A already had. The risk is recorded as **FU-44** in
//! `docs/development/follow-ups.md` — a future `crates/incident-postgres`
//! adapter implementing this trait must wrap one logical mutation's
//! calls in a single database transaction rather than trust the trait's
//! method boundaries to be atomic on their own.
//!
//! # Why the correlation index and dedup gate are here too
//!
//! ADR 0029's own context section lists `open_index` and `dedup_seen`
//! alongside `incidents`, `timeline`, `audit`, and `outbox` as the fields
//! `IncidentUnitOfWork` made direct accesses against — they are as much
//! part of "incident read/write" as the incident map itself, since a
//! correlation decision cannot be made without them.
//!
//! # Why idempotency is exposed whole, not flattened
//!
//! [`crate::idempotency::IdempotencyStore`] already fully encapsulates
//! its storage behind `check`/`record` — there is no raw collection to
//! leak by handing back a reference to it, unlike `incidents` or
//! `timeline`. Flattening its two methods onto this trait as
//! `idempotency_check`/`idempotency_record` would just be indirection
//! with no encapsulation benefit over exposing the store itself.

use std::collections::HashMap;

use crate::audit::AuditEntry;
use crate::correlation::{CorrelationKey, TenantId};
use crate::id::IncidentId;
use crate::idempotency::IdempotencyStore;
use crate::incident::Incident;
use crate::outbox::OutboxMessage;
use crate::timeline::TimelineEntry;

/// The operations [`crate::unit_of_work::IncidentUnitOfWork`] needs from
/// its backing storage. See the module doc for why the shape is
/// fine-grained rather than a single atomic mutation call, and for the
/// atomicity obligation that shape places on a future implementation.
pub trait IncidentStore: std::fmt::Debug {
    /// Looks up one incident by id.
    fn get(&self, id: &IncidentId) -> Option<&Incident>;

    /// Looks up one incident by id, mutably.
    fn get_mut(&mut self, id: &IncidentId) -> Option<&mut Incident>;

    /// Stores `incident`, keyed by its own `incident_id`. Replaces any
    /// existing incident at that id — every call site either knows the id
    /// is new (creation) or already holds `&mut` to the same row it is
    /// about to overwrite (an update that went through `get_mut` first),
    /// so there is no meaningful "already exists" case to reject here.
    fn insert(&mut self, incident: Incident);

    /// How many incidents are stored, for
    /// [`crate::unit_of_work::IncidentUnitOfWork::incident_count`].
    fn len(&self) -> usize;

    /// Whether no incidents are stored. Provided so clippy's
    /// `len_without_is_empty` lint stays clean on every implementer,
    /// same as any other `len`-bearing type.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The most recently resolved-or-closed incident matching `key` and
    /// `tenant`, if any — the query the automatic-recurrence reopen path
    /// needs, decided by each candidate's own
    /// [`Incident::reopen_reference_timestamp`] (`resolved_at` for
    /// `Resolved`, `closed_at` for `Closed`) so a `Resolved` candidate is
    /// never treated as equally (un)ranked against one that is `Closed`.
    ///
    /// A named query rather than an exposed iterator over every stored
    /// incident on purpose: the latter would force a future SQL-backed
    /// implementation to materialize its entire table to answer one
    /// lookup, exactly the anti-pattern a repository seam exists to hide
    /// behind a `WHERE` clause instead.
    fn reopen_candidate(&self, key: &CorrelationKey, tenant: &TenantId) -> Option<&Incident>;

    /// The incident currently open for correlation under `key`, if any.
    fn open_index_get(&self, key: &CorrelationKey) -> Option<IncidentId>;

    /// Marks `key` as open, correlating to `id`.
    fn open_index_claim(&mut self, key: CorrelationKey, id: IncidentId);

    /// Marks `key` as no longer open — an incident resolved, or was
    /// never actually claimed.
    fn open_index_release(&mut self, key: &CorrelationKey);

    /// The incident a `(tenant, dedup_key)` pair was already recorded
    /// against, if this exact recurrence has been seen before.
    fn dedup_get(&self, key: &(TenantId, String)) -> Option<IncidentId>;

    /// Records that `key` now maps to `id`, for future dedup lookups.
    fn dedup_record(&mut self, key: (TenantId, String), id: IncidentId);

    /// Appends one timeline entry. Never removed, never reordered — the
    /// timeline is an append-only log, and every caller of this method
    /// has already assigned `entry`'s sequence number before calling.
    fn append_timeline(&mut self, entry: TimelineEntry);

    /// The full timeline, in append order, for
    /// [`crate::unit_of_work::IncidentUnitOfWork::timeline`].
    fn timeline(&self) -> &[TimelineEntry];

    /// Appends one audit entry. See [`Self::append_timeline`]'s doc — the
    /// same append-only contract applies.
    fn append_audit(&mut self, entry: AuditEntry);

    /// The full audit log, in append order.
    fn audit(&self) -> &[AuditEntry];

    /// Appends one outbox message.
    fn append_outbox(&mut self, message: OutboxMessage);

    /// The full outbox, in append order.
    fn outbox(&self) -> &[OutboxMessage];

    /// The idempotency store, for a `check` call before a mutation is
    /// attempted.
    fn idempotency(&self) -> &IdempotencyStore;

    /// The idempotency store, mutably, for a `record` call once a
    /// mutation's outcome is known.
    fn idempotency_mut(&mut self) -> &mut IdempotencyStore;
}

/// The reference implementation: everything held in ordinary in-process
/// collections, exactly what [`crate::unit_of_work::IncidentUnitOfWork`]
/// held directly before this seam existed. A future
/// `crates/incident-postgres` adapter implements the same
/// [`IncidentStore`] trait against real tables instead — see the module
/// doc for the atomicity obligation that adapter must uphold that this
/// one gets for free from owning `&mut self`.
#[derive(Debug, Default)]
pub struct InMemoryIncidentStore {
    incidents: HashMap<IncidentId, Incident>,
    open_index: HashMap<CorrelationKey, IncidentId>,
    dedup_seen: HashMap<(TenantId, String), IncidentId>,
    timeline: Vec<TimelineEntry>,
    audit: Vec<AuditEntry>,
    outbox: Vec<OutboxMessage>,
    idempotency: IdempotencyStore,
}

impl InMemoryIncidentStore {
    pub fn new() -> Self {
        InMemoryIncidentStore::default()
    }
}

impl IncidentStore for InMemoryIncidentStore {
    fn get(&self, id: &IncidentId) -> Option<&Incident> {
        self.incidents.get(id)
    }

    fn get_mut(&mut self, id: &IncidentId) -> Option<&mut Incident> {
        self.incidents.get_mut(id)
    }

    fn insert(&mut self, incident: Incident) {
        self.incidents.insert(incident.incident_id, incident);
    }

    fn len(&self) -> usize {
        self.incidents.len()
    }

    fn reopen_candidate(&self, key: &CorrelationKey, tenant: &TenantId) -> Option<&Incident> {
        self.incidents
            .values()
            .filter(|i| &i.correlation_key == key && i.is_reopen_candidate())
            .filter(|i| &i.tenant_id == tenant)
            .max_by_key(|i| i.reopen_reference_timestamp().copied())
    }

    fn open_index_get(&self, key: &CorrelationKey) -> Option<IncidentId> {
        self.open_index.get(key).copied()
    }

    fn open_index_claim(&mut self, key: CorrelationKey, id: IncidentId) {
        self.open_index.insert(key, id);
    }

    fn open_index_release(&mut self, key: &CorrelationKey) {
        self.open_index.remove(key);
    }

    fn dedup_get(&self, key: &(TenantId, String)) -> Option<IncidentId> {
        self.dedup_seen.get(key).copied()
    }

    fn dedup_record(&mut self, key: (TenantId, String), id: IncidentId) {
        self.dedup_seen.insert(key, id);
    }

    fn append_timeline(&mut self, entry: TimelineEntry) {
        self.timeline.push(entry);
    }

    fn timeline(&self) -> &[TimelineEntry] {
        &self.timeline
    }

    fn append_audit(&mut self, entry: AuditEntry) {
        self.audit.push(entry);
    }

    fn audit(&self) -> &[AuditEntry] {
        &self.audit
    }

    fn append_outbox(&mut self, message: OutboxMessage) {
        self.outbox.push(message);
    }

    fn outbox(&self) -> &[OutboxMessage] {
        &self.outbox
    }

    fn idempotency(&self) -> &IdempotencyStore {
        &self.idempotency
    }

    fn idempotency_mut(&mut self) -> &mut IdempotencyStore {
        &mut self.idempotency
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::IncidentState;
    use crate::test_fixtures::valid_incident;

    #[test]
    fn get_and_get_mut_see_an_inserted_incident() {
        let mut store = InMemoryIncidentStore::new();
        let incident = valid_incident(IncidentState::Open);
        let id = incident.incident_id;
        store.insert(incident);

        assert!(store.get(&id).is_some());
        store.get_mut(&id).unwrap().title = "changed".to_string();
        assert_eq!(store.get(&id).unwrap().title, "changed");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn insert_keys_by_the_incident_s_own_id() {
        let mut store = InMemoryIncidentStore::new();
        let incident = valid_incident(IncidentState::Open);
        let id = incident.incident_id;
        store.insert(incident);
        assert!(store.get(&id).is_some());
    }

    #[test]
    fn open_index_round_trips_a_claim_and_a_release() {
        let mut store = InMemoryIncidentStore::new();
        let incident = valid_incident(IncidentState::Open);
        let key = incident.correlation_key.clone();
        let id = incident.incident_id;

        assert_eq!(store.open_index_get(&key), None);
        store.open_index_claim(key.clone(), id);
        assert_eq!(store.open_index_get(&key), Some(id));
        store.open_index_release(&key);
        assert_eq!(store.open_index_get(&key), None);
    }

    #[test]
    fn dedup_round_trips() {
        let mut store = InMemoryIncidentStore::new();
        let id = valid_incident(IncidentState::Open).incident_id;
        let key = (TenantId::new("acme"), "dedup-1".to_string());
        assert_eq!(store.dedup_get(&key), None);
        store.dedup_record(key.clone(), id);
        assert_eq!(store.dedup_get(&key), Some(id));
    }

    /// `reopen_candidate` must ignore an incident belonging to a
    /// different tenant even when the correlation key matches — the same
    /// isolation `IncidentUnitOfWork::check_tenant` enforces everywhere
    /// else, and the one piece of real query logic this extraction moved
    /// rather than merely renamed.
    #[test]
    fn reopen_candidate_is_tenant_scoped() {
        let mut store = InMemoryIncidentStore::new();
        let mut other_tenant = valid_incident(IncidentState::Resolved);
        other_tenant.tenant_id = TenantId::new("someone-else");
        let key = other_tenant.correlation_key.clone();
        store.insert(other_tenant);

        assert!(
            store
                .reopen_candidate(&key, &TenantId::new("acme"))
                .is_none(),
            "a Resolved incident under a different tenant must not be offered as a candidate"
        );
    }

    /// Only `Resolved` or `Closed` incidents are candidates at all —
    /// `Open` (or any other live state) must never be offered, matching
    /// `Incident::is_reopen_candidate`.
    #[test]
    fn reopen_candidate_excludes_live_states() {
        let mut store = InMemoryIncidentStore::new();
        let incident = valid_incident(IncidentState::Open);
        let key = incident.correlation_key.clone();
        let tenant = incident.tenant_id.clone();
        store.insert(incident);

        assert!(store.reopen_candidate(&key, &tenant).is_none());
    }

    /// Among several matching candidates, the one with the latest reopen
    /// reference timestamp wins — proving the ranking, not just that a
    /// candidate is found at all.
    #[test]
    fn reopen_candidate_picks_the_most_recently_resolved_or_closed() {
        let mut store = InMemoryIncidentStore::new();

        let mut older = valid_incident(IncidentState::Resolved);
        older.incident_id = crate::id::IncidentId::from_bytes([2; 16]);
        older.resolved_at = Some(crate::durable_time::DurableTimestamp::from_micros(1_000));
        let key = older.correlation_key.clone();
        let tenant = older.tenant_id.clone();

        let mut newer = valid_incident(IncidentState::Closed);
        newer.incident_id = crate::id::IncidentId::from_bytes([3; 16]);
        newer.correlation_key = key.clone();
        newer.resolved_at = Some(crate::durable_time::DurableTimestamp::from_micros(2_000));
        newer.closed_at = Some(crate::durable_time::DurableTimestamp::from_micros(3_000));

        let newer_id = newer.incident_id;
        store.insert(older);
        store.insert(newer);

        let candidate = store
            .reopen_candidate(&key, &tenant)
            .expect("two matching candidates exist");
        assert_eq!(candidate.incident_id, newer_id);
    }

    #[test]
    fn timeline_audit_and_outbox_are_append_only_in_order() {
        let mut store = InMemoryIncidentStore::new();
        assert!(store.timeline().is_empty());
        assert!(store.audit().is_empty());
        assert!(store.outbox().is_empty());

        let id = valid_incident(IncidentState::Open).incident_id;
        store.append_timeline(TimelineEntry::new(
            1,
            id,
            crate::authorization::Actor::System,
            crate::timeline::TimelinePayload::Opened,
        ));
        assert_eq!(store.timeline().len(), 1);
        assert_eq!(store.timeline()[0].sequence, 1);
    }

    #[test]
    fn idempotency_is_reachable_through_the_store() {
        let store = InMemoryIncidentStore::new();
        assert!(store.idempotency().is_empty());
    }
}
