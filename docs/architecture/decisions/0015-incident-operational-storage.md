# 0015. PostgreSQL Is the Operational Source of Truth for Incidents

Status: Proposed — **blocked on BQ-7**
Date: 2026-08-22
Deciders: Repository owner (pending review)

## Context

Incidents are mutable, transactional, and relational: a state change must
atomically update a row, append a timeline entry, append an audit record,
and enqueue an outbox row. Losing any one of those while keeping the
others produces a record that is wrong in a way nobody can detect later.

The repository already runs ClickHouse for
`wetechinetmon_detection_events` and the Phase 3 traffic tables, so
"reuse what we have" is the obvious first question. The
[master prompt](https://github.com/badshashorif/wetechi-netmon/blob/main/prompts/CLAUDE_MASTER_PROMPT.md) names
PostgreSQL for Phase 5, and this ADR should either confirm that with
reasons or dispute it — not simply defer to it.

## Options Considered

### Option A — ClickHouse for everything

- Pros: no new store, no new dependency, one backup story.
- Cons: ClickHouse is an analytical column store. `ALTER TABLE ... UPDATE`
  is an asynchronous mutation, not a transactional update. There are no
  multi-table transactions, no foreign keys, and no enforced unique
  constraints — so the partial unique index that makes "two open
  incidents for one correlation key" impossible cannot exist. Optimistic
  concurrency would have to be emulated in application code, in a race.
  Every property Phase 5 needs would be hand-built and unreliable.

### Option B — PostgreSQL for operational state, ClickHouse for analytics

- Pros: real transactions across tables; foreign keys; partial unique
  indexes; `SELECT ... FOR UPDATE`; mature migration tooling; JSONB for
  bounded semi-structured fields; Row-Level Security available for
  Phase 8; the outbox pattern is natural. ClickHouse keeps doing what it
  is good at.
- Cons: a second datastore to deploy, secure, back up, and monitor; new
  dependencies; two backup and restore procedures.

### Option C — SQLite

- Pros: no server; trivial single-node deployment; real transactions.
- Cons: single-writer concurrency is a poor fit for a correlator plus an
  API plus operators; no `LISTEN`/`NOTIFY`; no RLS; and it would be
  replaced before multi-node, making it throwaway work.

### Option D — Embedded key-value store

- Pros: no server; fast.
- Cons: every relational property — constraints, joins, indexes,
  transactions across "tables" — hand-built. Reinventing a database
  badly.

## Decision

**Option B.** PostgreSQL is the operational source of truth for
incidents, timeline, notes, assignments, audit, idempotency, outbox, and
dead-letter records. ClickHouse remains authoritative for detection
events and traffic, and additionally receives **immutable incident
analytics events** through the outbox.

**There is exactly one authority for any given fact.** ClickHouse never
holds mutable incident state, and PostgreSQL never becomes an analytics
store. A dual-authoritative design where both hold incident state and
disagree is the specific outcome this decision exists to prevent.

Minimum version: **PostgreSQL 14**, for `JSONB` improvements and mature
partial-index behaviour. RLS is available from 9.5 and is designed for
but not enabled in Phase 5.

Not decided here: the Rust driver (`sqlx` vs `tokio-postgres` vs `diesel`)
and the migration tool. Those belong with the implementation and depend
on **BQ-7**.

## Consequences

**Easier.** Atomic multi-table commits, which is the property the entire
persistence design rests on. Database-enforced invariants instead of
application-enforced conventions. A migration story that already exists.
Phase 8 tenancy has RLS available without a schema change.

**Harder.** A second datastore: deployment, credentials, connection
pooling, backup, restore, monitoring, and a documented upgrade path.
Single-node installation gains a prerequisite. Two restore procedures
that must both be *tested*, per NFR-2 — an untested restore is not a
backup.

**Forecloses.** Little. PostgreSQL is a common baseline. If incident
volume ever outgrew it — which would be surprising, since incidents are
thousands per day, not millions per second — partitioning and read
replicas come long before any rewrite.

**Security.** New credentials to manage, from the environment and never
committed. A new network surface to firewall. In exchange: real
constraints, real transactions, and a path to database-enforced tenant
isolation instead of application-enforced filtering.

**License.** PostgreSQL is under the PostgreSQL Licence, permissive and
compatible with the Apache-2.0 core. The Rust driver must be added to
[dependency-license-matrix.md](../../dependency-license-matrix.md) before
use. `sqlx` and `tokio-postgres` are both MIT/Apache-2.0.

**Operational.** Installation documentation, backup and restore runbooks,
and capacity guidance all need writing before Phase 5 ships — tracked in
the [acceptance criteria](../../development/phase5-acceptance-criteria.md).

## Follow-Up

- [ ] **BQ-7** — owner approves PostgreSQL and its driver.
- [ ] Choose the driver and migration tool at Milestone 5B.
- [ ] Add rows to the dependency licence matrix.
- [ ] Write and **test** backup and restore procedures (NFR-2).
- [ ] Evaluate RLS with Phase 8 (**FU-21**).
