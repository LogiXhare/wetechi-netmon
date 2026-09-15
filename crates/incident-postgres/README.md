# Incident PostgreSQL Adapter

**Status:** Milestone 5B-3(b), in progress. This crate carries:

- the forward-only, checksummed PostgreSQL schema for the incident domain
  (`migrations/`, 5B-2), with a migration smoke test and a compose file
  for an ephemeral local/CI PostgreSQL instance
- `src/staging.rs`, the in-memory `IncidentStore` that one unit-of-work
  call runs against under
  [ADR 0034](../../docs/architecture/decisions/0034-phase5b-persistence-bridge-load-run-flush.md)'s
  load–run–flush. It records what the call changed and refuses to hand
  back changes if the call looked up a key the load step never fetched.
- `src/row.rs`, the pure mapping between an incident and its `incidents`,
  notes, tags and policy-reference rows, and `src/sql.rs`, which inserts,
  version-guard updates and loads one incident. `tests/incident_row_mapping.rs`
  checks the mapping without a database; `tests/incident_row_round_trip.rs`
  runs the SQL against PostgreSQL.

- `src/load.rs`, `src/flush.rs` and `src/service.rs`: the rest of the
  load–run–flush. `IncidentPersistence` runs each unit-of-work entry
  point as one Read Committed transaction: load the working set under
  the ADR's locks, run the domain call over the staging store, flush the
  incidents and their detection-event links, timeline, audit, outbox and
  idempotency rows (`src/history.rs` maps those), then commit.
  `tests/service_round_trip.rs` covers it on PostgreSQL, including
  FU-44's gate that a connection killed mid-flush commits nothing.

- `src/retry.rs`: which failures are transient under ADR 0026, and the
  backoff. The service reruns a call from a fresh load on one, at most
  3 attempts. `tests/retry_and_races.rs` checks the classification
  against real PostgreSQL errors and that two concurrent first
  detections for one target open one incident.

- `src/outbox.rs` and `src/retention.rs` (5B-4). `OutboxConsumer` claims
  outbox rows under ADR 0033's lease-aware `FOR UPDATE SKIP LOCKED`
  query, marks each one published or failed, retries with backoff, and
  dead-letters at the retry limit. `run_retention` purges expired
  idempotency records, published outbox rows after 7 days, reviewed
  dead-letter rows after 90 days, and closed incidents after 24 months.
  It never purges audit or unreviewed dead-letter rows.
  `tests/outbox_and_retention.rs` covers both.
- `src/platform.rs` (5B-5). The outbox consumer, its stats and retention
  are the only cross-tenant paths, and each takes a `PlatformAuthority`.
  Only a context holding `PlatformIncidentAdmin` can produce one
  (ADR 0032, item 7). Every other function is scoped to a tenant.
- `src/fault.rs` (5B-5). Test-only failure injection behind the
  `fault-injection` feature, which only this crate's own dev-dependency
  turns on. `tests/failure_injection.rs` fails a creating ingest at each of
  the eight flush points and proves nothing committed, fails a command the
  same way, and checks that a transient failure is rerun and commits once.
- `tests/outbox_concurrency.rs` (5B-5) runs ADR 0033's outbox concurrency
  and crash list on separate connections. It covers simultaneous
  claimers, a rolled-back claim, and a worker crashing before and after
  its claim commits. It also checks that lease and backoff are measured
  on `transaction_timestamp()`, that `attempts` counts once when two
  holders race, and that the retry limit dead-letters on the same failure
  whatever claims happen in between.

**Not here yet:** a scheduler for the consumer and retention jobs,
connection pool wiring, and any production database connection. See
[ADR 0029](../../docs/architecture/decisions/0029-phase5b-repository-and-unit-of-work-seam.md)
for why this is the crate's real, final placement, and
[FU-42](../../docs/development/follow-ups.md) for the dependency probe it
started as.

The six conditionally-accepted dependencies (`uuid`, `tokio-postgres`,
`deadpool-postgres`, `rustls`, `tokio-postgres-rustls`, `refinery`) remain
declared here at the exact versions their respective ADRs pinned; see
`src/lib.rs`'s `_probe_every_dependency_links` for the Phase 5B-1 probe
this crate started as (all six approved, [dependency-license-matrix.md](../../docs/dependency-license-matrix.md)
rows 32–37).

## Migrations

`migrations/` holds twelve `refinery`-compatible SQL files
(`V1__enable_extensions.sql` through
`V12__policy_reference_order.sql`). V1–V11 follow the dependency order
[phase5-implementation-plan.md](../../docs/development/phase5-implementation-plan.md)'s
5B-2 section fixes: extensions, `incidents`, detection-event links,
timeline, audit, notes/tags/assignments, policy references and number
allocators, idempotency, outbox and dead-letter, the active-incident
partial unique indexes, and the RLS-ready application role (ADR
0032). V12 (5B-3(b)) adds `ref_index` so a load keeps policy references
in the order the domain holds them.

They are embedded into this crate's binary at compile time via
[`refinery::embed_migrations!`] (`src/lib.rs`'s `migrations` module) —
embedded so a deployed binary carries its own schema history with no
separate file-distribution step, while staying file-based and
individually reviewable in this repository (ADR 0024's own follow-up
left "embedded vs. file-based" as an open 5B-2 decision; this is the
resolution).

Design notes, open questions, and where this schema deliberately diverges
from `docs/architecture/incident-persistence.md`'s literal sketch (with
the reasoning for each) are documented inline in the migration files
themselves — start with `V2__incidents.sql`'s header comment.

[`refinery::embed_migrations!`]: https://docs.rs/refinery/0.9.2/refinery/macro.embed_migrations.html

## Running the migrations locally

This project never connects a migration to a real or production
database. `docker-compose.yml` next to this file brings up a throwaway,
loopback-only PostgreSQL 17 instance with no persistent volume:

```sh
docker compose -f crates/incident-postgres/docker-compose.yml up -d --wait

WETECHINETMON_INCIDENT_POSTGRES_TEST_URL="host=127.0.0.1 port=55432 user=wetechinetmon_test password=wetechinetmon_test_only dbname=wetechinetmon_incident_test" \
    cargo test -p wetechinetmon-incident-postgres --test migration_smoke_test

docker compose -f crates/incident-postgres/docker-compose.yml down -v
```

`tests/migration_smoke_test.rs` applies every migration, asserts a
second run is a no-op, and checks the resulting schema shape (every
table exists, the three active-incident partial unique indexes exist,
the durable-identity columns are real `GENERATED ALWAYS AS IDENTITY`
columns, and the `wetechinetmon_app` role exists without `BYPASSRLS`).
Without `WETECHINETMON_INCIDENT_POSTGRES_TEST_URL` set, this test skips
itself with an explanatory message rather than failing — an environment
with no Docker/PostgreSQL available must still be able to run `cargo
test --workspace` cleanly.

**In CI (FU-46, 5B-5):** the `postgres` job in
`.github/workflows/validate.yml` runs this crate's tests once for each
PostgreSQL version
[ADR 0025](../../docs/architecture/decisions/0025-phase5b-postgresql-version-support.md)
supports: 15, 16, 17 and 18. Each run gets an ephemeral
`postgres:<version>-alpine` service container and sets
`WETECHINETMON_INCIDENT_POSTGRES_TEST_URL`. The job fails if any test
prints its skip message, so a skip cannot pass silently there. Every PR
therefore runs every PostgreSQL test against all four versions.

## What this crate does not do

No `IncidentStore` implementation, no connection pool, no HTTP, no CLI,
no notification, no BGP, no mitigation. See
[phase5-implementation-plan.md](../../docs/development/phase5-implementation-plan.md)'s
5B-3 section onward for what comes next.
