# Roadmap

Status: Phase 2 complete
Last updated: 2026-08-20

This roadmap mirrors the phased delivery model and versioning plan in the
master prompt (sections 28–29). No phase begins until the prior phase has
documented output, acceptance criteria, passing required tests, a summary,
an explicit decision record, a commit, and updated documentation.

## Phases and Milestones

| Phase | Milestone | Deliverable focus |
|---|---|---|
| Phase 0 | — | Product foundation and clean-room boundary (this document set) — ✅ complete |
| Phase 1 | v0.1.0 | GitHub repository and documentation foundation — ✅ complete |
| Phase 2 | v0.2.0 | IPFIX collector MVP — ✅ complete |
| Phase 3 | v0.3.0 | Aggregation and direction classification |
| Phase 3 (cont.) | v0.4.0 | ClickHouse and Prometheus metrics |
| Phase 4 | v0.5.0 | Static detection engine |
| Phase 5 | v0.6.0 | Incident lifecycle |
| Phase 6 | v0.7.0 | Grafana and native UI |
| Phase 6 (cont.) | v0.8.0 | Notification integrations |
| Phase 7 | v0.9.0 | BGP mitigation lab |
| Phase 8 | v1.1.0 | Multi-tenancy |
| Phase 8 (cont.) | v1.2.0 | Enterprise authentication |
| Phase 9 | — | Production hardening (feeds into v1.0.0) |
| Phase 10 | v1.0.0 | Production-ready single-tenant release |
| Future | v2.0.0 | Distributed high-availability architecture |

Note: the master prompt's phase list (0–10) and version list (v0.1.0–v2.0.0)
interleave slightly — multi-tenancy/enterprise auth (v1.1.0/v1.2.0, Phase 8)
land before the v1.0.0 single-tenant release phase (Phase 10) is finalized
in the phase numbering, but v1.0.0 is defined as the single-tenant MVP. This
sequencing ambiguity is noted as a **minor open question**, not a blocking
one — Phase 1 planning should confirm whether Phase 8 (tenancy) precedes or
follows the v1.0.0 cut, since the version numbers imply tenancy is
post-1.0 while the phase list places it at Phase 8 of 10. Recommended
resolution: treat Phase 8–9 as post-v1.0.0 work items (v1.1.0/v1.2.0) and
let Phase 10 (v1.0.0) be reached after Phase 7, with Phase 9 hardening
tasks applied twice (once before v1.0.0, once before v1.1.0/v1.2.0) —
subject to confirmation, see [blocking-questions.md](blocking-questions.md).

## Per-Phase Exit Requirements (restated from master prompt §29 and §30)

Every phase must produce, before the next phase starts:

- Completed items
- Files created / modified
- Tests executed and their actual results
- Security considerations
- License considerations
- Documentation created
- Known limitations
- Risks (feeding [risk-register.md](risk-register.md))
- Next phase
- Recommended Conventional Commits message

## Phase 0 (this phase) — Scope

Product charter, clean-room boundary, functional and non-functional
requirements, architecture and technology options, dependency license
matrix, commercial boundaries, security principles, MVP scope, out-of-scope
list, risk register, this roadmap, acceptance criteria, blocking questions,
and naming/branding decision. No production code.

## Phase 1 — Scope (complete)

GitHub repository and documentation foundation — monorepo skeleton,
README, LICENSE recommendation, NOTICE, SECURITY, CONTRIBUTING,
CODE_OF_CONDUCT, GOVERNANCE, SUPPORT, ROADMAP, CHANGELOG, MkDocs skeleton,
ADR template, issue templates, PR template, CODEOWNERS, Dependabot,
validation CI, local dev setup, Makefile, Taskfile.

## Phase 2 — Scope (complete)

IPFIX collector MVP: clean-room IPFIX decoder (`crates/protocol-ipfix`),
per-exporter template caching and restart detection, Prometheus metrics,
structured JSON logging (`crates/common`), a synthetic flow replay tool
(`tools/flow-replay`), unit tests, property-based ("never panics on
arbitrary bytes") tests, and documentation (IPFIX collector guide,
configuration reference, ADR 0001 recording the Rust-for-collector
decision). See [architecture/decisions/0001-collector-implementation-language.md](architecture/decisions/0001-collector-implementation-language.md).

**Known limitation carried forward:** true coverage-guided fuzzing
(`cargo-fuzz`/libFuzzer) requires a nightly Rust toolchain, not installed
in this environment — property-based tests cover the same "never panics"
safety property via `proptest` instead. Tracked in
[risk-register.md](risk-register.md) R4, not silently dropped.

## Immediately Next: Phase 3 Preview

Aggregation and direction classification: host/network/hostgroup/ASN
aggregation, Incoming/Outgoing/Internal/Other classification, ClickHouse
output, tests, documentation. **Not started** — requires review of
Phase 2 first, per master prompt §29.
