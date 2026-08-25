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

- **Claim:** `SELECT outbox_id FROM incident_outbox WHERE status IN
  ('pending','retrying') AND available_at <= transaction_timestamp()
  ORDER BY outbox_id FOR UPDATE SKIP LOCKED LIMIT :batch_size`, then
  mark claimed rows with `locked_at`/`locked_by` in the same transaction.
- **Batch size:** configurable, no production default asserted by this
  planning pass.
- **Lease behavior:** a claimed row not published within a configured
  lease duration is eligible for re-claim by a different consumer — the
  lease timeout, not an assumption of consumer liveness, is what bounds
  "stuck forever."
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
