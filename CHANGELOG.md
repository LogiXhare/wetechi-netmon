# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once versioned releases begin (see [ROADMAP.md](ROADMAP.md)).

## [Unreleased]

### Added — Contribution licensing

- `DCO` — the Developer Certificate of Origin 1.1 text, verbatim.
  Contributions are accepted under Apache-2.0 and every non-merge commit
  now requires a matching `Signed-off-by` trailer (`git commit -s`).
  There is no separate CLA. See the new "Licensing and Sign-Off" section
  in [CONTRIBUTING.md](CONTRIBUTING.md); the pull-request template
  carries a matching checklist item.

### Security

- The DCO check's bot exemption no longer trusts commit author metadata.
  It previously skipped any commit whose `%an <%ae>` contained `[bot]`,
  so a contributor could bypass sign-off entirely with
  `git config user.name 'x[bot]'`. Verified against a synthetic commit
  authored as `evil[bot] <attacker@example.com>`: it was silently
  exempted before the fix and is correctly rejected after. The exemption
  is now keyed on the pull request author's login, which GitHub sets.

### Changed — CI efficiency

- `.github/workflows/validate.yml`: consolidated from nine jobs to three
  (`rust`, `docs`, `history`), split by *environment* rather than by
  individual check. `cargo fmt`/`clippy`/`test`/`build` previously ran as
  four separate runners that each recompiled the whole workspace from a
  cold cache; they now share one `target/` directory in a single job. No
  check was removed and no action version changed — the same commands run
  against the same pinned action SHAs.
- The former `secret-scan` job is now `history`, and gained a DCO
  sign-off check: it verifies every non-merge commit in a pull request
  carries a `Signed-off-by` trailer matching its author. It reuses the
  full-history checkout that the secret scan already needed, so it costs
  no extra runner minutes. Bot-authored pull requests are exempted on
  `github.event.pull_request.user.login`, which GitHub sets — never on
  commit author metadata, which a contributor controls.
- `.github/dependabot.yml`: updates are now grouped — all GitHub Actions
  bumps arrive as one pull request, and Cargo minor/patch bumps as
  another. Major Cargo bumps are deliberately left ungrouped so a
  breaking change gets reviewed on its own.
- `.githooks/pre-push` plus `make hooks` / `task hooks` — an opt-in local
  gate running `cargo fmt --check`, `cargo clippy -D warnings`, and
  `cargo test` before each push. It skips itself when `cargo` is absent,
  and honours `WETECHI_SKIP_PREPUSH=1`. Documented in
  [docs/development/local-setup.md](docs/development/local-setup.md).
- `.gitattributes` — pins shell scripts and `.githooks/**` to LF endings.
  Under `core.autocrlf=true` on Windows the hook would otherwise be
  checked out with a CRLF shebang and fail to execute on Linux or WSL.

### Added — Phase 3: Aggregation and Direction Classification

- `crates/common`: `NormalizedFlow` — protocol-independent flow record
  (`flow.rs`) and sampling-correction module (`sampling.rs`) implementing
  the documented priority order (record-level → options-template →
  exporter-configured → global default → unsampled), with zero-rate
  rejection and overflow rejection.
- `crates/classifier` (new crate): binary-trie prefix registry (IPv4 +
  IPv6, longest-prefix match, duplicate/overlap detection, tenant +
  hostgroup ownership — ADR 0002) and direction classification
  (Incoming/Outgoing/Internal/Other/Unknown, with explainable
  diagnostics). 29 tests.
- `crates/aggregator` (new crate): bounded multi-dimensional aggregation
  (hosts, networks, /24, configurable prefix lengths, hostgroups, ASNs,
  exporters, interfaces, protocols — ADR 0003), 1s/5s/15s/1m/5m rate
  windows over processing time, deterministic LRU-style eviction,
  inactivity expiration. 26 tests.
- `crates/storage` (new crate): original ClickHouse schemas for 9 tables,
  bounded batch writer, bounded retry queue with exponential backoff and
  drop-oldest-on-overflow (ADR 0005), idempotent migrations. 13 unit
  tests + 1 skip-cleanly-without-a-server integration test.
- `crates/collector`: wired IPFIX → normalize → classify → aggregate →
  (optional) ClickHouse export pipeline via a bounded in-process channel
  (ADR 0004); SIGTERM graceful shutdown (Unix); 14 new Prometheus
  metrics; periodic inactivity-expiration sweep; env-var configuration
  for local prefixes, aggregation limits, sampling defaults, and optional
  ClickHouse URL. 28 tests.
- `tools/flow-replay`: extended for IPv4/IPv6, TCP/UDP/ICMP, sampling
  (via synthetic Options Templates), and multiple simulated
  exporters/observation domains, with a `--scenario` flag for
  incoming/outgoing/internal/other traffic. 5 tests.
- `crates/protocol-ipfix/fuzz/`: `cargo-fuzz` target for
  `decode_message`, plus `.github/workflows/fuzz.yml` (scheduled/manual,
  nightly toolchain isolated to that workflow only) — **not executed**
  in this environment (no nightly toolchain available locally).
- ADRs 0002–0005 (prefix lookup data structure, in-memory aggregation
  structure, collector-aggregator event transport, ClickHouse batching
  and retry).
- Documentation: `docs/architecture/aggregation.md`,
  `docs/architecture/direction-classification.md`,
  `docs/configuration/prefixes.md`, `docs/configuration/aggregation.md`,
  `docs/integrations/clickhouse.md`,
  `docs/operations/aggregator-monitoring.md`,
  `docs/operations/capacity-planning.md`,
  `docs/development/flow-replay.md`.
- `docs/dependency-license-matrix.md` and `NOTICE` updated with the
  `clickhouse` and `time` Rust crates (both `cargo metadata`-verified).

**Known limitations carried forward:** ClickHouse write path not
verified against a live server (none available here); `cargo-fuzz` not
executed (no nightly toolchain here); `interface_traffic` ClickHouse
table not yet wired (aggregator's interface dimension isn't
exporter-scoped); no performance benchmark executed (100k flows/sec is a
documented target, not a measured result). None of these are silently
dropped — see `docs/risk-register.md` and the relevant docs pages above.

### Added — Phase 2: IPFIX collector MVP

- `crates/protocol-ipfix`: clean-room IPFIX (RFC 7011/7012/7015) decoder
  — message header, Template Sets, Options Template Sets, Data Sets
  (fixed and variable-length fields), per-exporter `TemplateCache`, and
  structural sampling-parameter extraction (`SamplingInfo`). 34 tests
  including 3 `proptest` properties ("never panics on arbitrary bytes").
- `crates/collector` (`wetechinetmon-collector` binary): UDP IPFIX
  listener, per-exporter template caching with sequence-number-regression
  restart detection, a hand-rolled `/metrics` HTTP endpoint (Prometheus
  text format), and environment-variable configuration
  (`WETECHINETMON_COLLECTOR_BIND`, `WETECHINETMON_COLLECTOR_METRICS_BIND`).
  16 tests, including a real `tokio` TCP integration test of the metrics
  endpoint. Verified end-to-end against a real running process (not just
  unit tests) — see `docs/roadmap.md` Phase 2 section.
- `crates/common`: shared structured JSON logging setup
  (`wetechinetmon_common::logging::init`).
- `tools/flow-replay` (`flow-replay` binary): synthetic-only IPFIX
  traffic generator for safely testing the collector, with a round-trip
  test against the real decoder.
- ADR 0001: Rust selected as the Telemetry Collector's implementation
  language (`docs/architecture/decisions/0001-collector-implementation-language.md`),
  resolving the leaning recorded in Phase 0's `docs/architecture-options.md`.
- Root `Cargo.toml` workspace; `Cargo.lock` now committed (binary crates —
  see `.gitignore`).
- `docs/integrations/ipfix-collector.md`, `docs/configuration/index.md`
  populated with the real collector config options.
- `docs/dependency-license-matrix.md` and `NOTICE` updated with real,
  `cargo metadata`-verified license rows for the crates now vendored
  (tokio, tracing, tracing-subscriber, thiserror, prometheus, proptest).
- `.github/dependabot.yml`: added the `cargo` ecosystem.
- `.github/workflows/validate.yml`: added `cargo fmt --check`,
  `cargo clippy -D warnings`, `cargo test --workspace`, and
  `cargo build --workspace --all-targets` jobs.

**Known limitation carried forward:** true coverage-guided fuzzing
(`cargo-fuzz`/libFuzzer) needs a nightly Rust toolchain, not installed in
this environment. Property-based tests (`proptest`) cover the same
"never panics" safety property via random sampling instead. Tracked in
`docs/risk-register.md` R4, not silently dropped.

### Added — Phase 1: GitHub repository and documentation foundation

- Full monorepo directory skeleton per `prompts/CLAUDE_MASTER_PROMPT.md`
  §22 (apps/, crates/, deployments/, docs/, grafana/, database/, tests/,
  tools/, scripts/, examples/, branding/), with reserved-status README
  placeholders and no production code.
- Professional root `README.md`.
- `LICENSE` (Apache License 2.0, recommendation pending confirmation —
  see `docs/license-recommendation.md`) and `NOTICE`.
- `SECURITY.md`, `SUPPORT.md`, `CONTRIBUTING.md`, `GOVERNANCE.md`,
  `CODE_OF_CONDUCT.md` (Contributor Covenant v2.1).
- Root `ROADMAP.md` (public summary of `docs/roadmap.md`).
- MkDocs Material skeleton (`mkdocs.yml` + `docs/*/index.md` section
  landing pages).
- Architecture Decision Record process and template
  (`docs/architecture/decisions/`).
- GitHub issue templates (bug report, feature request, documentation) and
  issue-template config linking to `SECURITY.md`.
- Pull-request template with a clean-room self-certification checklist.
- `.github/CODEOWNERS`.
- `.github/dependabot.yml` (github-actions ecosystem).
- Validation-only GitHub Actions workflow (Markdown lint, YAML lint,
  Markdown link check) — no build, deploy, or release automation yet.
- `Makefile` and `Taskfile.yml` with `lint-markdown`, `lint-yaml`,
  `docs-serve`, `docs-build`, and `validate` targets.
- `docs/development/local-setup.md` documenting how to work with the
  repository at this phase.

### Added — Phase 0: Product foundation and clean-room boundary

- Product charter, clean-room boundary, functional and non-functional
  requirements, architecture and technology options, dependency license
  matrix, commercial boundaries, security principles, MVP scope,
  out-of-scope list, risk register, roadmap, acceptance criteria, blocking
  questions, and naming/branding decisions under `docs/`.

[Unreleased]: https://github.com/badshashorif/wetechi-netmon/compare/main...HEAD
