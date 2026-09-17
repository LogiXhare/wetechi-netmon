# 0035. Phase 5C Detection-Event Inbox

Status: **Accepted**
Date: 2026-09-17
Deciders: Repository owner

## Context

[ADR 0012](0012-incident-event-ingestion.md) chose a transactional
outbox in PostgreSQL to carry detection events from the detector to the
incident manager. Delivery is at-least-once, and processing is made
effectively-once by idempotent consumption. Milestone 5C builds it.
Several points were left open:

- **The table.** `incident_outbox` (V9, [ADR 0033](0033-phase5b-transactional-outbox-and-dead-letter.md))
  carries events *out of* the incident domain: Opened, Resolved and so
  on. Its rows are keyed by aggregate and version, and its consumers
  publish. A detection event carries no incident yet. It needs a
  different key (`dedup_key`) and a different end state (processed, with
  an outcome).
- **The detector's side.** The detector is synchronous, and its sink must
  not block ([ADR 0011](0011-incident-domain-boundary.md),
  [ADR 0021](0021-phase5b-async-runtime-boundary.md), `crates/detector/src/sink.rs`).
  So the event cannot be written to PostgreSQL on the tick path.
- **Tenant.** A `DetectionEvent` names its tenant in `target.tenant`,
  which the detector takes from configuration. The ingest call scopes
  every read and write by the tenant it is given and refuses an event
  for another one ([ADR 0032](0032-phase5b-tenant-isolation-and-rls-readiness.md)).
  So the inbox must not let a row's tenant and its event disagree.
- **Failure.** It must be settled what a poison event is, which failures
  are retried, and where a dead-lettered event is reviewed.

## Options Considered

### Option A — A separate inbox table with the same lease mechanics

`detection_event_inbox`: one row per detection event. Rows are unique on
`(tenant_id, dedup_key)` and claimed under ADR 0033's lease query. A row
ends `processed`, recording the ingest outcome, or `dead_letter`.

- Pros: each table has one meaning. The claim, lease, backoff and
  dead-letter code already proven for the outbox (#38) is reused. An
  enqueue is idempotent at the database. The recorded outcome makes
  replay auditable.
- Cons: one more table, with its own retention.

### Option B — Reuse `incident_outbox` with an `aggregate_type` of `detection_event`

- Pros: no new table.
- Cons: `aggregate_id` and `aggregate_version` have no meaning for a
  detection event. Every consumer of the outbox would have to filter out
  rows it must never publish. Retention and depth alerts would mix inbound
  and outbound work. A misconfigured publisher could forward raw
  detection data.

### Option C — Write synchronously from the detector sink

- Pros: no loss window on the detector side.
- Cons: puts a database round trip on the detection tick, which ADR 0011
  and the sink contract forbid.

## Decision

**Option A.**

### Table (migration V14)

`detection_event_inbox`:

- `inbox_id`: identity primary key, which also gives claim order.
- The event's identity: `tenant_id`, `dedup_key`, `detection_id`,
  `event_id` and `schema_version`.
- `payload`: the event as JSONB.
- Queue state, with the same meaning and checks as `incident_outbox`:
  `status` (`pending`, `retrying`, `processed`, `dead_letter`),
  `attempts`, `available_at`, `locked_at`, `locked_by`, `last_error`.
- The result: `outcome`, the ingest outcome kind, and `processed_at`.
- `created_at`.
- **Constraints:** `UNIQUE (tenant_id, dedup_key)`. A processed row must
  have an outcome and a processing time.
- **Grants:** granted to `wetechinetmon_app` like the other tables.

### Producer

- **Enqueueing.** `enqueue` inserts a batch in one statement with
  `ON CONFLICT (tenant_id, dedup_key) DO NOTHING`. It returns how many
  rows were new, so a re-sent batch is harmless and counted.
- **Tenant.** The producer is configured with one tenant, and `enqueue`
  takes it. If any event in the batch targets a different tenant,
  `enqueue` refuses the whole batch before writing anything
  (`TenantMismatch`). A row's `tenant_id` therefore always matches its
  event.
- **Detector side.** The sink accepts events into a bounded in-memory
  queue and never blocks. A separate async task drains the queue in
  batches.
  - When the queue is full, the event is dropped and counted.
  - When a write fails, the batch is retried with backoff.
- **Loss window, stated plainly.** An event is durable once its batch
  commits. An event still in the in-memory queue when the detector
  process dies is lost.
  - A detection that is still active is re-detected on restart, and the
    domain links it to the open incident: "detector restart produces one
    incident, not two".
  - An `Ended` event lost this way is covered by the staleness sweep.
  - ADR 0012's guarantee is about the *incident manager* restarting.
    Inbox rows survive that.

### Worker

- **Claiming.** The worker claims a batch with the ADR 0033 lease query
  over `detection_event_inbox`. Claiming is cross-tenant, so building a
  worker takes a `PlatformAuthority`.
- **Processing.**
  - Each row is ingested as its own `ingest_detection_event` call, in
    `inbox_id` order.
  - Each call runs under `AuthorizationContext::correlator(tenant_id)`
    for the row's tenant.
  - The row is then marked `processed` with the outcome.
- **Crash between commit and mark.** If the worker crashes after the
  ingest commits but before the row is marked, the lease expires and the
  row is claimed again. The unique link row makes the second ingest a
  `Duplicate`, so the event is processed once in effect.
- **Poison event.** A payload that does not deserialize is dead-lettered
  at once, without retries. It is never guessed at (ADR 0012).
- **Newer schema.** An event that does deserialize but carries a newer
  `schema_version` reaches the domain. The domain's schema gate records
  it as `Quarantined`, and the row is marked processed with that
  outcome.
- **Other failures.**
  - Covered: a domain error, or a persistence error the service's own
    ADR 0026 reruns did not clear.
  - Handling: the failure is counted and the row is retried with backoff.
    At the retry limit it is dead-lettered.
- **Dead letter.** Rows are copied into the existing `incident_dead_letter`
  with `aggregate_type = 'detection_event'`, `aggregate_id = dedup_key`,
  and `event_type` set to the event kind. There is one review queue for
  both directions.
- **Ordering.** Rows are processed in `inbox_id` order within a batch.
  - No order across workers is promised.
  - A late or out-of-order event is decided by the domain on
    `observed_at_ms`, as tested in #41. It is never decided by queue
    position.
- **Clocks.** Every lease and backoff comparison uses
  `transaction_timestamp()`, as in the outbox.

### Retention

`processed` rows are purged after 7 days, the same as published outbox
rows. `dead_letter` rows stay; the reviewed copy in
`incident_dead_letter` follows its own 90-day rule.

## Consequences

**Easier.** Replaying an event is an `UPDATE` back to `pending`, and the
domain's idempotency makes that safe. The outcome of every event can be
seen with SQL. The lease code is shared with the outbox.

**Harder.** Two queues to watch: inbox depth becomes its own alert
alongside outbox depth. The detector-side queue is a real, bounded loss
window, and its drop counter must be monitored.

**Forecloses.** Nothing beyond ADR 0012. A NATS producer later replaces
the enqueue call; the worker's idempotent consumption stays.

**Security.** The payload holds target addresses, so it is tenant-scoped
and retained no longer than stated. A dead-lettered payload is data for
review and is never interpreted.

**License.** No new dependency.

## Follow-Up

- [x] V14 migration and the inbox enqueue, claim, mark and dead-letter
      functions, with PostgreSQL tests.
- [x] Detector-side sink with a bounded queue and a drain task.
- [ ] Correlation worker loop and graceful shutdown drain (ADR 0012).
- [ ] Inbox depth and detector-queue drop metrics (5C metrics).
