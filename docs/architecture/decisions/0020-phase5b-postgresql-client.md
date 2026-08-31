# 0020. Phase 5B PostgreSQL Client

Status: **Conditionally Accepted** — pending the Phase 5B-1 dependency
probe (see [ADR 0018](0018-phase5-dependency-selection.md))
Date: 2026-08-24
Deciders: Repository owner

## Context

[ADR 0018](0018-phase5-dependency-selection.md) fixed the criteria and a
shortlist (`sqlx`, `tokio-postgres`, `diesel`) without selecting one, and
explicitly warned that its own directional note ("`sqlx` or
`tokio-postgres`") "must not be adopted on the strength of that lean."
**FU-25** requires the selection be made with verified evidence.

## Verified evidence (2026-08-24)

| | `tokio-postgres` | `sqlx` |
|---|---|---|
| Version | 0.7.18 (2026-06-12) | 0.9.0 (2026-05-21) |
| License | MIT OR Apache-2.0 | MIT OR Apache-2.0 |
| Advisories | 1, RUSTSEC-2026-0178, **patched at 0.7.18** (the version selected) | 1, RUSTSEC-2024-0363, patched at ≥0.8.1 (well below 0.9.0) |
| Repository activity | `sfackler/rust-postgres`, pushed 2026-07-27, 3,989 stars | `transact-rs/sqlx`, pushed 2026-08-20, 17,406 stars |
| MSRV | Not separately published for this evaluation | 1.94.0 (verified from CHANGELOG; local toolchain is 1.97.1, compatible) |
| Governance | Long-standing `sfackler` maintainership | Repository moved from `launchbadge` to `transact-rs` in May 2026 — verified as a **governance clarification**: the project states it was never actually owned by LaunchBadge LLC and moved to collective ownership of its principal authors, not an abandonment |

Both advisories are patched at the versions this ADR would select.
Neither disqualifies its crate under ADR 0018's "open unfixed advisories
are disqualifying" criterion.

**Requires implementation-time verification:** measured `cargo tree` for
each, `cargo audit`, `unsafe` inventory, Windows-GNU and Linux build,
compile-time impact (sqlx's proc-macros vs. tokio-postgres's plain
async fns).

## Options Considered

### Option A — `tokio-postgres` + separate pool + separate migrations

- Pros: lower-level, smaller expected closure — consistent with the
  workspace's existing small-closure posture (153 packages today, the
  same posture that makes ADR 0007's "cannot reach a router" claim
  checkable); **no build-time database coupling** — sqlx's compile-time
  query checking needs either a live `DATABASE_URL` at build time or
  committed `.sqlx` offline metadata that can silently drift from the
  schema, which is a materially worse failure mode while GitHub Actions
  cannot run at all ([FU-1](../../development/follow-ups.md)); client,
  pool, and migrations stay separately reviewable per ADR 0018's
  explicit instruction not to approve a bundled stack "merely because
  one framework bundles everything"; 0.7.x line is mature (first
  released 2016).
- Cons: no compile-time query checking — a class of production failure
  (a typo'd column name, a type mismatch) is caught at runtime or in
  integration tests instead of at `cargo build`; pooling, migrations,
  and TLS are each a separate crate to track.

### Option B — `sqlx` 0.9.0

- Pros: compile-time-checked queries remove a real class of failure;
  built-in migration support; active security attention (the 0.9
  `SqlSafeStr` change specifically targets protocol-level query
  smuggling); large, well-audited ecosystem (17,406 stars).
- Cons: MSRV jumped to 1.94.0 in this release (three months old at
  decision time); compile-time checking needs either a live database at
  `cargo build` or maintained offline query metadata — an operational
  cost this project does not yet have infrastructure for, given CI
  cannot currently run at all; larger transitive closure expected
  (unverified, but sqlx bundles connection handling, multiple database
  backends behind features, and a macro crate); 766 open issues at
  research time, more surface than `tokio-postgres`'s 178.

### Option C — `diesel` / `diesel-async`

- Pros: mature ORM, synchronous core available.
- Cons: a different programming model (ORM query DSL vs. SQL-first) that
  the rest of this workspace does not use anywhere; `diesel-async` is a
  second dependency on top of the ORM to get the async story this
  workspace already needs via existing Tokio usage. Not evaluated
  further — no requirement in the approved architecture favors an ORM.

## Decision

**Option A, conditionally: `tokio-postgres`**, paired with
`deadpool-postgres` ([ADR 0022](0022-phase5b-connection-pool.md)),
`refinery` ([ADR 0024](0024-phase5b-migration-framework.md)), and
`tokio-postgres-rustls` ([ADR 0023](0023-phase5b-postgresql-tls.md)) as
four separately-justified, separately-reviewable crates rather than one
bundled framework.

`sqlx` 0.9.0 **remains the documented alternative** — it is not rejected
on any disqualifying ground (license, advisories, maintenance), only
weighed against `tokio-postgres` on closure size and the CI-availability
argument. If the Phase 5B-1 probe measures `tokio-postgres`'s actual
transitive closure as unexpectedly large, or a Windows-GNU build issue
surfaces, this ADR should be revisited in `sqlx`'s favor rather than
worked around.

Conditional on the Phase 5B-1 probe returning a clean result for
`tokio-postgres` specifically.

## Consequences

**Easier.** The client, pool, TLS, and migration choices are each
independently reviewable and independently replaceable, matching this
workspace's pattern of small, auditable dependencies.

**Harder.** More crates to track individually than a single bundled
framework; the team writes and maintains hand-written SQL without
compile-time checking, which the integration-test plan
([ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md),
Phase 5B-5) must compensate for with real-database coverage.

**Forecloses.** Nothing structural — `sqlx` remains adoptable later if
the closure or CI argument changes; the repository/unit-of-work seam
(ADR 0029) means the client is swappable behind that boundary without
touching `crates/incident`.

**Security.** Both advisories were investigated to their patched-version
status rather than assumed absent; see the evidence table above.

**License.** MIT OR Apache-2.0, compatible with the Apache-2.0 core.

## Follow-Up

- [ ] Run the Phase 5B-1 dependency probe for `tokio-postgres` and its
      pairing crates together (the combination is what actually gets
      built, not each crate in isolation).
- [ ] Update [dependency-license-matrix.md](../../dependency-license-matrix.md)
      with the measured result before any crate is added.
- [ ] If the probe fails a gate, re-open this ADR against `sqlx` 0.9.0
      rather than silently working around the failure.
