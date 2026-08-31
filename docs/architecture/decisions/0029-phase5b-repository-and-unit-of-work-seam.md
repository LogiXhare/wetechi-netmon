# 0029. Phase 5B Repository and Unit-of-Work Seam Extraction

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

`phase5-implementation-plan.md` claimed Milestone 5A delivers "repository
*traits* with in-memory implementations." Verified against the actual
source, this is not accurate: `crates/incident` exposes exactly three
traits (`IncidentGenerator`, `NumberAllocator`, `PermissionResolver`).
`IncidentUnitOfWork` (`crates/incident/src/unit_of_work.rs`,
2,713 lines) is a concrete struct making **72 direct accesses** to its
own `HashMap`/`Vec` fields (`incidents`, `open_index`, `dedup_seen`,
`timeline`, `audit`, `outbox`, `idempotency`), with the correlation
algorithm, authorization checks, and storage calls interleaved inside
single methods (e.g. `ingest_detection_event`).

There is no trait a `crates/incident-postgres` implementation could
satisfy today. **This is a correction of the architecture record, not a
new requirement** — Phase 5B cannot "implement" 5A's persistence
promise, because that promise was not built. Phase 5B must first extract
the seam 5A's own planning document assumed already existed.

Two more findings compound this:

- `Incident` derives only `Debug, Clone, PartialEq` — no
  `Serialize`/`Deserialize`, no public constructor.
  `state_before_recovering` and `matched_metrics` are `pub(crate)` with
  **no accessors** (`crates/incident/src/incident.rs:109,115`).
  A separate crate cannot construct or fully read an `Incident` today.
- `Timestamp` has exactly one public constructor,
  `Timestamp::now(clock)`, and its comparisons use the **monotonic**
  (`Instant`) half, which is process-local and cannot be restored from a
  database row. See [ADR 0031](0031-phase5b-durable-time.md).

## Options Considered

### Option A — Phase 5B-0: extract the seam first, in-tree, with zero new dependency, before any SQL exists

- Pros: the refactor is reviewable on its own — a pure-Rust,
  dependency-free diff against code two adversarial reviews have already
  certified, rather than buried inside a PostgreSQL implementation diff
  where reviewers must evaluate seam correctness and SQL correctness at
  once; the in-memory `IncidentUnitOfWork` becomes the *reference*
  implementation of the new trait(s), so its 107 existing unit tests
  continue exercising the seam directly; FU-38 (the internal
  `close_internal`/`reopen_incident_internal` guard gap) is naturally
  fixed in the same pass, since extracting the seam is exactly the "a
  second caller" condition FU-38's own text names as the trigger for
  needing the fix.
- Cons: a real body of work before any PostgreSQL code is written; risk
  of destabilizing recently-certified code if done carelessly.

### Option B — Design the PostgreSQL repository directly against `IncidentUnitOfWork`'s current concrete shape, seam extraction as a side effect

- Pros: fewer total commits.
- Cons: conflates two large changes — extracting an interface and
  implementing a database-backed version of it — into one diff, making
  regression harder to isolate if something breaks; does not produce a
  reviewable seam on its own, contradicting the reviewability principle
  every prior Phase 5 milestone was built around.

### Option C — Leave `IncidentUnitOfWork` concrete; PostgreSQL persistence becomes a snapshot/restore layer bolted onto the existing struct rather than a real repository seam

- Pros: smallest possible change to `crates/incident`.
- Cons: does not solve the actual problem — a `crates/incident-postgres`
  crate still cannot express "the same command-handling logic, backed by
  PostgreSQL instead of `HashMap`s" without either duplicating
  `IncidentUnitOfWork`'s ~2,700 lines or depending on it directly with
  its storage swapped by feature flag, which is not a seam, it is a
  configuration hack. Rejected.

## Decision

**Option A.** Milestone **5B-0** — no SQL, no external dependency —
does:

1. **Persistence contract extraction.** Introduce a repository-shaped
   trait (or small set of traits — the exact decomposition is
   implementation work, not fixed by this ADR) covering incident
   read/write, timeline append, audit append, idempotency check/record,
   and outbox append, matching the operations `IncidentUnitOfWork`
   already performs against its own fields.
2. **Aggregate snapshot and controlled reconstitution** — see
   [ADR 0030](0030-phase5b-aggregate-reconstitution.md).
3. **Durable timestamp semantics** — see
   [ADR 0031](0031-phase5b-durable-time.md).
4. **Internal state-transition guard hardening** — close **FU-38** by
   moving a cause-dispatched `can_automatic_transition_to`/
   `can_operator_transition_to` check inside `close_internal` and
   `reopen_incident_internal` themselves, with the table-driven
   illegal-source-state test FU-38's own text already specifies as the
   acceptance gate.
5. **Migrate the in-memory adapter to the new seam** — `IncidentUnitOfWork`
   (or its successor) becomes the trait's reference implementation,
   proving the seam is real by having a second, independent
   implementation possible in principle before one is actually written.
6. **Regression tests** — the existing 531 workspace tests must remain
   green throughout; the seam extraction is a refactor, not a behavior
   change.

**Crate placement:** the future PostgreSQL implementation lives in a new
crate, `crates/incident-postgres`. **No PostgreSQL dependency is added to
`crates/incident`.** `crates/incident` gains the trait(s) and the
snapshot/reconstitution API, both dependency-free.

## Consequences

**Easier.** A PostgreSQL implementation becomes "implement this trait,"
reviewable against a fixed contract, rather than a monolithic rewrite of
`IncidentUnitOfWork`. The seam extraction's own correctness is verifiable
by the existing 531 tests before a single line of SQL exists.

**Harder.** Phase 5B now has a milestone (5B-0) that touches code every
prior review certified, before the "actual" PostgreSQL work — a real
schedule cost, accepted because the alternative (Option B) makes
regression harder to isolate, not because it is free.

**Forecloses.** Nothing — the trait shape is implementation-time work;
this ADR fixes only that a seam must exist and where the PostgreSQL
implementation lives, not its exact method signatures.

**Security.** None distinct — this is an internal architecture
correction with no external attack surface change.

**License.** N/A — no dependency.

## Follow-Up

- [ ] Design the exact trait decomposition at Phase 5B-0 implementation
      time, informed by (not fixed by) this ADR.
- [ ] Close **FU-38** as part of 5B-0, with the acceptance-gate test
      FU-38's register entry already specifies.
- [ ] Correct `phase5-implementation-plan.md`'s 5A description, which
      claims a repository seam 5A did not deliver — tracked as part of
      this planning pass's documentation updates.
- [ ] Add a regression test asserting no PostgreSQL or Tokio type
      appears in `crates/incident`'s public API (paired with
      [ADR 0021](0021-phase5b-async-runtime-boundary.md)'s follow-up).
