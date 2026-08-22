# Risk Register

Status: Phase 5 planning
Last updated: 2026-08-22

Scale: Likelihood (L) and Impact (I) rated Low/Medium/High. This register
is reviewed and updated at the end of every phase per the master prompt's
phase-summary requirement.

| # | Risk | L | I | Mitigation |
|---|---|---|---|---|
| R1 | Accidental reuse of proprietary FastNetMon (or other vendor) concepts, terminology, or schema shape by an engineer/agent unconsciously pattern-matching on prior exposure | M | H | Clean-room boundary document, explicit "never copy" list, PR self-certification checklist (Phase 1), escalate any doubt as a blocking question |
| R2 | Grafana AGPLv3 relicensing creates unexpected copyleft obligations if a modified Grafana binary is ever bundled | M | H | Ship dashboard JSON + provisioning only; treat Grafana server as operator-supplied external dependency; confirmed as blocking question before Phase 6 |
| R3 | Redpanda BSL license terms restrict commercial/managed-service use if selected as event transport | L | M | Flagged in dependency matrix; default leaning is NATS JetStream; legal check required if Redpanda is chosen instead |
| R4 | Flow collector (untrusted UDP input) has a memory-safety or parsing vulnerability exploitable by a crafted packet | M | H | Rust with no unsafe without review (confirmed — `crates/protocol-ipfix` has zero `unsafe`), malformed-packet test suite (implemented, 34 tests). Property-based tests (`proptest`, 3 properties, "never panics on arbitrary bytes") are implemented and passing. **As of Phase 3**: a `cargo-fuzz` target now exists (`crates/protocol-ipfix/fuzz/`) plus a scheduled/manual CI workflow (`.github/workflows/fuzz.yml`) — but neither has actually been *executed*, in CI or locally, since no nightly Rust toolchain is available in this development environment. This risk downgrades to Low once the CI fuzz workflow has run at least once with a clean result (tracked, not assumed). |
| R5 | BGP mitigation misconfiguration announces an overly broad or unauthorized blackhole/FlowSpec route in production | L | H | Dry-run and BGP-disabled by default at every layer, authorized-prefix allowlist, manual approval for first production mitigation, emergency disable switch |
| R6 | High-cardinality or adversarial flow traffic exhausts collector/aggregator memory (self-inflicted DoS) | M | M | Bounded memory, top-N limits, backpressure, cardinality protection designed in from Phase 3 |
| R7 | Secrets committed to Git (config files, .env, credentials) during rapid iteration | M | H | No secrets in Git rule, secret-scanning CI (Phase 1/22), documented external-config pattern from day one |
| R8 | Scope creep — building Enterprise/Managed-tier features or artificial Community-edition limitations before there is a real edition boundary decision | M | M | Explicit MVP scope and out-of-scope documents, "no artificial limitations in OSS core during MVP" rule |
| R9 | Dependency with unverified or incompatible license gets vendored without review | M | M | Dependency-license-matrix process; nothing added to build files without a completed row |
| R10 | Event-transport or collector-language decisions get made implicitly by whichever code gets written first, rather than deliberately via ADR | M | M | ADRs required before the phase that depends on each decision (Phase 1 template, enforced before Phase 3/7) |
| R11 | Fabricated test results, benchmark numbers, or security claims presented as real | L | H | Explicit rule (master prompt §30): never claim tests passed unless actually executed; report commands and results verbatim |
| R12 | Multi-tenant data leakage once tenancy ships (v1.1.0) if schema-level tenant scoping wasn't designed in early | L | H | NFR-3 requires tenant ID as a first-class schema dimension from early phases even though enforcement ships later |
| R13 | WeTechi Solutions' own commercial licensing intent (dual-license? proprietary enterprise modules?) is never decided, blocking Phase 1 LICENSE file | M | M | Raised as blocking question; Phase 1 cannot fully complete repository scaffolding without an answer |
| R14 | Reference lab values (ASNs, IPs, communities in master prompt §4) leak into code as hardcoded defaults instead of configuration | L | M | NFR-7 explicit rule against hardcoding; lab values are reference-only, never defaults baked into binaries |
| R15 | Long multi-phase project loses coherence across sessions/agents, re-deciding settled questions or drifting from this Phase 0 baseline | M | M | Phase 0 docs act as the durable baseline; every phase must reference and update them rather than restart reasoning from scratch |
| R16 | Incident tenant isolation is enforced in application code until Phase 8 adds Row-Level Security, so a single missing tenant predicate leaks another tenant's incidents | M | H | Tenant context is a repository constructor argument, making a tenant-less query inexpressible rather than merely discouraged; `tenant_id` on every table so RLS can be enabled without migration; a dedicated cross-tenant isolation suite runs every endpoint as tenant A against tenant B ([threat model](security/incident-threat-model.md) T-05) |
| R17 | Operator note content is stored verbatim and could execute as stored XSS once a web UI renders it (Phase 6) | M | M | The API returns JSON only and never HTML, and documents notes as untrusted content. Input sanitisation is deliberately **not** used — it destroys the operator's actual words and gives false assurance. Escaping on output is a Phase 6 acceptance requirement (T-08) |
| R18 | Evidence attachment has no designed access model, because binary evidence storage is out of Phase 5 scope | L | M | Phase 5 stores evidence *references* only, tenant-scoped and authorized on dereference. Binary storage must not ship before its access model is designed (T-22, FU-23) |

## Review Trigger

Add a row whenever a phase surfaces a new risk in its "Risks" section
(required by master prompt §30). Close/downgrade a row only with a stated
reason, not silently.
