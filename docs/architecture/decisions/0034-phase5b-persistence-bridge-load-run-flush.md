# 0034. Phase 5B Persistence Bridge: Load, Run, Flush

Status: **Accepted**
Date: 2026-09-15
Deciders: Repository owner

## Context

Milestone 5B-3 implements the PostgreSQL side of the `IncidentStore`
seam that 5B-0 extracted ([ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md)).
Three earlier decisions constrain how:

- [ADR 0021](0021-phase5b-async-runtime-boundary.md): `crates/incident`
  stays synchronous, with no Tokio or PostgreSQL type in its public API.
  It left the sync/async bridging mechanism to 5B-3.
- [FU-44](../../development/follow-ups.md): every store call belonging to
  one logical mutation must commit or roll back together.
- [ADR 0026](0026-phase5b-transaction-isolation.md): Read Committed,
  explicit locks and constraints, bounded retries.

The seam's shape settles most of the question. `IncidentStore` returns
borrows: `get` gives `Option<&Incident>`, `get_mut` gives
`Option<&mut Incident>`, `timeline()` gives `&[TimelineEntry]`, and
`idempotency()` gives `&IdempotencyStore` (verified against
`crates/incident/src/store.rs`). A borrow of a row that lives in a remote
database cannot be returned from a synchronous method. Whatever the
bridge is, the domain call has to run against rows already held in
memory.

## Options Considered

### Option A — Load, run, flush

One transaction per unit-of-work entry-point call. Load every row the
call can read, locking where ADR 0026 requires it. Run the unchanged
synchronous domain logic against an in-memory staging store holding
those rows. Write back what changed, in the same transaction, then
commit.

- Pros: the domain crate and its trait stay as they are; FU-44 holds by
  construction, because the database sees nothing until one commit; this
  is the "async I/O before and after a synchronous domain call" pattern
  ADR 0021 already named; round trips are bounded per call rather than
  per trait method.
- Cons: the load step must fetch everything the call might read (see
  Consequences for how a miss is made to fail closed); the adapter needs
  a small change-tracking layer.

### Option B — Change the trait to an owned, per-call shape

Replace the borrowing methods with owned ones (`load(id) -> Incident`,
`save(&Incident)`, `timeline_for(id)`).

- Pros: each method maps directly onto SQL.
- Cons: reopens the seam 5B-0 shipped and the owner chose; revisits
  about 93 call sites in code already certified by review; one round trip
  per call; it still needs a transaction held across every call to satisfy
  FU-44, so it does not remove the hard part. Rejected.

### Option C — Block on SQL inside each trait method

Implement the current trait by running each method's query with a
blocking call from a `spawn_blocking` thread.

- Pros: no staging layer.
- Cons: it cannot return the borrows the trait requires without caching
  rows in memory anyway, which is Option A done piecemeal; it holds a
  pool thread and a connection for the whole domain call; it multiplies
  round trips. Rejected.

## Decision

**Option A.** Every call to an `IncidentUnitOfWork` entry point
(`ingest_detection_event`, `handle_command`, `enter_recovering`,
`confirm_recovery_if_due`, `abort_recovery`,
`attempt_automatic_closure`) becomes one load–run–flush transaction.

### 1. Load

`BEGIN` at Read Committed, then fetch the working set for that entry
point, always filtered by `tenant_id` ([ADR 0032](0032-phase5b-tenant-isolation-and-rls-readiness.md)):

| Entry point | Rows loaded | Lock |
|---|---|---|
| `handle_command`, recovery, and closure calls | the incident by id; its idempotency record, if the command carries a key | `FOR UPDATE` on the incident row |
| `ingest_detection_event` | the dedup record for `(tenant, dedup_key)`; the active incident for the correlation key, if any; otherwise the reopen candidate for that key, if any | `FOR UPDATE` on whichever incident row is loaded |
| `ingest_detection_event`, when no active incident exists | additionally, the tenant's `incident_number_allocators` row (inserted first if absent) | `FOR UPDATE` on that row |

The allocator lock is taken only when a new incident could be created, so
linking events to an open incident, the common case, never waits on it.
Holding the lock until commit keeps numbers gap-free, which is stricter
than `NumberAllocator`'s contract requires ("never the same number
twice").

### 2. Run

Build a per-call `IncidentUnitOfWork` over a staging store (living in
`crates/incident-postgres`) that holds the loaded rows and records every
change made through `insert`, `get_mut`, `append_*`, `dedup_record`, and
the idempotency store. The domain call is synchronous, in memory, and
performs no I/O, so it runs inline on the async task between two awaits,
with no `spawn_blocking` and no await inside it.

The staging store records every lookup of a key the load step did not
fetch. A call that made such a lookup is **not flushed**: the adapter
rolls back and returns an internal error. A load step that missed a row
fails closed instead of letting the domain decide on a false "absent".

### 3. Flush

In the same transaction:

- updated incidents: `UPDATE … WHERE tenant_id = $t AND incident_id = $id
  AND version = $loaded_version`. Zero rows affected is a conflict; roll
  back. This backs up the lock and the domain's own `expected_version`
  check.
- new incidents, detection-event links, timeline, audit, and outbox rows,
  and idempotency records: `INSERT`. The database assigns
  `timeline_id`/`audit_id`/`outbox_id` as identity columns
  ([ADR 0027](0027-phase5b-durable-record-identity.md)). The domain's in-memory sequence
  counters are local to the call and are not persisted; durable order is
  the identity column's.
- the allocator's `next_value`, only if a number was consumed.

Then `COMMIT`.

### 4. Retry

A retryable failure (ADR 0026's matrix: `23505` on an active-incident
partial index, `40001`, `40P01`) rolls back and reruns the **whole**
load–run–flush from a fresh load, at most 3 attempts with backoff and
jitter. A decision is never flushed against state it was not made from.
An operator version conflict is returned, not retried, per
[ADR 0016](0016-incident-concurrency-and-idempotency.md).

### Domain-crate changes this requires

All dependency-free, all within ADR 0021's boundary:

- a way to build `IncidentUnitOfWork` over a caller-supplied
  `Box<dyn IncidentStore>` (today `new` always creates an
  `InMemoryIncidentStore`), and a way to recover that store after the call;
- read-only iteration over `IdempotencyStore`'s records, so the adapter
  can find new ones;
- no change to any command's behavior. The existing tests remain the
  regression gate.

### Delivery

5B-3 lands in three reviewable steps: **(a)** the domain-crate changes
above plus the staging store and its change tracking, with no SQL;
**(b)** row mapping and the load/flush SQL, with integration tests
against the CI PostgreSQL service ([FU-46](../../development/follow-ups.md));
**(c)** retry and conflict classification per ADR 0026.

## Consequences

**Easier.** FU-44's partial-write risk is closed by the transaction
itself, not by discipline in each method. The domain crate, its 595
tests, and the seam the owner chose all stay as they are. Each call's
database cost is a bounded load plus a bounded flush.

**Harder.** The load table above is now a contract that has to be kept
accurate. A new domain read path added later needs a matching load-step
entry; the fail-closed rule turns a forgotten entry into a visible
error, not a silent wrong decision. The database constraints (the
partial unique indexes, the dedup unique key) remain the last line of
defense. `IncidentUnitOfWork::timeline()`, `audit()`, and `outbox()` see
only the current call's appends in this mode, so read-side APIs must
query the database directly, not go through a unit of work.

**Forecloses.** Nothing permanent. Option B stays available through a
future ADR if the load table becomes unmanageable in practice.

**Security.** Every load and flush statement filters on `tenant_id`.
Bounded retries keep a database outage from turning into a retry storm
(ADR 0026).

**License.** N/A. Only dependencies already approved in ADRs 0020, 0022,
and 0023 are used.

## Follow-Up

- [ ] 5B-3(a): the store-injecting constructor and store recovery on
      `IncidentUnitOfWork`, idempotency record iteration, and the staging
      store with change tracking and unloaded-key detection. Unit-tested,
      no SQL.
- [ ] 5B-3(b): integration tests for FU-44's acceptance gate (a
      connection killed mid-flush commits nothing), for fail-closed
      unloaded-key access, and for the zero-row version conflict.
- [ ] 5B-3(c): a `23505` on an active-incident index retries into the
      link path, and retries stop after 3 attempts.
- [ ] ADR 0021's regression test: no `tokio` or `tokio_postgres` type in
      `crates/incident`'s public API.
