# 0022. Phase 5B Connection Pool

Status: **Conditionally Accepted** — pending the Phase 5B-1 dependency
probe
Date: 2026-08-24
Deciders: Repository owner

## Context

[ADR 0020](0020-phase5b-postgresql-client.md) selects `tokio-postgres`,
which does not bundle a connection pool. ADR 0018 lists
`deadpool-postgres`/`bb8` as candidates to evaluate.

## Verified evidence (2026-08-24)

| | `deadpool-postgres` | `bb8` |
|---|---|---|
| Version | 0.14.1 | 0.9.1 |
| License | MIT OR Apache-2.0 | **MIT only** |
| Last release | 2024-12-18 | 2025-11-24 |
| Advisories | None found (no `rustsec/advisory-db` directory) | None found (no directory) |
| Repository activity | `bikeshedder/deadpool`, **pushed 2026-08-21**, 1,330 stars, 67 open issues | `djc/bb8`, pushed 2026-08-24 (today), 950 stars, 30 open issues |

`deadpool-postgres`'s ~20-month release gap initially reads as
stale; the repository's commit and push history shows it is
maintained-and-stable rather than abandoned — the last release simply
predates a need for a new one. `bb8` is more recently released but
carries no dependency-graph advantage over `deadpool-postgres` and is
MIT-only.

**Requires implementation-time verification:** measured `cargo tree`
overlap with `tokio-postgres`'s own dependency graph (a pool crate built
specifically for a client typically shares much of that client's
closure), `cargo audit`, Windows-GNU build.

## Options Considered

### Option A — `deadpool-postgres`

- Pros: dual MIT/Apache-2.0 licensed — Apache-2.0 carries an explicit
  patent grant that MIT lacks, which matters for a project with a
  planned commercial edition; purpose-built for `tokio-postgres`
  specifically (not a generic `r2d2`-style pool retrofitted onto it);
  actively maintained.
- Cons: fewer stars/less mindshare than `bb8`.

### Option B — `bb8`

- Pros: generic async pool (`r2d2` for async), pushed today at research
  time, slightly smaller reported open-issue count.
- Cons: **MIT-only** — no patent grant, a real (if often overlooked)
  gap for a project that intends a commercial edition per ADR 0018's own
  licensing criterion; generic design (works with more than just
  `tokio-postgres`) is not an advantage here since this project has
  exactly one client to pool.

### Option C — hand-rolled minimal pool

- Pros: zero dependency, full control.
- Cons: connection pooling correctness (health checks, acquire timeouts,
  max lifetime, graceful shutdown, backpressure under load) is exactly
  the kind of infrastructure code most likely to have a subtle
  concurrency bug if written once for this project rather than
  battle-tested across many. Rejected — not evaluated further.

### Option D — no pool for tests, real pool for production

- Not a fourth candidate but a deployment note: the test-database
  strategy ([ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md)
  follow-up) may use a single connection or a tiny pool per test suite;
  this ADR governs the production pool crate regardless.

## Decision

**Option A, conditionally: `deadpool-postgres` 0.14.1.** The dual
license is decisive given the other candidate is functionally
comparable and MIT-only.

Safe initial defaults to design against, **not production sizing
claims:**

- `max_size`: configurable, no hardcoded production value asserted here.
- `create_timeout` / `wait_timeout`: bounded, never infinite — an
  unbounded wait under database outage would itself become the
  "queue full" backpressure failure
  [incident-persistence.md](../incident-persistence.md)'s failure table
  already requires surfacing as `503`, not silently blocking forever.
- `recycling_method`: verify a connection before reuse rather than
  trusting an idle one, so a database restart or network blip does not
  hand a broken connection to a request.
- No credential is ever hardcoded; connection parameters come from the
  existing external-configuration pattern this repository already uses
  (R7 in [risk-register.md](../../risk-register.md)).

## Consequences

**Easier.** A pool purpose-built for the selected client, with a
license compatible with the commercial-edition intent.

**Harder.** One more dependency; its overlap with `tokio-postgres`'s own
closure is unmeasured until the Phase 5B-1 probe runs.

**Forecloses.** Nothing — `bb8` remains a documented fallback if the
probe finds a problem specific to `deadpool-postgres`.

**Security.** Pool exhaustion is a denial-of-service surface; see the
threat-model update
([incident-threat-model.md](../../security/incident-threat-model.md))
for the corresponding threat entry and the bounded-timeout requirement
above.

**License.** MIT OR Apache-2.0, compatible with the Apache-2.0 core and
preferable to the MIT-only alternative for a commercial edition.

## Follow-Up

- [ ] Run the Phase 5B-1 probe measuring the combined
      `tokio-postgres` + `deadpool-postgres` closure.
- [ ] Define concrete pool-sizing defaults at Phase 5B-3, informed by
      (not asserted before) the performance-test plan.
- [ ] Add pool metrics (`pool_in_use`, `pool_idle`, `pool_wait`,
      `pool_timeout`) to the observability plan.
