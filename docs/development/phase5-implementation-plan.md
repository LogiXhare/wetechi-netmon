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

These block 5A, not just the phase:

- **BQ-5** incident identity — determines the primary key type
- **BQ-6** FR-5.1 deviation — determines the state set
- **BQ-7** dependency approval — determines whether 5B is possible at all
- **BQ-8** manual closure for critical — a state-machine rule
- **BQ-9** reopen window default — a correlation rule

BQ-5, BQ-6, and BQ-7 are hard blockers: each changes code that 5A writes.
BQ-8 and BQ-9 are configuration defaults and could be deferred to 5C, but
resolving all five together is cheaper than revisiting.

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

## Milestone 5B — PostgreSQL persistence

**Blocked on BQ-7.**

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
BQ-5, BQ-6, BQ-7 ──► 5A ──► 5B ──► 5C ──► 5D ──► 5E ──► 5F
                              │                    │
                              └── 5D may start ────┘
                                  once 5B lands
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
