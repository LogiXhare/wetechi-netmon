# Phase 5 Implementation Plan

Status: **Planning only.** No milestone below has started. Part of the
[Phase 5 plan](../architecture/phase5-incident-management-plan.md).

Six milestones, each independently reviewable and independently
mergeable, each ending green. The ordering has one governing property:
**everything testable without a database is built and proven before the
database arrives.** Milestone 5A produces a complete, exhaustively tested
domain with in-memory repositories, so that when PostgreSQL lands in 5B
the only new risk is persistence rather than persistence *and* domain
logic at once.

## Before any milestone starts

**Resolved 2026-08-22 — 5A and 5B are unblocked:**

- **BQ-5** incident identity → **UUIDv7**, conditional on a dependency and
  licence review. Determines the primary key type; 5A may proceed.
- **BQ-6** FR-5.1 deviation → mitigation states **excluded**; seven-state
  lifecycle; suppression is an attribute. Determines the state set; 5A may
  proceed.
- **BQ-7** dependency approval → **approved architecturally**. 5B may
  proceed once [ADR 0018](../architecture/decisions/0018-phase5-dependency-selection.md)
  selects crates with verified evidence (**FU-25**, **FU-26**).

- **BQ-8** critical closure → **manual by default**. 5A implements the
  guard that refuses an automatic `Resolved` → `Closed` for `critical`;
  5B stores the override; 5D exposes effective-configuration diagnostics.
- **BQ-9** reopen window → **15 minutes, inclusive boundary**, range
  0–24 h. 5A implements the boundary comparison and its tests.

**No Phase 5 planning decision remains outstanding.** The only thing
standing between here and 5B is crate selection under
[ADR 0018](../architecture/decisions/0018-phase5-dependency-selection.md),
which is implementation work with verification requirements rather than
an open architectural question (**FU-25**, **FU-26**).

## Milestone 5A — Domain and state machine

**No database. No API. No dependencies beyond the workspace.**

New crate `wetechinetmon-incident`. Domain types from the
[domain model](../architecture/incident-domain-model.md); the state
machine with a guard refusing illegal edges; correlation key construction
and canonicalisation; category derivation; the `Clock` seam reused from
the detector; repository *traits* with in-memory implementations;
`CorrelationStrategy` and `AssignmentPolicy` seams from ADR 0017.

Tests: all domain tests, all state-machine tests, and property tests
1–13 from the [testing plan](../architecture/incident-testing-plan.md).

**Exit:** every legal and illegal transition tested; correlation
order-independence proven; no dependency added; existing 403 tests still
green.

**Reviewable because** it is pure logic with no infrastructure, so review
attention goes to the rules rather than to wiring.

**5A is database-independent and dependency-free.** It may implement
incident domain types, the seven-state lifecycle, correlation, reopen
behaviour, the suppression attribute, severity and priority, assignment
abstractions, timeline and audit types, in-memory repositories,
idempotency and outbox *abstractions*, and unit and property tests.

It must **not** add a PostgreSQL driver, an HTTP framework, migrations, a
REST server, notification delivery, mitigation, BGP, or FlowSpec. If 5A
needs any of those, the domain boundary in
[ADR 0011](../architecture/decisions/0011-incident-domain-boundary.md)
has been drawn wrong and that is the thing to fix, not the constraint.

## Milestone 5B — PostgreSQL persistence

**BQ-7 resolved architecturally; 5B does not start until every gate below
is met.** BQ-7 approved the *capability*; these gates are what turns that
into a specific, defensible set of crates.

| # | Entry gate |
|---|---|
| 1 | UUID crate ADR (from [ADR 0013](../architecture/decisions/0013-incident-identity.md), still conditional) |
| 2 | PostgreSQL client ADR |
| 3 | Async runtime ADR — the runtime follows from the frameworks, never the reverse |
| 4 | Connection-pool decision, whether in-driver or separate |
| 5 | Migration-framework decision |
| 6 | **Verified registry metadata** for every candidate — queried, not recalled |
| 7 | Dependency licence review against the Apache-2.0 core |
| 8 | `cargo tree` — a **measured** transitive closure, not an estimate |
| 9 | `cargo audit` clean, with no open unfixed advisory |
| 10 | **Windows build** — the primary development machine |
| 11 | **Linux build** — the deployment target |
| 12 | [Dependency licence matrix](../dependency-license-matrix.md) updated |
| 13 | `NOTICE` reviewed and updated where a licence requires attribution |

Gates 6 and 8 exist because every version and licence figure in these
planning documents was written from knowledge rather than from a registry
query. They are plausible and they are not evidence. See
[ADR 0018](../architecture/decisions/0018-phase5-dependency-selection.md).

Schema and forward-only migrations for all ten tables; the partial unique
index; real repository implementations; optimistic concurrency;
idempotency with fingerprints; the transactional
state-plus-timeline-plus-audit-plus-outbox write; retention jobs.

Tests: all persistence tests, with **injected failure at each of the four
commit points** proving all-or-nothing; genuine concurrent inserts
proving the partial unique index holds; migrations applying cleanly.

**Exit:** atomicity proven by failure injection, not by observing
success; tenant-less queries impossible to construct; retention never
cascades into audit.

## Milestone 5C — Ingestion and correlation

Outbox producer on the detector side; the correlation worker; retry,
backoff, dead-letter; the staleness sweep; recovery and auto-close
timers; Prometheus metrics; structured logging.

Tests: duplicate, late, and out-of-order events; poison event handling;
replay safety; the five distinct end reasons; detector restart producing
one incident rather than two; **the full end-to-end test** from synthetic
IPFIX bytes through to an incident, including the negative assertions
that nothing was notified and nothing was mitigated.

**Exit:** end-to-end test passing; replay proven safe; metric label sets
asserted against an allowlist.

## Milestone 5D — REST API

**Entry gates**, all required before the first endpoint is written:

| # | Entry gate |
|---|---|
| 1 | HTTP framework ADR, under the [ADR 0018](../architecture/decisions/0018-phase5-dependency-selection.md) criteria |
| 2 | OpenAPI approach — generated from code, or hand-maintained and tested against the implementation |
| 3 | TLS boundary — terminated at the service or at a proxy, stated either way |
| 4 | Authentication seam, so Phase 8 can replace the identity provider without touching the incident domain |
| 5 | Authorization seam — `PermissionResolver`, per [ADR 0017](../architecture/decisions/0017-incident-community-enterprise-boundary.md) |
| 6 | Rate-limiting approach and its storage |
| 7 | API error model — RFC 9457 problem details with stable `error` codes |
| 8 | Dependency review for the framework and its closure, gates 6–13 of 5B |

Endpoints from the [API plan](../architecture/incident-api-plan.md);
authorization at the command boundary; cursor pagination; filtering and
sorting through an allowlist; rate limiting; RFC 9457 error bodies; the
OpenAPI document promoted from draft to a real file.

Tests: all API tests, especially **404-not-403 for cross-tenant on every
endpoint**, and rejection of unknown fields and undocumented sort fields.

**Exit:** tenant isolation suite passing on every endpoint; OpenAPI
matching the implementation.

## Milestone 5E — CLI

`wetechinetmonctl incidents` per the
[CLI plan](../architecture/incident-cli-plan.md); table, JSON, and wide
output; exit codes; confirmations; per-command idempotency keys reused
across retries.

Tests: output formats; exit codes per error class; confirmation required
without `--yes`; **no TTY plus no `--yes` is an error, never an assumed
yes**; keys reused on retry.

**Exit:** every command mapped to an endpoint with no business logic in
the CLI.

## Milestone 5F — Operations and hardening

Installation and upgrade documentation; the runbook promoted from plan;
**tested** backup and restore (NFR-2); capacity benchmarks actually run,
with real numbers replacing the placeholders; a security review against
all 24 threats; final full validation.

**Exit:** every threat has a passing test; benchmarks measured and
published; restore tested rather than assumed; acceptance criteria met.

## Dependency graph

```text
BQ-5, BQ-6 (resolved) ──► 5A ──► 5B ──► 5C ──► 5D ──► 5E ──► 5F
                                  ▲       │              │
   BQ-7 (resolved) + ADR 0018 ────┘       └── 5D may ────┘
   crate selection                            start once
                                              5B lands
```

5D depends on 5B for persistence, not on 5C, so API work can proceed in
parallel with ingestion once the repositories exist.

## Per-milestone gate

Every milestone must pass, before review:

`cargo fmt --check` · `cargo clippy --workspace --all-targets -- -D
warnings` · `cargo test --workspace` · `cargo build --workspace
--all-targets` · `make validate` · `mkdocs build --strict` ·
`markdownlint-cli2` at the CI pin · `js-yaml` · `actionlint` ·
`git diff --check` · DCO on every commit.

Plus, every milestone: **the existing 403 Phase 4 tests remain green**,
no Phase 4 behaviour is modified, no dependency is added beyond those
approved in BQ-7, and ClickHouse stays on `0.13`.

## Not in Phase 5

Notification delivery, mitigation, BGP, RTBH, FlowSpec, firewall, router
control, webhooks, script execution, full RBAC, SSO, Entra ID, customer
portal, PDF reports, ML, distributed correlation, multi-region, SLA
billing, subscriptions, bulk mutation (**FU-22**), binary evidence
storage (**FU-23**), and customer-visible notes.

Seams are planned for several of these. **A planned seam is not an
implementation**, and no milestone above delivers functionality behind
one.
