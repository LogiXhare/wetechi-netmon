# Engineering Follow-Ups

Work that is known, scoped, and deliberately not done yet — each entry
says what blocks it. This is not the product roadmap
([roadmap.md](../roadmap.md) covers phases); it is the list of loose ends
that would otherwise live only in a maintainer's head.

Status: opened 2026-08-21 alongside the DCO and CI-consolidation work;
extended 2026-08-22 with Phase 4 items and Phase 5 planning items.

| # | Item | Blocked on | Notes |
|---|---|---|---|
| FU-1 | Resolve GitHub Actions billing | Account owner | Actions has been unable to start jobs since 2026-08-20; every run fails within seconds with "recent account payments have failed or your spending limit needs to be increased". No workflow change can fix this. |
| FU-2 | First remote execution of the consolidated workflow | FU-1 | `.github/workflows/validate.yml` was restructured from nine jobs to three and has been statically and locally validated only. It has never executed on a GitHub-hosted runner. |
| FU-3 | Enable nightly `cargo-fuzz` | A nightly Rust toolchain | The fuzz target (`crates/protocol-ipfix/fuzz/`) and its scheduled workflow exist from Phase 3 but have never been run. |
| FU-4 | Test the pre-push hook on Linux, WSL, Git Bash, and macOS | Access to each environment | `.githooks/pre-push` has been exercised on Git Bash only. `.gitattributes` pins it to LF specifically so the other platforms work, but that is reasoning, not evidence. |
| FU-5 | Revisit CLA vs. DCO | The first external contribution | See [ADR 0006](../architecture/decisions/0006-contribution-licensing-dco-not-cla.md). Reversible only while the contributor list is empty. |
| FU-7 | ~~Fix the 9 relative links that break `mkdocs build --strict`~~ | **Done 2026-08-21** | Fixed on `chore/dco-and-ci-consolidation`. Two were genuine wrong-depth paths; the rest pointed outside `docs/`. Root governance files now have documentation-native pages ([Contributing](contributing.md), [Security Policy](../security/security-policy.md)) and source/README references use explicit repository URLs, which `.github/mlc_config.json` ignores because the repository is private. `mkdocs build --strict` now exits 0 with zero warnings. |
| FU-6 | `clickhouse` 0.13.3 to 0.15.1 compatibility | A dependency-testing branch | Dependabot PR is open and deliberately unmerged. Two minor versions across a 0.x crate is a breaking-change risk for `crates/storage`; it needs its own branch and a real build, not a blind merge. |
| FU-8 | Decide whether DCO should also be enforced on direct pushes | A branch-protection decision | The check runs on `pull_request` only, so a commit pushed straight to `main` is never checked. Deliberate and documented in `CONTRIBUTING.md` and in the workflow, not an oversight. The durable fix is branch protection requiring the `history` check before merge, which forces changes through pull requests — but branch protection needs a public repository or GitHub Pro, so it is a project decision, not a code change. |

## Phase 4 — detection engine

| # | Item | Blocked on | Notes |
|---|---|---|---|
| FU-9 | Mechanically enforce that the detector cannot reach a router | A dependency-policy tool | [ADR 0007](../architecture/decisions/0007-detection-engine-cannot-mitigate.md) claims `wetechinetmon-detector` has no dependency capable of network transport or command execution. The claim is currently held by review of `Cargo.toml`. A `cargo deny`-style check failing CI on a new transport dependency would make it structural rather than reviewed. |
| FU-10 | ASN and interface detection scopes | Demand | Both dimensions exist in the aggregator and both are plausible detection scopes. Neither is needed for the flood cases Phase 4 targets, and each costs another bounded map per direction. |
| FU-11 | Exercise the detection-event ClickHouse path against a live server | A ClickHouse instance | `DetectionEventRow` conversion is unit-tested and the batch/retry machinery is tested in `crates/storage`, but the wiring has never run against a real server — the same caveat Phase 3's export path carries. |
| FU-12 | Measure detection throughput on representative hardware | Hardware and a load generator | No throughput figure is published for the detector, deliberately. The detector adds a second counter update per flow plus a prefix lookup; that cost is understood in shape but not in numbers. |
| FU-13 | Reconsider tumbling vs. rolling windows | Evidence that boundary-split bursts are being missed | [ADR 0010](../architecture/decisions/0010-detector-owns-its-windowed-counters.md) chose tumbling windows for memory. Requiring `triggerFor` to span at least one window is what makes the split acceptable; if real traffic shows detections being missed at boundaries, a rolling window for a bounded subset of scopes is the next option. |
| FU-14 | Map detection identifiers to UUIDs if an integration needs them | An integration that requires it | [ADR 0009](../architecture/decisions/0009-detection-event-identity.md) mints identifiers without a random-number dependency, so they will not parse as UUIDs. |
| FU-15 | Add YAML as a second policy format | A maintained YAML crate | [ADR 0008](../architecture/decisions/0008-detection-policy-configuration.md). `PolicyDocument` carries no format knowledge, so this is a `from_yaml` constructor and nothing else. Revisit when a crate exists with an active maintainer and more than a year of releases. |

## Phase 5 — incident management planning

Raised by the Phase 5 architecture work. None of these blocks planning;
each is a loose end the plan deliberately left open rather than guessed
at.

| # | Item | Blocked on | Notes |
|---|---|---|---|
| FU-16 | Carry exporter and interface identity on detection events | A Phase 4 detector change | [FR-5.2](../functional-requirements.md) requires persisting which exporter and interface observed the traffic. Phase 4's event carries `exporters_observed`, a *count*, not an identity. Phase 5 planning is forbidden from changing the detector, so FR-5.2 cannot be fully satisfied until this lands. |
| FU-17 | Baseline metrics for incidents | A baselining phase | Phase 4 compares against static thresholds only, so `baseline_metrics` is `NULL` on every Phase 5 incident. The column exists so a future baselining phase needs no migration; the API must return `null` rather than `0`, because "never measured" and "measured as zero" are different facts. |
| FU-18 | Per-policy correlation-group opt-out | Evidence that policy-blind correlation merges things operators want separate | [Correlation](../architecture/incident-correlation.md) deliberately excludes `policy_id` from the key, so one attack matched by several policies is one incident. The cost is that a deliberately narrow policy is absorbed into a broad one. If that proves wrong operationally, the fix is an opt-out flag, not a change to the default key. |
| FU-19 | Tamper-evident audit log | A decision that it is needed | Hash-chaining each audit row to its predecessor is cheap if the column is reserved now. The threat it addresses is an attacker who already has database write access, so it is deferred rather than dismissed — see [threat model](../security/incident-threat-model.md). |
| FU-20 | Formal review of retention against contracts and regulation | Legal input | Retention defaults in [persistence](../architecture/incident-persistence.md) are engineering judgements. **No legal or regulatory requirement is asserted anywhere in the Phase 5 plan**, deliberately. Anything contractual needs review by someone qualified to give it. |
| FU-21 | PostgreSQL Row-Level Security for tenant isolation | Phase 8 tenancy | The Phase 5 schema is shaped so RLS can be enabled without migration — `tenant_id` on every table, no table relying on a join to establish tenancy. RLS itself needs a per-tenant role model that does not exist yet. Until then isolation is application-enforced (**R16**). |
| FU-22 | Bulk incident mutation | An authorization design for it | Closing every incident matching a filter is genuinely useful and genuinely dangerous. It needs its own permission and its own audit shape rather than being bolted onto the single-incident commands. |
| FU-23 | Binary evidence storage and its access model | A decision that it is needed | Phase 5 stores evidence *references* only. Packet samples and exported reports need a storage location, a size bound, and an authorization model on dereference before any of it ships (**R18**). |

## Phase 5 — decision review (2026-08-22)

Raised when BQ-5, BQ-6, and BQ-7 were resolved.

| # | Item | Blocked on | Notes |
|---|---|---|---|
| FU-24 | Decide whether the incident number resets annually | Owner preference | [ADR 0013](../architecture/decisions/0013-incident-identity.md) approves UUIDv7 for the internal id but leaves the display sequence open: `WNM-2026-000123` resetting each January, or a continuous per-tenant sequence. Affects a display value, not the primary key, so it does not block 5A. |
| FU-25 | Select the PostgreSQL driver, with verified evidence | [ADR 0018](../architecture/decisions/0018-phase5-dependency-selection.md) | BQ-7 approved the *capability*, not a crate. Selection requires verified registry metadata, a measured `cargo tree`, `cargo audit`, and a build on both Windows and Linux. |
| FU-26 | Select the HTTP framework, with verified evidence | [ADR 0018](../architecture/decisions/0018-phase5-dependency-selection.md) | As FU-25. The runtime choice must follow from the frameworks, not precede them. |
| FU-27 | Update FR-5.1 to reference ADR 0014 | Nothing | [FR-5.1](../functional-requirements.md) still specifies a single machine containing mitigation states. BQ-6 resolved that they stay out, so the requirement and the design now disagree in writing until FR-5.1 is amended. |
| FU-28 | Record close-to-recurrence gap distribution | Phase 5C telemetry | BQ-9's reopen window is currently a judgement. Measuring the gap between a close and the next qualifying event on the same correlation key would let the value be chosen from evidence. |

## Why these are not GitHub issues yet

The repository's CI cannot run (FU-1), so an issue tracker would be the
only moving part in a repository where nothing else can be verified
automatically. These are recorded here, in the branch that created them,
and should be opened as issues once Actions is working again.
