# 0037. Phase 5D HTTP Framework and OpenAPI

Status: **Accepted**. The 5D dependency probe (gate 8) passed on 2026-10-05:
16 new third-party crates, `cargo audit` clean over 270, no `unsafe` in
axum, axum-core, tower, tower-layer, utoipa or utoipa-gen, and a clean
Windows-GNU build, with Linux checked by CI. See the
[licence matrix](../../dependency-license-matrix.md), rows 38 and 39.
Date: 2026-10-05
Deciders: Repository owner. On 2026-10-05 the owner delegated the choice
to industry practice and standards ("industry practice & standard se
anujayee continue kro").

## Context

Milestone 5D's first two entry gates are this ADR:

1. an HTTP framework chosen under [ADR 0018](0018-phase5-dependency-selection.md)'s
   criteria;
2. an OpenAPI approach, either generated from code or hand-maintained and
   tested against the implementation.

ADR 0018 listed `axum`, `actix-web`, `poem`, `salvo` and plain `hyper`,
and said the planning lean towards `axum` "must not be adopted on the
strength of that lean". The evidence below was queried for this ADR, not
recalled.

## Verified evidence (2026-10-05)

Sources: crates.io for versions, dates, licences, MSRV and downloads;
GitHub for repository activity; `rustsec/advisory-db` for advisories.

| | `axum` | `actix-web` | `poem` | `salvo` |
|---|---|---|---|---|
| Version | 0.8.9 (2026-04-14) | 4.15.0 (2026-08-21) | 3.1.12 (2025-07-28) | 1.0.1 (2026-10-04) |
| Licence | MIT | MIT OR Apache-2.0 | MIT OR Apache-2.0 | Apache-2.0 |
| MSRV | 1.80 | 1.88 | 1.85 | 1.94 |
| Downloads, last 90 days | 126.0 M | 11.3 M | 0.76 M | 0.81 M |
| Repository | `tokio-rs/axum`, pushed 2026-10-04, 27 365 stars, 87 open issues | `actix/actix-web`, pushed 2026-10-03, 24 855 stars, 191 open issues | `poem-web/poem`, pushed 2026-10-04, 4 444 stars | `salvo-rs/salvo` |
| Advisories | none for `axum`. `axum-core`: RUSTSEC-2022-0055, patched at ≥ 0.2.8 / ≥ 0.3.0-rc.2 (selected: 0.5.6) | `actix-web`: RUSTSEC-2018-0019; `actix-http`: 2020-0048, 2021-0081, all long patched | none | not queried; ruled out below |
| Runtime | Tokio, already in the workspace | its own `actix-rt` on Tokio, plus actor heritage | Tokio | Tokio |

Already in `Cargo.lock` through the PostgreSQL and ClickHouse clients:
`hyper` 1.11, `hyper-util`, `http` 1.5, `tower-service`, `tokio-rustls`.
`hyper` carries 7 historical advisories, the latest RUSTSEC-2022-0022,
patched at ≥ 0.14.12; the workspace has 1.11. `tower-http` carries
RUSTSEC-2021-0135 and 2022-0043, both patched at ≥ 0.2.1.

OpenAPI candidates:

| | `utoipa` (+ `utoipa-axum`) | `aide` |
|---|---|---|
| Version | 6.0.0 (2026-09-22) | 0.15.1 (2025-08-19) |
| Licence | MIT OR Apache-2.0 | MIT OR Apache-2.0 |
| MSRV | 1.88 | unpublished |
| Downloads, last 90 days | 16.5 M | 0.86 M |
| Repository | `juhaku/utoipa`, pushed 2026-10-04 | `tamasfe/aide` |
| Advisories | none | none |

The local toolchain is Rust 1.97.1, so every MSRV above is met.

## Decision

### Framework: `axum` 0.8

- **Least new runtime and the smallest new closure.** It is Tokio and
  `hyper` 1, which the workspace already carries, so the new crates are
  essentially `axum`, `axum-core`, `tower` and `matchit`. The probe
  measures this rather than assuming it.
- **Middleware is plain `tower`.** Authentication, rate limiting, request
  ids, body limits and timeouts are `tower` layers. They can be tested
  without a socket, and they are not tied to the framework.
- **It is the de facto standard.** It is maintained by the Tokio project
  and has about ten times the downloads of the next candidate. This is
  the industry-practice criterion the owner set.
- **Typed extractors fit this API's rules.** A rejected body, unknown
  field or bad path parameter becomes our problem-details response
  through one rejection handler (ADR 0038).
- **Licence:** MIT, compatible with the Apache-2.0 core. Attribution goes
  in `NOTICE`.

Rejected:

- **`actix-web`.** Mature and fast. It brings its own runtime layer next
  to Tokio, and a second middleware model beside the `tower` the
  workspace already uses.
- **`poem`.** Built-in OpenAPI is attractive. The last release was in
  July 2025, and its adoption is about 0.6 % of axum's.
- **`salvo`.** The current release, 1.0.1, is from 2026-10-04. The MSRV
  is 1.94, and its adoption (0.8 M downloads in 90 days) is small.
- **`hyper` directly.** The smallest closure, but we would write and
  maintain the routing, extraction and rejection handling that axum
  already provides.

### OpenAPI: generated from code with `utoipa` 6, checked against a committed file

- **Generated from code.** Every handler and schema type carries
  `utoipa` annotations, and the document is built from them. A spec
  written by hand drifts from the code. A spec generated from the code
  cannot describe a route that does not exist.
- **The generated document is committed** at `docs/api/openapi.json`. A
  test regenerates it and fails if it differs. So:
  - every API change shows up as a reviewable spec diff;
  - the draft in [docs/api/incident-api-plan.md](../../api/incident-api-plan.md)
    is replaced by a real file;
  - the exit criterion "OpenAPI matching the implementation" is a test,
    not a promise.
- **`utoipa-axum`** is added only if the probe shows it is worth its
  closure. Plain `utoipa` with an explicit path list works with any
  router.
- **No Swagger UI is served.** It would bundle a JavaScript application
  into the binary. Operators can point any OpenAPI viewer at the file.
- `aide` is rejected for its smaller adoption and stale release cadence.

## Consequences

**Easier.**

- Middleware, extraction and errors follow the most widely documented
  Rust pattern.
- The OpenAPI document cannot silently drift.
- The workspace keeps one async runtime ([ADR 0021](0021-phase5b-async-runtime-boundary.md)).
- `crates/incident` stays free of HTTP: the API crate depends on it, not
  the other way round.

**Harder.**

- `utoipa`'s derive macros add compile time, measured by the probe.
- Annotations must be kept on every handler. The committed-spec test is
  what makes forgetting one visible.

## Follow-ups

- [x] The 5D dependency probe (gate 8):
  - measured `cargo tree` for `axum` and `utoipa` with the selected
    features;
  - `cargo audit`;
  - the `unsafe` inventory;
  - Windows-GNU and Linux builds;
  - the [licence matrix](../../dependency-license-matrix.md) and
    `NOTICE` updated.
- [ ] `docs/api/openapi.json` generated, committed, and checked by a
  test.
