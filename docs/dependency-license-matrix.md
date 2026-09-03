# Dependency License Matrix

Status: Phase 5B-1 dependency probe — rows 32–37 verified locally
2026-09-03 (Windows-GNU build, `cargo tree`, `unsafe` inventory,
`cargo audit`); Linux confirmation pending this probe's own PR CI. See
FU-42 in [follow-ups.md](development/follow-ups.md)
Last updated: 2026-09-03

No dependency is approved for use until this matrix has a completed row for
it with an explicit Approved/Rejected decision. License fields that are not
independently and currently verified by the reader are marked **REQUIRES
VERIFICATION** rather than guessed — do not treat unmarked license values
below as legally confirmed; they reflect commonly known licensing at time
of writing and must be re-verified against the actual version selected
before any commercial distribution.

| # | Project | Purpose | Known/likely license | Copyleft implications | Commercial distribution | Status |
|---|---|---|---|---|---|---|
| 1 | Rust toolchain | Core language | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 2 | tokio | Async runtime | MIT (verified via `cargo metadata` against v1.53.1) | None | Safe | **Approved** — vendored in `crates/collector`, `tools/flow-replay` (Phase 2) |
| 3 | axum | REST API framework | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 4 | tonic | gRPC framework | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 5 | serde / serde_json / serde_yaml | Serialization | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 6 | clap | CLI argument parsing | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 7 | sqlx | PostgreSQL client | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 8 | tracing / tracing-subscriber | Structured logging/tracing | MIT (verified via `cargo metadata` against v0.1.44 / v0.3.23) | None | Safe | **Approved** — vendored in `crates/common` (Phase 2) |
| 9 | prometheus (Rust crate) | Metrics | Apache-2.0 (verified via `cargo metadata` against v0.14.0) | None | Safe | **Approved** — vendored in `crates/collector` (Phase 2) |
| 10 | GoBGP | BGP speaker (external process/API dependency, not linked) | Apache-2.0 — REQUIRES VERIFICATION | None expected at Apache-2.0 | Likely safe; verify integration is via API/process boundary, not static link, before assuming no obligations | REQUIRES VERIFICATION |
| 11 | ClickHouse (server, external service) | Analytics storage | Apache-2.0 — REQUIRES VERIFICATION | None expected | Safe as external service dependency | REQUIRES VERIFICATION — integration code written (Phase 3, `crates/storage`) and unit-tested, but no live ClickHouse server was reachable in this environment to verify the actual write path end to end; see docs/integrations/clickhouse.md |
| 12 | PostgreSQL (server, external service) | Config/metadata storage | PostgreSQL License (permissive) — REQUIRES VERIFICATION | None | Safe | REQUIRES VERIFICATION |
| 13 | Prometheus (server) | Metrics collection | Apache-2.0 — REQUIRES VERIFICATION | None | Safe | REQUIRES VERIFICATION |
| 14 | Grafana | Dashboards | **AGPLv3 for core Grafana (post-2021 relicensing); some components Apache-2.0 — REQUIRES VERIFICATION against the exact edition/version used** | **AGPLv3 has network-use copyleft implications; must be evaluated before bundling/redistributing a modified Grafana in a commercial appliance** | **BLOCKING — legal review required before Phase 6 commercial packaging** | REQUIRES VERIFICATION — flagged as legal risk |
| 15 | NATS JetStream (candidate transport) | Event transport | Apache-2.0 — REQUIRES VERIFICATION | None expected | Safe | REQUIRES VERIFICATION |
| 16 | Redpanda (candidate transport) | Event transport | **Business Source License (BSL) for some components — REQUIRES VERIFICATION**, converts to Apache-2.0 after a time delay per version | Commercial-use restrictions possible under BSL depending on version/edition | **Needs legal check before selection** if BSL-covered version is used | REQUIRES VERIFICATION — flagged as risk |
| 17 | Kafka (candidate transport) | Event transport | Apache-2.0 — REQUIRES VERIFICATION | None | Safe | REQUIRES VERIFICATION |
| 18 | React | Web app framework | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 19 | Vite | Build tool | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 20 | Tailwind CSS | CSS framework | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 21 | shadcn/ui | UI components | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 22 | Recharts | Charting | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 23 | Apache ECharts | Charting | Apache-2.0 | None | Safe | REQUIRES VERIFICATION (version pin) |
| 24 | InfluxDB (v1-compatible target, external service) | Legacy metrics output | MIT (v1.x) / varies by version — REQUIRES VERIFICATION | Verify per version | Safe if MIT-era version targeted | REQUIRES VERIFICATION |
| 25 | MkDocs Material | Documentation site | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 26 | thiserror | Error-type derive macro | MIT OR Apache-2.0 (verified via `cargo metadata` against v2.0.20) | None | Safe | **Approved** — vendored in `crates/protocol-ipfix`, `crates/common`, `crates/collector` (Phase 2) |
| 27 | proptest | Property-based testing | MIT OR Apache-2.0 (verified via `cargo metadata` against v1.11.0) | None | Safe, dev-dependency only (not shipped in release binaries) | **Approved** — dev-dependency in `crates/protocol-ipfix` (Phase 2) |
| 28 | clickhouse (Rust client crate) | ClickHouse HTTP client/writer | MIT OR Apache-2.0 (verified via `cargo metadata` against v0.13.3) | None | Safe | **Approved** — vendored in `crates/storage` (Phase 3) |
| 29 | time | Date/time handling for ClickHouse row timestamps | MIT OR Apache-2.0 (verified via `cargo metadata` against v0.3.55) | None | Safe | **Approved** — vendored in `crates/storage`, `crates/collector` (Phase 3) |
| 30 | serde (direct use) | Row serialization for `clickhouse::Row` | MIT OR Apache-2.0 (verified via `cargo metadata` against v1.0.229) | None | Safe | **Approved** — vendored in `crates/storage` (Phase 3; already a transitive dependency since Phase 2, now a direct one) |
| 31 | libfuzzer-sys | libFuzzer bindings for the cargo-fuzz target | MIT OR Apache-2.0 (commonly known; **not independently verified via `cargo metadata`** — the fuzz crate is its own standalone Cargo workspace, per `crates/protocol-ipfix/fuzz/Cargo.toml`, so it doesn't appear in the main workspace's dependency graph) | None expected | Safe; dev/tooling-only, never shipped in a release binary | REQUIRES VERIFICATION — **not yet run in CI or locally in this environment** (no nightly toolchain here); verify before first real `cargo fuzz run` |
| 32 | uuid | UUIDv7 incident-identity generation (Phase 5B, `crates/incident-postgres` only) | Apache-2.0 OR MIT — re-verified 2026-09-03 against the actually-resolved v1.26.0 (the `"1.25"` requirement's newest compatible patch), reading `Cargo.toml`'s `license` field directly in the vendored source, not crates.io API | None expected | Safe — `features = ["v7"], default-features = false`, no `uuid::Uuid` type crosses `crates/incident`'s public API (it is used only inside the new probe stub, `crates/incident-postgres`) | **Approved, pending this PR's Linux CI confirmation** — [ADR 0019](architecture/decisions/0019-phase5b-uuidv7-identity-generation.md); Windows-GNU build, cargo tree, unsafe inventory, and cargo audit (0 vulnerabilities/252 crates) verified locally 2026-09-03; Linux (ubuntu-latest) verification is this PR's own required Rust check, see [FU-42](development/follow-ups.md) |
| 33 | tokio-postgres | PostgreSQL async client (Phase 5B, `crates/incident-postgres`) | MIT OR Apache-2.0 — re-verified 2026-09-03 against the resolved v0.7.18 (exact ADR-pinned version) | None | Safe — 1 advisory found (RUSTSEC-2026-0178), **patched at the selected version 0.7.18**; `cargo audit` against the full resulting `Cargo.lock` (252 crates) found **0 vulnerabilities** | **Approved, pending this PR's Linux CI confirmation** — [ADR 0020](architecture/decisions/0020-phase5b-postgresql-client.md); Windows-GNU build, cargo tree, unsafe inventory, and cargo audit (0 vulnerabilities/252 crates) verified locally 2026-09-03; Linux (ubuntu-latest) verification is this PR's own required Rust check, see [FU-42](development/follow-ups.md) |
| 34 | deadpool-postgres | Connection pool for `tokio-postgres` (Phase 5B) | MIT OR Apache-2.0 — re-verified 2026-09-03 against the actually-resolved v0.14.2 (the `"0.14.1"` requirement's newest compatible patch) | None expected | Safe — no advisories found in RustSec advisory-db | **Approved, pending this PR's Linux CI confirmation** — [ADR 0022](architecture/decisions/0022-phase5b-connection-pool.md); Windows-GNU build, cargo tree, unsafe inventory, and cargo audit (0 vulnerabilities/252 crates) verified locally 2026-09-03; Linux (ubuntu-latest) verification is this PR's own required Rust check, see [FU-42](development/follow-ups.md) |
| 35 | rustls | TLS backend for the PostgreSQL connection (Phase 5B) | Apache-2.0 OR ISC OR MIT — re-verified 2026-09-03 against the resolved v0.23.43 (exact ADR-pinned version) | None | Safe — 2 advisories found (RUSTSEC-2024-0336, RUSTSEC-2024-0399), **both patched** well below the selected version. **New finding, not in ADR 0023:** rustls 0.23.x's default crypto provider pulls in `aws-lc-rs`/`aws-lc-sys` (a vendored, compiled-from-C fork of BoringSSL, compound-licensed ISC/Apache-2.0/MIT/BSD-3-Clause, requiring a C compiler + `cmake` at build time) as a **transitive** dependency — 73 crates and ~14,300 `unsafe`-keyword occurrences added to the workspace's dependency closure, of which `aws-lc-sys` alone accounts for ~87%. This does not change the license or advisory verdict (`cargo audit` found 0 vulnerabilities, and NOTICE only tracks direct dependencies per its own stated convention), but it is a materially larger and more C-toolchain-dependent closure than choosing rustls's alternative `ring` crypto provider would have produced — recorded as a fact for whoever designs the actual TLS wiring in Milestone 5B-2, not acted on here. | **Approved, pending this PR's Linux CI confirmation** — [ADR 0023](architecture/decisions/0023-phase5b-postgresql-tls.md); Windows-GNU build, cargo tree, unsafe inventory, and cargo audit (0 vulnerabilities/252 crates) verified locally 2026-09-03; Linux (ubuntu-latest) verification is this PR's own required Rust check, see [FU-42](development/follow-ups.md) |
| 36 | tokio-postgres-rustls | Bridges `tokio-postgres` to `rustls` (Phase 5B) | MIT — re-verified 2026-09-03 against the resolved v0.14.0 (exact ADR-pinned version) | None expected | Safe — no advisories found in RustSec advisory-db | **Approved, pending this PR's Linux CI confirmation** — [ADR 0023](architecture/decisions/0023-phase5b-postgresql-tls.md); Windows-GNU build, cargo tree, unsafe inventory, and cargo audit (0 vulnerabilities/252 crates) verified locally 2026-09-03; Linux (ubuntu-latest) verification is this PR's own required Rust check, see [FU-42](development/follow-ups.md) |
| 37 | refinery | Forward-only PostgreSQL migrations (Phase 5B) | MIT — re-verified 2026-09-03 against the resolved v0.9.2 (exact ADR-pinned version), `default-features = false, features = ["tokio-postgres"]` | None expected | Safe — no advisories found in RustSec advisory-db | **Approved, pending this PR's Linux CI confirmation** — [ADR 0024](architecture/decisions/0024-phase5b-migration-framework.md); Windows-GNU build, cargo tree, unsafe inventory, and cargo audit (0 vulnerabilities/252 crates) verified locally 2026-09-03; Linux (ubuntu-latest) verification is this PR's own required Rust check, see [FU-42](development/follow-ups.md) |

## Toolchain-Only, Not Shipped in Any Binary

The Rust GNU toolchain requires a MinGW-w64 build environment (binutils —
`as`, `ar`, `dlltool`, `ld` — plus `gcc.exe` used only as a linker driver)
to link `windows-sys`-dependent crates on Windows dev machines. Installed
locally via winget (`BrechtSanders.WinLibs.POSIX.UCRT`, multiple licenses
— see <https://winlibs.com/#license>). This is **build tooling only**
— none of it is linked into or distributed with WetechiNetMon binaries, so
it carries no distribution obligations for the product itself. Not added
as a numbered row above since it is not a dependency of the product in any
license-relevant sense; noted here for engineering-environment
reproducibility (see docs/development/local-setup.md).

## Process

Each row above is a **candidate**, not an approval. Before a dependency is
actually added to `Cargo.toml`, `package.json`, or a deployment manifest:

1. Verify the license of the exact version pinned (not just "the project").
2. Fill in copyright notice requirements and record them for NOTICE file
   generation (Phase 1 deliverable).
3. Record integration method (linked library vs. external service/process
   boundary vs. build-time-only tool) — this materially changes copyleft
   exposure (e.g. GoBGP and ClickHouse as separate processes/services carry
   different obligations than a statically linked GPL library would).
4. Record security maintenance status (actively maintained? recent CVEs?).
5. Mark Approved or Rejected explicitly — "REQUIRES VERIFICATION" is not a
   valid terminal state for anything actually shipped.

**"Conditionally Approved"** was a distinct, non-terminal state used for
the Phase 5B candidates while the crate's version, license, and advisory
history were independently verified against live sources but the crate
was not yet added to any `Cargo.toml`. Rows 32–37 carried that state
until the Phase 5B-1 probe (2026-09-03) added all six to
`crates/incident-postgres` and measured transitive closure, `cargo
audit`, and `unsafe` inventory, with a Windows-GNU build verified
locally — all clean. They now read **"Approved, pending this PR's Linux
CI confirmation"**: the Linux leg is this probe's own PR running the
workspace's existing Rust check on `ubuntu-latest`, not yet observed
green at the time these rows were written. Flip to a plain **Approved**
once that check passes; do not backdate this note to imply it already
did.

## Known Legal Flags Raised in Phase 0

- **Grafana AGPLv3 relicensing (2021)**: if WetechiNetMon bundles or
  modifies Grafana itself (rather than shipping original dashboard JSON
  that an operator loads into their own separately-installed Grafana),
  AGPLv3's network-copyleft terms could require WetechiNetMon's own source
  to be made available. Recommended default posture: ship Grafana
  dashboard JSON and provisioning config only, treat Grafana server as an
  operator-supplied external dependency (like PostgreSQL), never bundle a
  modified Grafana binary. This must be confirmed as a blocking question
  before Phase 6 — see [blocking-questions.md](blocking-questions.md).
- **Redpanda BSL**: if Redpanda is selected as the event transport instead
  of NATS or Kafka, its license terms must be re-checked per version before
  any commercial/managed-service use, since BSL is not OSI-approved
  open-source and can carry field-of-use restrictions.

## Out of Scope for Phase 0

Actually running `cargo deny`, `cargo audit`, or SBOM tooling — that is a
Phase 1 CI deliverable once there is a real `Cargo.toml` to scan.
