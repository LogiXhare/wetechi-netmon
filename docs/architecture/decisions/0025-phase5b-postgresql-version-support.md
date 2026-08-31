# 0025. Phase 5B PostgreSQL Version Support Range

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

A supported PostgreSQL range must be fixed before schema design commits
to any version-specific feature. Nothing in prior Phase 5 planning
recorded one.

## Verified evidence (2026-08-24, official PostgreSQL versioning policy)

| Version | Latest minor | End of life |
|---|---|---|
| 18 | 18.6 | 2030-11-14 |
| 17 | 17.11 | 2029-11-08 |
| 16 | 16.15 | 2028-11-09 |
| 15 | 15.19 | 2027-11-11 |
| 14 | 14.24 | **2026-11-12 — under three months from this decision** |

Each major version is supported for five years from initial release.
PostgreSQL 18 introduced a built-in `uuidv7()` function (verified — see
[ADR 0019](0019-phase5b-uuidv7-identity-generation.md)); nothing else in
the schema design requires a version newer than 15 (partial unique
indexes, JSONB, `uuid`, transaction semantics, and Row-Level Security all
predate it by many years).

## Options Considered

### Option A — Minimum 15, recommended 17, test matrix 15/16/17/18

- Pros: excludes 14, which is about to lose all support; 16 is Ubuntu
  24.04 LTS's packaged default, so a floor of 15 keeps that path open
  without requiring it; 17 gives headroom on a managed-service default
  without requiring the newest major version; testing through 18 without
  requiring it keeps the door open for the native `uuidv7()` function
  later without coupling this decision to it now.
- Cons: supporting a four-version matrix is more integration-test surface
  than a single pinned version.

### Option B — Require PostgreSQL 18

- Pros: newest features available, including native `uuidv7()`.
- Cons: unnecessarily aggressive — nothing in the approved schema needs
  it, and it excludes any operator still on a widely-supported managed
  PostgreSQL 15/16 offering for no functional reason. Rejected per this
  task's explicit instruction not to require 18 "solely because it is
  current."

### Option C — Single pinned version (e.g., only 17)

- Pros: simplest test matrix.
- Cons: needlessly narrow for a project that does not yet have its own
  managed-hosting story; an operator on a different supported major
  version gains nothing from being excluded.

## Decision

**Option A.** Minimum PostgreSQL **15**. Recommended production version
**17**. Tested versions: **15, 16, 17, 18**. Unsupported: 14 and below.

**Upgrade policy:** this range is revisited whenever a supported major
version approaches its own end-of-life (following the same five-year
horizon), not on a fixed calendar schedule.

## Consequences

**Easier.** A schema and query design that does not accidentally depend
on an 18-only feature; a documented, verifiable version claim instead of
an assumed one.

**Harder.** Four versions in the integration-test matrix multiplies CI
time once CI is unblocked (see [FU-1](../../development/follow-ups.md)).

**Forecloses.** Native `uuidv7()` as the identity-generation mechanism
while the floor remains 15 — see ADR 0019, which keeps identity
generation in the application layer specifically so this is not a forced
coupling.

**Security.** None distinct.

**License.** N/A.

## Follow-Up

- [ ] Confirm the test-database plan (ADR 0029 follow-up) actually
      exercises at least the minimum (15) and recommended (17) versions
      before Phase 5B-5 integration tests are considered complete.
- [ ] Revisit this range when PostgreSQL 14's 2026-11-12 end-of-life
      passes, to confirm no accidental 14-only dependency crept in.
