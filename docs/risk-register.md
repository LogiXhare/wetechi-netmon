# Risk Register

Status: Phase 0 draft
Last updated: 2026-08-20

Scale: Likelihood (L) and Impact (I) rated Low/Medium/High. This register
is reviewed and updated at the end of every phase per the master prompt's
phase-summary requirement.

| # | Risk | L | I | Mitigation |
|---|---|---|---|---|
| R1 | Accidental reuse of proprietary FastNetMon (or other vendor) concepts, terminology, or schema shape by an engineer/agent unconsciously pattern-matching on prior exposure | M | H | Clean-room boundary document, explicit "never copy" list, PR self-certification checklist (Phase 1), escalate any doubt as a blocking question |
| R2 | Grafana AGPLv3 relicensing creates unexpected copyleft obligations if a modified Grafana binary is ever bundled | M | H | Ship dashboard JSON + provisioning only; treat Grafana server as operator-supplied external dependency; confirmed as blocking question before Phase 6 |
| R3 | Redpanda BSL license terms restrict commercial/managed-service use if selected as event transport | L | M | Flagged in dependency matrix; default leaning is NATS JetStream; legal check required if Redpanda is chosen instead |
| R4 | Flow collector (untrusted UDP input) has a memory-safety or parsing vulnerability exploitable by a crafted packet | M | H | Rust with no unsafe without review (confirmed — `crates/protocol-ipfix` has zero `unsafe`), malformed-packet test suite (implemented, 34 tests). **Partial mitigation as of Phase 2**: property-based tests (`proptest`, 3 properties, "never panics on arbitrary bytes") are implemented and passing; true coverage-guided `cargo-fuzz`/libFuzzer fuzzing requires a nightly Rust toolchain not installed in this environment, so real fuzz corpora/crash-minimization coverage is not yet in place. Follow-up: install a nightly toolchain and add `cargo-fuzz` targets before this risk can be downgraded further. |
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

## Review Trigger

Add a row whenever a phase surfaces a new risk in its "Risks" section
(required by master prompt §30). Close/downgrade a row only with a stated
reason, not silently.
