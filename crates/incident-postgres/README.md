# Incident PostgreSQL Adapter

**Status:** Milestone 5B-2 — schema and migrations only. This crate now
carries the forward-only, checksummed PostgreSQL schema for the incident
domain (`migrations/`), a migration smoke test, and a compose file for an
ephemeral local/CI PostgreSQL instance. **It still has no `IncidentStore`
implementation, no connection pool wiring, and no production database
connection** — see [FU-42](../../docs/development/follow-ups.md) for the
dependency-probe acceptance gate this crate previously existed only to
satisfy, and [ADR 0029](../../docs/architecture/decisions/0029-phase5b-repository-and-unit-of-work-seam.md)
for why this is the crate's real, final placement rather than a
throwaway. Milestone 5B-3 implements the actual repository code against
the schema this milestone builds.

The six conditionally-accepted dependencies (`uuid`, `tokio-postgres`,
`deadpool-postgres`, `rustls`, `tokio-postgres-rustls`, `refinery`) remain
declared here at the exact versions their respective ADRs pinned; see
`src/lib.rs`'s `_probe_every_dependency_links` for the Phase 5B-1 probe
this crate started as (all six approved, [dependency-license-matrix.md](../../docs/dependency-license-matrix.md)
rows 32–37).

## Migrations

`migrations/` holds eleven `refinery`-compatible SQL files
(`V1__enable_extensions.sql` through
`V11__rls_ready_roles.sql`), in the dependency order
[phase5-implementation-plan.md](../../docs/development/phase5-implementation-plan.md)'s
5B-2 section fixes: extensions, `incidents`, detection-event links,
timeline, audit, notes/tags/assignments, policy references and number
allocators, idempotency, outbox and dead-letter, the active-incident
partial unique indexes, and finally the RLS-ready application role (ADR
0032).

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

**Open item, not yet resolved:** this repository's `.github/workflows/validate.yml`
`rust` job does not currently provision a PostgreSQL service container,
so the smoke test above does not yet run in CI — only locally, with
Docker, by whoever sets the environment variable. Wiring a `postgres:`
service into that job (or a dedicated workflow) is follow-up work, not
done as part of this migration-authoring milestone.

## What this crate does not do

No `IncidentStore` implementation, no connection pool, no HTTP, no CLI,
no notification, no BGP, no mitigation. See
[phase5-implementation-plan.md](../../docs/development/phase5-implementation-plan.md)'s
5B-3 section onward for what comes next.
