# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once versioned releases begin (see [ROADMAP.md](ROADMAP.md)).

## [Unreleased]

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
