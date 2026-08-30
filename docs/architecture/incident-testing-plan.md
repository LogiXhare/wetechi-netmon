# Incident Testing Plan

Status: **Planning only.** Part of the
[Phase 5 plan](phase5-incident-management-plan.md). No tests are written.

Phase 4 finished with 403 tests and a full IPFIX-bytes-to-event
end-to-end test. Phase 5 must meet the same bar: **failure paths tested,
not just happy paths**, and no test that sleeps.

## Ground rules

- **Injectable clock everywhere.** Phase 4 has a `TestClock`; Phase 5
  reuses that pattern. A test that sleeps five minutes to check the
  recovery window is a test nobody will run.
- **In-memory repositories** for domain and state-machine tests, so
  Milestone 5A is fully testable before any schema exists.
- **A real PostgreSQL** for persistence tests. Transaction atomicity,
  partial unique indexes, and constraint violations cannot be tested
  against a fake — the fake would have to reimplement the very behaviour
  under test.
- **No network in unit tests.**
- Every threat in the
  [threat model](../security/incident-threat-model.md) has a test.

## Domain tests

Incident creation from a detection event; correlation key construction
and canonicalisation, including IPv6 spellings; duplicate event; multiple
matched reasons on one event; multiple policies onto one incident;
`/32` and parent `/24` producing **separate** incidents; category
derivation for all ten categories including `multi_vector` precedence;
category change over an incident's life; recurrence inside and outside
the reopen window; recovery, abort, and prior-state restoration; closure;
suppression absorbing events without transitioning; severity derived from
the highest contributing policy; operator severity override not being
overwritten by the next event.

## State-machine tests

- **Every legal transition**, asserting resulting state, timeline entry,
  audit record, and version increment.
- **Every illegal transition** rejected with the right error code — a
  matrix test over all states × all commands, so a future added state
  cannot quietly acquire undefined edges.
- Idempotent repeat with the same key; conflicting concurrent
  transitions; each automatic transition; each operator transition; role
  denial per command; tenant denial per command.
- The five distinct end reasons — `traffic_cleared`, `detector_stale`,
  `detector_silent`, `policy_withdrawn`, `detector_reset` — each produce
  the correct recorded reason, since conflating them is the failure that
  makes an operator close a live incident.

### Closure policy (BQ-8)

- A `critical` incident reaches `Resolved` automatically and **does not**
  auto-close, under default configuration, however long the clock runs.
- An operator holding `incident.close` closes a `critical` incident.
- An operator without `incident.close` is refused.
- A non-critical incident auto-closes once
  `automatic_closure_delay` elapses, when closure is enabled.
- `Resolved` and `Closed` remain distinct: a resolved incident is not
  reported as closed by any query, metric, or representation.
- Overriding `critical_manual_closure_required` writes an audit record
  naming actor, scope, old value, and new value.
- The override is refused without `incident.closure_policy.override`.
- Effective-configuration diagnostics report the effective value and its
  source.
- A duplicate `CloseIncident` with the same idempotency key replays the
  original result rather than closing twice.
- Concurrent `CloseIncident` commands produce exactly one winner and one
  `409`.

### Reopen window (BQ-9)

Boundary tests use the injectable clock; none of them sleep.

- Recurrence at **14 m 59 s** reopens.
- Recurrence at **exactly 15 m 00 s** reopens — the boundary is
  **inclusive**, and this test is the one that pins it.
- Recurrence at **15 m 01 s** creates a new incident referencing its
  predecessor.
- `reopen_window = 0` always creates a new incident.
- Elapsed time is measured from `closed_at` when the incident never
  passed through `Resolved`.
- A reopen links the new detection evidence.
- `reopen_count` increments and `reopened_at` updates.
- The timeline remains append-only, and **all prior entries and evidence
  are unchanged** after a reopen.
- The incident keeps its original `incident_id` and `incident_number`.
- A duplicate recurrence event is idempotent — one reopen, not two.
- **Concurrent reopen attempts leave exactly one active incident** for the
  correlation key, enforced by the partial unique index rather than by
  application ordering.
- Cross-tenant recurrence never correlates.
- Opposite directions never correlate.
- Host and parent prefix never correlate.
- A category change does not split an incident.
- A policy change does not split an incident.
- A detector restart, minting a new `detection_id`, still correlates the
  recurrence onto the existing incident.

## Persistence tests

Atomicity is the centre of this section, and it must be tested by
**injecting failure**, not by observing success:

- State change, timeline, audit, and outbox commit together; a forced
  failure at each of the four points rolls back **all** of them.
- **An audit write failure rolls back the mutation.** The mutation must
  not be visible afterwards.
- Optimistic concurrency: conflicting updates produce exactly one winner.
- **Each of the three target-specific partial unique indexes**
  (`incidents_active_host`, `incidents_active_network`,
  `incidents_active_hostgroup` — corrected from a single generic index,
  see [incident-persistence.md](incident-persistence.md)'s "Active-
  incident invariant" section — **not** ADR 0032, which is cited
  incorrectly here as of the original planning pass; ADR 0032 covers
  tenant isolation and RLS readiness, not the typed-target index design)
  actually prevents two active incidents per correlation key, tested
  with genuine concurrent inserts rather than sequential ones, for each
  target type independently.
- Duplicate `dedup_key` insert raises the constraint and is handled as a
  duplicate rather than an error.
- Idempotency: same key and body replays; same key, different body
  conflicts.
- Rollback leaves no orphan timeline or audit rows.
- Retry after a transient failure does not double-apply.
- Database outage produces `503` and no partial state.
- Migrations apply cleanly forward, and the incident schema version is
  recorded, across the full supported version matrix
  ([ADR 0025](decisions/0025-phase5b-postgresql-version-support.md):
  15, 16, 17, 18).
- Retention deletes what it should and **never cascades into audit**.
- Tenant isolation at the repository layer: a tenant-less query cannot be
  constructed.

### Phase 5B additions (2026-08-24 planning)

- **Seam-extraction regression:** all 531 existing Phase 5A tests remain
  green against the extracted repository seam (Milestone 5B-0), proving
  the refactor changed no behaviour — see
  [ADR 0029](decisions/0029-phase5b-repository-and-unit-of-work-seam.md).
- **Aggregate reconstitution:** `Incident::reconstitute` rejects a
  structurally invalid snapshot (e.g. `state_before_recovering` set
  while `state != Recovering`) rather than silently accepting it — see
  [ADR 0030](decisions/0030-phase5b-aggregate-reconstitution.md).
- **Clock-skew contract:** a decision timestamp earlier than the
  persisted reference does not clamp, does not reopen, does not create a
  duplicate incident, and returns a structured error — see
  [ADR 0031](decisions/0031-phase5b-durable-time.md).
- **FU-38 acceptance gate:** the table-driven "every command × every
  illegal source state" test exercises `close_internal` and
  `reopen_incident_internal` directly, post-hardening.
- **UUIDv7 round trip:** generated identity round-trips through
  PostgreSQL's native `uuid` column and back without loss.
- **`incident_policy_references` overflow behaviour:** a 65th distinct
  policy on one incident is recorded, not silently dropped — either
  normalized without a cap, or with an explicit omitted-count increment,
  never a silent no-op.
- **Outbox lease and dead-letter:** a claimed-but-never-published row is
  reclaimed by a different consumer after its lease expires; a row that
  exceeds its retry limit transitions to dead-letter, never retries
  indefinitely.
- **Duplicate-consumer idempotency:** a re-claimed and re-published
  outbox event does not corrupt downstream state under at-least-once
  delivery.
- **Windows-GNU and Linux builds** of the full Phase 5B suite, per
  [ADR 0018](decisions/0018-phase5-dependency-selection.md)'s
  non-negotiable cross-platform requirement.

### Tenant-aware composite foreign keys (added 2026-08-30)

Per [incident-persistence.md](incident-persistence.md)'s "Tenant-aware
composite foreign keys" section and
[ADR 0032](decisions/0032-phase5b-tenant-isolation-and-rls-readiness.md):

- **Same-tenant child reference succeeds:** inserting a timeline, note,
  detection-event, policy-reference, assignment, or tag row whose
  `(tenant_id, incident_id)` matches a real incident's own
  `(tenant_id, incident_id)` succeeds normally.
- **Cross-tenant child reference fails:** inserting the same row with
  the *correct* `incident_id` but the *wrong* `tenant_id` (or vice
  versa) is rejected by the foreign-key constraint itself
  (`23503`), not merely by an application-level check — proving the
  composite key, not a single-column one, is what is actually enforced.
- **Note supersession is tenant-checked:** a note cannot be recorded as
  superseding a note belonging to a different tenant — the
  `FOREIGN KEY (tenant_id, supersedes_note_id) REFERENCES
  incident_notes (tenant_id, note_id)` rejects a cross-tenant
  `supersedes_note_id`, even when the referencing note's own tenant
  matches its own incident correctly.
- **Outbox and audit cannot reference another tenant's incident, at the
  application layer:** since `incident_outbox` and `incident_audit`
  are deliberately not foreign-key-constrained to `incidents` (see
  those tables' own sections in
  [incident-persistence.md](incident-persistence.md) for why), this
  invariant is application-enforced, not database-enforced — the test
  asserts the repository layer refuses to write an outbox or audit row
  whose `aggregate_id`/`resource_id` names an incident belonging to a
  different tenant than the row's own `tenant_id`, closing the gap a
  missing foreign key leaves open for exactly these two tables.
- **Archival preserves required audit history:** purging a closed
  incident past its 24-month retention window (which cascades to its
  timeline, notes, detection events, policy references, assignments,
  and tags via `ON DELETE CASCADE`) does **not** remove that incident's
  `incident_audit` rows — audit has no foreign key to `incidents` and
  therefore nothing to cascade from, and the retention table's own
  24-month-minimum audit window is independently verified to still hold
  the rows after the incident itself is gone.

## API tests

Pagination including cursor stability while rows are inserted mid-page —
the specific bug offset pagination has and cursors do not. Filtering on
every documented filter; sorting on every documented field; rejection of
undocumented sort fields (T-10). Authorization per endpoint per role.
Tenant context: another tenant's ID returns **404, not 403**, on every
endpoint including audit. Validation: unknown fields rejected, malformed
IDs rejected, oversized notes rejected, suppression without expiry
rejected. Conflict responses carrying current version and state. Rate
limiting. `Idempotency-Key` required on transitions and absent-key
behaviour. Error bodies carrying stable `error` codes.

## Property tests

Following Phase 4's use of `proptest`:

1. No sequence of commands reaches an undefined state.
2. The timeline is append-only — no operation reduces its length or
   alters an existing entry.
3. `version` increases strictly monotonically per incident.
4. Duplicate ingestion never creates a second incident, for any
   interleaving.
5. Every state mutation produces exactly one timeline entry.
6. Every mandatory mutation produces exactly one audit record.
7. Tenant isolation: no operation as tenant A returns or alters data for
   tenant B, for any generated operation sequence.
8. No incident exceeds any configured limit.
9. Same idempotency key and same request returns the same result.
10. Same idempotency key and different request always conflicts.
11. `last_detected_at` never moves backwards, for any event order.
12. `peak_metrics` are monotonically non-decreasing per metric.
13. Correlation is order-independent: the same event set in any order
    yields the same final incident set. This is the property that makes
    outbox replay safe, and it is the single most valuable test here.

## End-to-end test

Extending Phase 4's `synthetic_ipfix_produces_a_dry_run_detection_event_end_to_end`
to run the whole chain:

```text
synthetic IPFIX datagram
  → collector decode
  → normalized flow
  → direction classification
  → aggregation
  → detector evaluation
  → detection event
  → outbox
  → correlation
  → incident opened
  → timeline entry
  → audit record
  → API query returns the incident
```

Asserting, as Phase 4's test does, on **contents** rather than merely
that something appeared: tenant, target, direction, category, severity,
policy references, opening reason, incident number format, timeline
entries in order, audit record present, and the API representation.

And asserting the negatives, which matter as much:

- `executed` remains `false` on every linked detection event.
- `mitigation_status` and `notification_status` are `"none"`.
- **No notification is delivered** — asserted with a recording sink that
  must receive nothing.
- **No mitigation is attempted** — asserted structurally, by the incident
  crate's dependency closure containing no transport crate, exactly as
  ADR 0007 is verified for the detector.

## Performance plan

Benchmarks to be **written and run** before Phase 5 ships. **No
performance number is claimed in this plan**, and none should appear in
any Phase 5 document until it has been measured — the same discipline
that left Phase 4's throughput unpublished (FU-12).

Operations to measure: ingestion throughput; correlation lookup;
incident creation; update; timeline append; audit append; unfiltered and
filtered list queries; concurrent operator commands; duplicate ingestion;
outbox publication; tenant-scoped queries.

Dimensions to vary: 10 / 1 000 / 10 000 active incidents; 100 000 /
1 000 000 closed; 10 / 1 000 / 10 000 events per incident; 1 / 10 / 100
tenants; 1 / 10 / 50 concurrent operators; ingestion at 1 / 100 / 1 000
events per second; timelines of 100 / 10 000 / 50 000 entries.

The specific questions worth answering, since they are where this design
is most likely to disappoint: does list-query latency stay acceptable at
a million closed incidents, and does correlation stay constant-time as
open incidents grow?

## Coverage expectations

Not a percentage target, which measures the wrong thing. Instead:

- Every state-machine edge, legal and illegal.
- Every failure branch in the persistence layer.
- Every threat in the threat model.
- Every documented API error code.
- Every capacity limit, at the limit and one beyond.

The existing **403 Rust tests must remain green** throughout. Phase 5
adds tests; it does not modify Phase 4 behaviour.
