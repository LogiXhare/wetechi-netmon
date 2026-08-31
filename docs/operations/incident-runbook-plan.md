# Incident Management Runbook — Plan

Status: **Planning only.** Part of the
[Phase 5 plan](../architecture/phase5-incident-management-plan.md). The
procedures below describe a system that does not exist yet; they are here
so the operational surface is designed alongside the code rather than
written after the first outage.

This is a runbook for operating **the incident manager itself** — not for
handling network incidents, which is what the product is for.

## Health

| Signal | Healthy |
|---|---|
| `wetechinetmon_incident_outbox_pending` | Low and draining |
| `wetechinetmon_incident_dead_letter_pending` | **Zero** |
| `wetechinetmon_incident_audit_failures_total` | **Never increments** |
| `wetechinetmon_incident_repository_failures_total` | Flat |
| `wetechinetmon_incident_events_ingested_total` | Tracks detector event volume |
| `wetechinetmon_incidents_active` | Plausible for current traffic |
| `wetechinetmon_incident_oldest_unacknowledged_seconds` | Below the SLA the team sets |

## Outbox backlog

**Alert:** `outbox_pending > 1000` for 10 minutes.

Backlog means events are arriving faster than correlation processes them,
or the consumer has stalled. Detection is unaffected — events are safe in
PostgreSQL — but incidents are lagging reality, which is exactly the
wrong time for that.

1. Is the consumer alive? Check the process and its log.
2. Is PostgreSQL healthy? Check `repository_failures_total`, connection
   pool saturation, and `pg_stat_activity` for long-running queries.
3. Is one correlation key hot? A flapping detection can dominate the
   queue. Group pending rows by `aggregate_id`.
4. Is the backlog draining at all? Compare depth over five minutes.

If a single poison event is stalling the queue, it should already have
gone to dead-letter after `INCIDENT_INGEST_MAX_ATTEMPTS`. If it has not,
the retry cap is misconfigured — that is the bug, not the event.

**Do not** clear the outbox to make the alert stop. Those are unprocessed
detection events, and deleting them loses incidents silently.

## Dead-letter review

**Alert:** any row at all.

A dead-letter row is a detection event that could not become an incident.
Each one is potentially a missed attack, which is why the threshold is
zero rather than a number.

1. Read the row: raw payload, failure reason, attempt count.
2. Classify: malformed input, unsupported schema version, a bug, or a
   transient failure that outlasted the retry cap.
3. For a bug: fix, then replay by resetting the row to `pending`. Replay
   is safe — idempotent consumption means a duplicate cannot create a
   second incident.
4. For malformed input: identify the producer. A malformed event from the
   detector is a Phase 4 bug and should be reported as one.
5. For an unsupported schema version: the incident manager is older than
   the detector. Upgrade it; do not widen the schema check to make the
   error go away.

**Never bulk-delete unreviewed dead-letter rows.**

## Audit write failure

**Alert:** any increment.

An audit failure rolls back the whole transaction, so the mutation did
not happen. The operator's command failed and they were told so. Nothing
is silently unaudited — that is the design working — but it means
commands are failing.

1. Check PostgreSQL: disk space first, then permissions on
   `incident_audit`, then constraint violations in the log.
2. Confirm nothing is silently succeeding: the count of mutations should
   equal the count of audit records for the period.
3. Do not disable audit to restore service. An unauditable incident
   system is not a degraded system, it is a non-compliant one.

## PostgreSQL unavailable

The API returns `503` and ingestion stops consuming without
acknowledging. No partial state is written — this is the fail-closed
design.

1. Restore PostgreSQL.
2. Ingestion resumes from `pending` automatically; no manual replay.
3. Expect a backlog burst; watch it drain.
4. Verify no partial incidents exist: every incident should have at least
   one timeline entry and one audit record. A query for incidents lacking
   either should return nothing, ever.

## ClickHouse unavailable

Operational state is unaffected — this separation is the reason the
outbox exists. Analytics lag; incidents keep working.

1. Confirm operational health is unaffected: `repository_failures_total`
   flat, API serving.
2. Restore ClickHouse.
3. Analytics events publish from the outbox; no data is lost unless the
   outbox retention expires first, so extend
   `INCIDENT_RETENTION_OUTBOX_PUBLISHED_DAYS` if the outage is long.

## Limit reached

**Alert:** `limit_reached_total` increments.

| Limit | Meaning | Action |
|---|---|---|
| Open incidents per tenant | New incidents refused | Investigate why so many are open — usually unclosed incidents, not real attacks |
| Events per incident | Evidence links stopped; counting continues | Usually a long attack; no action unless evidence is needed |
| Timeline entries | Approaching unmanageable | Investigate flapping detection; tune the policy's hysteresis |
| Notes | Rejected | Genuine operator load; consider a linked external ticket |

The open-incident limit refuses rather than evicts. If it is reached,
incidents are not being closed — check for a stuck auto-close or a team
that has stopped triaging.

## Conflict storm

A sustained rise in `command_conflicts_total` means either an integration
retrying incorrectly — retrying a `409` without re-reading, which will
never succeed — or two operators repeatedly colliding on one incident.
Group by `command` and by actor to tell them apart.

## Ingestion stalled

Detector events continue but `events_ingested_total` is flat. The
consumer is alive but not making progress. Check for a long-running
transaction blocking the outbox read, a connection-pool deadlock, or a
consumer waiting on a lock. This is distinct from backlog: backlog means
slow, stalled means stopped.

## Backup and restore

Both stores need testing, not assuming — NFR-2. **Technical design
targets (2026-08-24 Phase 5B planning), not an SLA commitment or legal
requirement:** RPO 15 minutes, RTO 4 hours — see
[phase5b-postgresql-persistence-plan.md](../architecture/phase5b-postgresql-persistence-plan.md).

- **PostgreSQL** is the source of truth. Point-in-time recovery
  (continuous WAL archiving, sized against the 15-minute RPO target)
  plus logical backups (`pg_dump`); restore tested on a schedule, not
  only after a failure. A backup is taken and verified **before every
  migration** (see Upgrades below).
- **ClickHouse** analytics are reconstructible from the outbox only
  within its retention. Beyond that, incident analytics history is lost
  while operational history survives. That trade-off should be stated in
  the installation documentation rather than discovered.
- **A restore that has never been tested is not a backup.** This is
  called out because it is the single most common way an operations plan
  turns out to be fiction.
- **Outbox rows in flight at backup time** are recovered from `pending`
  on restore, the same behaviour as a service restart — no special
  restore-time handling needed, per
  [incident-persistence.md](../architecture/incident-persistence.md)'s
  existing failure-behaviour table.
- **Tenant-scoped restore is not supported in Phase 5B** — restore is a
  full-database operation; a single-tenant export/restore path is a
  later design, not assumed to exist (see
  [ADR 0032](../architecture/decisions/0032-phase5b-tenant-isolation-and-rls-readiness.md)).

## Connection pool health

**Alert:** `pool_wait` climbing, or `pool_timeout` incrementing.

A pool near exhaustion degrades every operator command and every
detection-event ingestion at once — it is a whole-service brownout, not
a single-feature failure.

1. Is `pool_in_use` near `max_size`? Check for a long-running query or
   transaction holding connections open longer than expected.
2. Is `pool_idle` unexpectedly low even under light load? A leaked
   connection (acquired, never released) is the usual cause — check for
   an error path that returns early without releasing.
3. **Do not** raise `max_size` reflexively — a larger pool against an
   under-provisioned PostgreSQL instance moves the bottleneck to the
   database's own `max_connections`, it does not remove it.

A sustained pool-exhaustion condition should surface as `503` to
callers, per the bounded-timeout requirement in
[ADR 0022](../architecture/decisions/0022-phase5b-connection-pool.md) —
never as an indefinite hang.

## Migration locking

**Alert:** a deployment startup that hangs at the migration step.

`refinery`'s migration-runner locking (or an explicit PostgreSQL
advisory lock wrapping it, per
[ADR 0024](../architecture/decisions/0024-phase5b-migration-framework.md))
prevents two service instances from applying the same migration twice
during a rolling deployment. A hang here usually means a prior deploy's
instance crashed mid-migration and still holds the lock.

1. Confirm no other instance is genuinely mid-migration before clearing
   a lock by hand.
2. **Never** manually mark a migration as applied without confirming its
   statements actually ran — a checksummed migration system exists
   specifically so this class of manual intervention is rare and
   auditable, not routine.

## Upgrades

1. Back up PostgreSQL. Verify the backup.
2. Apply migrations — **forward-only**; no down-migrations in
   production. Rollback is roll-forward (a corrective migration) or
   restore-from-backup, never an assumed automatic reverse of a
   destructive change (see
   [ADR 0024](../architecture/decisions/0024-phase5b-migration-framework.md)).
3. Start the new version; confirm the incident schema version.
4. Watch `events_rejected_total` for schema-version quarantines, which
   indicate a detector newer than the incident manager.

Order matters: upgrade the incident manager **before** the detector, so a
newer event schema never arrives at an older consumer.

## What operators cannot do here

- Cannot mitigate. Phase 5 has no such capability.
- Cannot notify. No delivery exists.
- Cannot edit history. Timeline and audit are append-only; notes are
  superseded, never overwritten.
- Cannot delete audit for one tenant without a separately authorized,
  separately audited operation.
