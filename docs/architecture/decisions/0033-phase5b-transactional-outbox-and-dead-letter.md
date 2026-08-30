# 0033. Phase 5B Transactional Outbox and Dead-Letter Design

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

[incident-persistence.md](../incident-persistence.md) already sketches
`incident_outbox` and `incident_dead_letter` and states "Phase 5 writes
event types those later phases will consume and publishes only the
analytics ones" — the outbox pattern itself is approved. This ADR fixes
the claim boundary and claim mechanics precisely, since the pattern's
value depends entirely on the claim step being race-free without adding
a stronger isolation level than [ADR 0026](0026-phase5b-transaction-isolation.md)
already decided against.

## Options Considered

### Option A — `SELECT ... FOR UPDATE SKIP LOCKED` batch claim

- Pros: multiple consumer instances can claim disjoint batches
  concurrently at Read Committed, with no isolation-level escalation and
  no explicit application-level distributed lock; a locked-but-uncommitted
  row is simply skipped by a concurrent claimer rather than blocking it,
  which is exactly the behavior needed for "many workers, no single
  point of serialization."
- Cons: a claimed-but-never-released row (a crashed consumer) needs a
  lease-expiry mechanism, since `FOR UPDATE SKIP LOCKED`'s lock is held
  only for the claiming transaction's lifetime — if that transaction
  never commits or rolls back cleanly (process killed mid-claim), the
  lock releases with the connection, but if it commits a "locked" state
  without actually publishing, a separate lease-timeout column is
  needed to reclaim it.

### Option B — A single dedicated consumer, no concurrent claiming

- Pros: no claim race to solve at all.
- Cons: no horizontal scaling path for the outbox consumer, and a single
  point of failure for the outbox → ClickHouse (and later, notification)
  path — contradicts the operational posture the runbook plan already
  assumes (multiple instances, health-checked).

### Option C — External queue (Kafka/NATS/Redpanda) instead of a PostgreSQL-backed outbox

- Pros: purpose-built for high-throughput message delivery.
- Cons: **explicitly out of scope for Phase 5B** per this task's own
  instruction ("do not add Kafka, NATS, or Redpanda during Phase 5B
  unless separately approved"). Also reopens R3 in
  [risk-register.md](../../risk-register.md) (Redpanda BSL licensing) for
  no demonstrated need — the outbox's actual current consumer (the
  ClickHouse analytics exporter) does not need a separate broker's
  throughput. Rejected for this phase.

## Decision

**Option A.** `incident_outbox` claim mechanics:

- **Claim — corrected 2026-08-30.** The original wording selected rows
  by `status`/`available_at` alone and recorded `locked_at`/`locked_by`
  without the claim query itself accounting for the lease, so a claiming
  transaction's commit released the row lock with nothing else changed —
  a second consumer's identical query would re-select the same row
  immediately, on every batch, not only after lease expiry. The
  corrected claim is lease-aware in the predicate itself:

  ```sql
  SELECT outbox_id FROM incident_outbox
  WHERE status IN ('pending', 'retrying')
    AND available_at <= transaction_timestamp()
    AND (
      locked_at IS NULL
      OR locked_at + :lease_interval <= transaction_timestamp()
    )
  ORDER BY outbox_id
  FOR UPDATE SKIP LOCKED
  LIMIT :batch_size
  ```

  In the same transaction, each claimed row is updated with
  `locked_at = transaction_timestamp()` and `locked_by = :consumer_id`.
  `status` is **not** advanced to a separate "processing" value —
  `locked_at` compared against the configured `:lease_interval` is the
  sole lease marker, so a fresh, unleased row (`locked_at IS NULL`) and
  an expired-lease row (`locked_at` older than the interval) are both
  captured by the same predicate without a third status value to keep in
  sync. A successful publish sets `status = 'published'`,
  `published_at = transaction_timestamp()`, and clears `locked_at`/
  `locked_by`. A failed publish clears `locked_at`/`locked_by`,
  increments `attempts`, sets `status = 'retrying'`, and advances
  `available_at` by the backoff interval — so a retrying row is not
  immediately re-claimed as if its lease had simply expired; the backoff
  predicate (`available_at <= transaction_timestamp()`) and the lease
  predicate both gate re-selection, independently.
- **Batch size:** configurable, no production default asserted by this
  planning pass.
- **Lease behavior:** a claimed row not published (nor moved to
  `retrying` or `dead_letter`) within `:lease_interval` is eligible for
  re-claim by a different consumer, per the corrected predicate above —
  the lease timeout, not an assumption of consumer liveness, is what
  bounds "stuck forever." An active lease
  (`locked_at + :lease_interval > transaction_timestamp()`) is not
  reclaimable by any concurrent claimer. A transaction that claims a row
  and then rolls back (crash, error, forced abort) leaves the row
  exactly as it was before the claim — `locked_at`/`locked_by` were
  never durably written, so the row is immediately claimable again
  without waiting out a lease it never actually held.
- **Retry:** exponential backoff with jitter (matching
  [incident-persistence.md](../incident-persistence.md)'s existing
  failure-behavior table), `attempts` incremented per failed publish,
  `last_error` recorded.
- **Retry limit and dead-letter transition:** after a configured maximum
  attempt count, the row moves to `incident_dead_letter` rather than
  retrying indefinitely — matching this ADR's own retry-bounding
  principle from [ADR 0026](0026-phase5b-transaction-isolation.md)
  ("no infinite retry loop... a poison event... must become visible
  instead").
- **Duplicate-consumer behavior:** the outbox's own event carries enough
  identity (`aggregate_id`, `aggregate_version`, `event_type`) for a
  downstream consumer (the ClickHouse exporter today) to de-duplicate on
  its own side if a lease expires and a second consumer re-publishes an
  event the first consumer actually did deliver before crashing — an
  at-least-once contract, not exactly-once, consistent with
  [incident-persistence.md](../incident-persistence.md)'s existing
  framing of the whole ingestion path.
- **Cleanup:** published rows retained 7 days then purged (existing
  retention table); dead-letter rows retained 90 days and **never
  auto-purged while unreviewed** (existing rule, restated here because
  the claim mechanics above are new).
- **Observability:** `outbox_pending`, `outbox_retries`,
  `dead_letter_count` as bounded metrics (already listed in the
  observability plan).
- **Phase 5B persists the outbox without implementing external
  notification delivery.** Producing an event nobody yet consumes beyond
  the analytics exporter is the seam [ADR 0011](0011-incident-domain-boundary.md)
  requires, not an implementation of notification.

## Consequences

**Easier.** Multiple consumer instances can process the outbox
concurrently without an application-level distributed lock or a stronger
isolation level than Read Committed.

**Harder.** The lease-expiry mechanism is one more piece of state
(`locked_at` interpreted against a configured lease duration) to get
right, and it needs its own integration test (a consumer crashing
mid-claim must not lose or duplicate-forever the event).

**Forecloses.** Nothing — an external queue remains adoptable later if
volume or consumer diversity (multiple downstream systems, not just
ClickHouse) demonstrates a real need, per Option C's note.

**Security.** None distinct from the transaction-isolation ADR.

**License.** N/A — no new dependency, `SKIP LOCKED` is native PostgreSQL.

## Follow-Up

- [ ] Define the concrete lease-duration default at Phase 5B-4
      implementation, informed by (not asserted before) the
      performance-test plan.
- [ ] Add the stale-lease-reclaim and duplicate-consumer-idempotency
      integration tests at Phase 5B-5.
- [ ] Confirm the ClickHouse exporter consumer (the only Phase 5B
      consumer) tolerates at-least-once delivery, per its existing
      design.
- [ ] **Added 2026-08-30, from the lease-predicate correction above** —
      the following integration tests at Phase 5B-5, alongside the two
      already listed:
  - [ ] A second worker cannot immediately reclaim a row whose lease is
        still active.
  - [ ] A row becomes eligible for reclaim only after its lease
        genuinely expires (`locked_at + lease_interval <=
        transaction_timestamp()`), not merely after the first
        transaction commits.
  - [ ] A crashed worker's claimed-but-never-published row is
        eventually reclaimed by a different consumer.
  - [ ] A successfully published row (`status = 'published'`) is never
        reclaimed by any predicate.
  - [ ] A `retrying` row does not become available before its
        backoff-advanced `available_at` elapses, independent of the
        lease predicate.
  - [ ] Two simultaneous claimers against the same pending batch receive
        disjoint row sets (`FOR UPDATE SKIP LOCKED` holds under real
        concurrency, not only sequential test calls).
  - [ ] A claiming transaction that rolls back leaves the row
        immediately claimable — `locked_at`/`locked_by` were never
        durably committed.
  - [ ] The lease and backoff clocks are both PostgreSQL
        `transaction_timestamp()`, never client-supplied time.
  - [ ] `attempts` increments exactly once per genuine claim-and-fail
        cycle, not once per predicate evaluation.
  - [ ] The dead-letter transition at the configured retry limit is
        deterministic and does not depend on claim timing.
