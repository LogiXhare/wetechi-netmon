# Phase 5 Acceptance Criteria

Status: **Planning only.** Two distinct gates below: what makes the
*planning* complete (this session's deliverable), and what will make the
*implementation* complete (a future gate, listed so it is not invented
retrospectively).

## Gate 1 — Planning complete

The deliverable of this session. Owner review turns these from "produced"
into "approved".

| # | Criterion | Status |
|---|---|---|
| 1 | Phase 4 event model documented from source, not from memory | Done — [plan](../architecture/phase5-incident-management-plan.md) |
| 2 | Gaps between FR-5 and the real Phase 4 model identified | Done — five gaps |
| 3 | Incident domain boundary defined | Done — [ADR 0011](../architecture/decisions/0011-incident-domain-boundary.md) |
| 4 | Incident model defined with bounds | Done — [domain model](../architecture/incident-domain-model.md) |
| 5 | Correlation rules deterministic, with worked examples | Done — [correlation](../architecture/incident-correlation.md) |
| 6 | State machine complete, legal and illegal edges | Done — [state machine](../architecture/incident-state-machine.md) |
| 7 | Every transition has a permission | Done |
| 8 | Detection clear behaviour defined, five end reasons distinguished | Done |
| 9 | Recurrence and reopen behaviour defined | Done |
| 10 | Assignment designed, including failure cases | Done — [security model](../architecture/incident-security-model.md) |
| 11 | Severity and priority separated | Done |
| 12 | Timeline append-only | Done — [persistence](../architecture/incident-persistence.md) |
| 13 | Audit separate from timeline | Done |
| 14 | PostgreSQL selected with reasons | Done — [ADR 0015](../architecture/decisions/0015-incident-operational-storage.md) |
| 15 | Transaction boundaries defined | Done |
| 16 | Concurrency control defined | Done — [ADR 0016](../architecture/decisions/0016-incident-concurrency-and-idempotency.md) |
| 17 | Idempotency defined, including key reuse | Done |
| 18 | Outbox designed | Done — [ADR 0012](../architecture/decisions/0012-incident-event-ingestion.md) |
| 19 | Tenant isolation designed at three layers | Done |
| 20 | Authorization permissions enumerated | Done |
| 21 | REST API planned with schemas | Done — [API plan](../api/incident-api-plan.md) |
| 22 | CLI planned | Done — [CLI plan](../architecture/incident-cli-plan.md) |
| 23 | Prometheus metrics planned, cardinality bounded | Done — [observability](../architecture/incident-observability.md) |
| 24 | Structured logging planned, exclusions explicit | Done |
| 25 | Capacity bounds defined with breach behaviour | Done |
| 26 | Retention designed; no invented legal requirement | Done |
| 27 | Threat model complete, 24 threats | Done — [threat model](../security/incident-threat-model.md) |
| 28 | Test strategy complete | Done — [testing plan](../architecture/incident-testing-plan.md) |
| 29 | Performance plan defined; no number claimed | Done |
| 30 | Community/Enterprise seam defined | Done — [ADR 0017](../architecture/decisions/0017-incident-community-enterprise-boundary.md) |
| 31 | Implementation milestones defined | Done — [implementation plan](phase5-implementation-plan.md) |
| 32 | Out-of-scope items explicit | Done |
| 33 | No dependency added | Verified |
| 34 | No production code added | Verified |
| 35 | No migration created | Verified |
| 36 | MkDocs strict passes | Verified |
| 37 | Markdown lint passes | Verified |
| 38 | YAML validation passes | Verified |
| 39 | actionlint passes | Verified |
| 40 | 403 Rust tests remain green | Verified |
| 41 | Working tree clean after commits | Verified |
| 42 | Blocking questions raised, not silently decided | Done — BQ-5 to BQ-9 |

**Gate 1 status as of 2026-08-22.** BQ-5, BQ-6, and BQ-7 are **resolved**
— see [blocking questions](../blocking-questions.md). BQ-8 and BQ-9
remain open, and neither blocks implementation: both are runtime
configuration defaults, and the mechanisms they parameterise are already
decided.

Gate 1 is therefore **substantially passed**, with the caveat that two
production defaults have not yet been consciously chosen. Shipping either
default unreviewed would be the failure mode this register exists to
prevent.

## Gate 2 — Implementation complete

A future gate. Listed now so it is defined before the work starts rather
than assembled afterwards to match whatever was built.

### Functional

- Detection events ingest through the outbox, at-least-once, with
  effectively-once processing.
- Correlation is deterministic and order-independent, proven by property
  test.
- Duplicate events never create a second incident, for any interleaving.
- Every legal transition works; every illegal one is refused with a
  stable error code.
- The five end reasons are distinguished and surfaced through the API.
- Recovery, abort-with-prior-state-restoration, auto-close, and reopen
  all behave per configuration.
- A detector restart mid-attack yields **one** incident, not two.
- A detector that goes silent leaves no incident open forever.
- Assignment, notes with supersession, severity and priority changes all
  work and are audited.
- The REST API implements every planned endpoint.
- The CLI implements every planned command.

### Non-functional

- Every mutation is atomic across state, timeline, audit, and outbox,
  **proven by injected failure at each commit point**.
- An audit write failure rolls back the mutation.
- Optimistic concurrency prevents lost updates.
- Idempotent retries are safe; key reuse with a different body conflicts.
- Tenant isolation holds across every endpoint; cross-tenant returns
  `404`.
- Every capacity limit is enforced with defined breach behaviour.
- The timeline is never truncated.
- Prometheus label sets are asserted against an allowlist by a test.
- No note body appears in a log at `INFO` or above.
- Benchmarks are **run and published with real numbers**.
- Backup and restore are **tested**, not assumed.

### Safety — the non-negotiables

- **No notification is delivered by any code path**, proven by a
  recording sink that must receive nothing.
- **No mitigation is attempted**, proven structurally by the incident
  crate's dependency closure containing no BGP, firewall, router, SMTP,
  or chat-delivery crate — the same verification ADR 0007 uses for the
  detector.
- `executed` remains `false` on every linked detection event.
- `mitigation_status` and `notification_status` are `none` on every
  incident.
- Phase 4 detector behaviour is unmodified.
- ClickHouse remains on `0.13`.
- No Dependabot PR merged as part of Phase 5.

### Process

- All 403 existing tests green, plus the Phase 5 suite.
- Every gate exit 0.
- DCO on every commit.
- ADRs 0011–0017 moved from Proposed to Accepted, or superseded.
- FR-5.1 updated to reference ADR 0014, so the requirement and the
  implementation stop disagreeing.
- Documentation updated: installation, configuration, operations,
  runbook, API reference.
- Risk register updated; R16–R18 reassessed.

## Known limitations to carry forward

Even a complete Phase 5 will still have these, and they should be stated
rather than discovered:

- Tenant isolation is application-enforced until Phase 8 RLS (**R16**).
- Output escaping cannot be proven until a UI exists (**R17**).
- Evidence storage is undesigned (**R18**, **FU-23**).
- Exporter and interface identity are absent from detection events, so
  FR-5.2 cannot be fully satisfied without a Phase 4 change (**FU-16**).
- Baseline metrics are always `NULL` — Phase 4 has no baselining.
- Correlation is single-node.
- GitHub-hosted CI remains billing-blocked (**FU-1**), so Phase 5
  evidence will again be local-only.
