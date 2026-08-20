# Dependency License Matrix

Status: Phase 0 draft — candidate dependencies only, nothing vendored yet
Last updated: 2026-08-20

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
| 2 | tokio | Async runtime | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 3 | axum | REST API framework | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 4 | tonic | gRPC framework | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 5 | serde / serde_json / serde_yaml | Serialization | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 6 | clap | CLI argument parsing | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 7 | sqlx | PostgreSQL client | MIT / Apache-2.0 dual | None | Safe | REQUIRES VERIFICATION (version pin) |
| 8 | tracing / tracing-subscriber | Structured logging/tracing | MIT | None | Safe | REQUIRES VERIFICATION (version pin) |
| 9 | prometheus (Rust crate) | Metrics | Apache-2.0 | None | Safe | REQUIRES VERIFICATION (version pin) |
| 10 | GoBGP | BGP speaker (external process/API dependency, not linked) | Apache-2.0 — REQUIRES VERIFICATION | None expected at Apache-2.0 | Likely safe; verify integration is via API/process boundary, not static link, before assuming no obligations | REQUIRES VERIFICATION |
| 11 | ClickHouse (server, external service) | Analytics storage | Apache-2.0 — REQUIRES VERIFICATION | None expected | Safe as external service dependency | REQUIRES VERIFICATION |
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
