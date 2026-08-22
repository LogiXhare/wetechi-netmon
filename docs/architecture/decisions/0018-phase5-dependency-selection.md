# 0018. Phase 5 Dependency Selection: Criteria and Shortlist

Status: **Proposed** — selection criteria only. **No crate is chosen by
this ADR**, and none may be added on the strength of it.
Date: 2026-08-22
Deciders: Repository owner (crate selection pending)

## Context

[ADR 0015](0015-incident-operational-storage.md) records that BQ-7 was
approved on 2026-08-22 at the *architectural* level: Phase 5 may
introduce PostgreSQL and HTTP dependencies. That approval deliberately
stopped short of naming crates.

It stopped short for a reason. Phase 4 added **zero** third-party crates,
and Phase 5 will add the first since Phase 3. Choosing them inside an
implementation commit — where the diff is dominated by the code that uses
them — is how a dependency with a bad licence, an unmaintained upstream,
or a 200-crate transitive closure gets adopted without anyone deciding
to.

This ADR fixes the **criteria** and the **shortlist** now, while the
question is still visible, and defers the answer to a focused review that
queries the registry rather than trusting recollection.

## The honesty constraint

**Every version, licence, and maintenance claim in the Phase 5 planning
documents was written from knowledge, not from a registry query.** That
includes the parenthetical "`sqlx` or `tokio-postgres`, both
MIT/Apache-2.0" in BQ-7 itself. Those claims are plausible and are
**not** evidence.

The selection review must verify, for each candidate, against the actual
registry and repository at the time of selection:

- current version and release date
- licence, read from the published package, not assumed
- commits and releases in the last twelve months
- open advisories (`cargo audit`, RustSec)
- **measured** transitive dependency count, from `cargo tree`, not
  estimated
- whether it forces an async runtime, and which
- a clean build on **both Windows and Linux** — the primary development
  machine is Windows, and a crate that only builds on Linux would be
  discovered far too late

A candidate that cannot be verified is not selected. This mirrors the
discipline that produced Phase 4's dependency posture, where the YAML
crates were rejected on checked evidence — `serde_yaml` really is
published as `0.9.34+deprecated` — rather than on impression.

## Candidates

Shortlists to evaluate, not a ranking.

### PostgreSQL client

| Candidate | Evaluate for |
|---|---|
| `sqlx` | Compile-time-checked queries; async; migration support built in; larger closure |
| `tokio-postgres` | Lower level, smaller closure; no compile-time query checking; pooling separate |
| `diesel` | Mature ORM and migrations; synchronous core; a different programming model |
| `deadpool-postgres` / `bb8` | Pooling, if the driver does not provide it |

### HTTP server framework

| Candidate | Evaluate for |
|---|---|
| `axum` | Tower ecosystem; typed extractors; Tokio-based |
| `actix-web` | Mature, fast; its own actor runtime heritage |
| `poem` | OpenAPI generation built in |
| `salvo` | Smaller ecosystem |
| `hyper` directly | Smallest closure; the most code to write and maintain ourselves |

### The async runtime is a consequence, not a preference

Phase 3 already depends on Tokio through the collector, so a Tokio-based
choice adds a runtime the workspace *already* carries rather than a new
one. That is a genuine argument and should be weighed as such — but the
runtime must be justified by the frameworks selected, not chosen first
and used to justify them.

## Criteria

Weighted for a project that intends a commercial edition and an
Apache-2.0 core:

| Criterion | Why it matters here |
|---|---|
| Licence | Must be compatible with the Apache-2.0 core; copyleft is disqualifying for a distributed binary |
| Maintenance | An unmaintained dependency in an audited system is a liability, not a saving |
| Advisories | Open unfixed advisories are disqualifying |
| Transitive count | Phase 4's small closure is what makes ADR 0007's "cannot reach a router" claim checkable; a large closure erodes that for the incident crate too |
| Runtime coupling | Two runtimes in one binary is a defect |
| Compile time | Affects every future contributor |
| Type safety | Compile-time query checking removes a class of production failure |
| Migrations | Needed at Milestone 5B either way |
| Pooling | Needed; may be in the driver or separate |
| TLS | Needed for a remote database |
| OpenAPI | Useful, since the API plan already carries a draft spec |
| Testability | Integration tests need a real PostgreSQL |
| Windows + Linux | Non-negotiable; the dev machine is Windows |
| Commercial distribution | Attribution obligations must be satisfiable in `NOTICE` |

## Decision

**Deferred, deliberately.** No crate is selected. This ADR records what
must be true of the answer, and the next ADR in this series records the
answer with evidence attached.

A directional note, explicitly **not** a selection: `sqlx` or
`tokio-postgres` for PostgreSQL, and `axum` for the API, are the
candidates the planning work leaned toward. They must not be adopted on
the strength of that lean.

## Consequences

**Easier.** The dependency decision is visible as its own reviewable
artefact rather than buried in an implementation diff. The criteria are
fixed before anyone has an implementation to defend.

**Harder.** One more review step before Milestone 5B can begin, and the
verification work is real: registry queries, `cargo tree`, `cargo audit`,
and a two-platform build for each finalist.

**Forecloses.** Nothing. It constrains *how* the choice is made.

**Security.** The verification requirements are the control. Skipping
them to save time is the failure this ADR exists to prevent.

**License.** Every selected crate needs a matrix row and, where its
licence requires attribution, a `NOTICE` entry, before it is added.

## Follow-Up

- [ ] Select the PostgreSQL driver, with verified evidence — **FU-25**.
- [ ] Select the HTTP framework, with verified evidence — **FU-26**.
- [ ] Update
      [dependency-license-matrix.md](../../dependency-license-matrix.md)
      and `NOTICE` before any crate is added.
- [ ] Re-verify the `uuid` crate under the same rules
      ([ADR 0013](0013-incident-identity.md)).
