# 0019. Phase 5B UUIDv7 Identity Generation

Status: **Conditionally Accepted** — pending the Phase 5B-1 dependency
probe (see [ADR 0018](0018-phase5-dependency-selection.md) and
[dependency-license-matrix.md](../../dependency-license-matrix.md))
Date: 2026-08-24
Deciders: Repository owner

## Context

[ADR 0013](0013-incident-identity.md) recommended UUIDv7 for
`incident_id`, conditional on BQ-5 and BQ-7. Both are resolved. Phase 5A
shipped a placeholder generator behind `IncidentGenerator`
(`crates/incident/src/id.rs`) explicitly documented as
"not UUIDv7 and not cryptographically unpredictable," existing only so
5A could create incidents without the `uuid` crate. Phase 5B needs a real
UUIDv7 generator for the durable primary key PostgreSQL will store.

ADR 0018's honesty constraint applies: the crate must be verified against
the live registry, not recalled.

## Verified evidence (2026-08-24)

- **Version:** 1.25.0, released 2026-08-22 (two days before this
  decision).
- **License:** Apache-2.0 OR MIT.
- **Advisories:** zero, in the crate's entire history, per the RustSec
  advisory-db source repository (`crates/uuid` has no directory under
  `rustsec/advisory-db`).
- **Downloads:** 170M+ recent — among the most widely used crates in the
  ecosystem.
- **Repository:** <https://github.com/uuid-rs/uuid>, active.

**Requires implementation-time verification, not yet measured:** exact
transitive closure with `features = ["v7"], default-features = false`,
`cargo tree` output, `cargo audit` result, `unsafe` inventory, Windows-GNU
and Linux build results, MSRV under the selected feature set.

## Options Considered

### Option A — `uuid` crate, `v7` feature only

- Pros: RFC 9562 compliant, canonical byte order, dual-licensed, zero
  advisory history, directly produces `[u8; 16]` compatible with 5A's
  opaque `IncidentId` representation, widely audited by the ecosystem.
- Cons: a new dependency; exact transitive count not yet measured.

### Option B — PostgreSQL-generated `uuidv7()`

- Pros: no Rust dependency at all.
- Cons: **confirmed new in PostgreSQL 18 only** (2026-08-24 research).
  Requiring it would force a PostgreSQL 18 minimum, contradicting
  [ADR 0025](0025-phase5b-postgresql-version-support.md)'s 15/17 range,
  and moves identity generation outside the domain's testable seam —
  every existing `IncidentGenerator` unit test would need a live
  database.

### Option C — `ulid`

- Pros: also time-ordered.
- Cons: not RFC 9562. [ADR 0013](0013-incident-identity.md) specifies
  UUIDv7 by name; switching format is a re-litigation of a resolved
  decision, not a dependency substitution.

### Option D — custom generation (no crate)

- Pros: zero dependency, matches BQ-5's original hesitation.
- Cons: BQ-5 conditionally approved `uuid` specifically to avoid
  hand-rolling RFC 9562 timestamp-and-randomness packing correctly.
  5A's placeholder generator already demonstrates the crate-free
  approach is available as a fallback, not that it should be preferred.

## Decision

**Option A, conditionally.** `uuid = { version = "1.25", default-features
= false, features = ["v7"] }`, placed **only** inside the future
`crates/incident-postgres` adapter
([ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md)), behind
the existing `IncidentGenerator` trait. `crates/incident` gains no new
dependency and no `uuid::Uuid` type crosses its public API — `IncidentId`
stays the opaque `[u8; 16]` it already is.

Conditional on the Phase 5B-1 probe (§ above) returning a clean result.
If the probe finds an unacceptable transitive closure or a Windows-GNU
build failure, this ADR is revisited before any implementation commit.

## Consequences

**Easier.** Chronologically sortable primary keys avoid the B-tree
fragmentation a random UUIDv4 would cause on `incidents(incident_id)`.
Round-tripping through PostgreSQL's native `uuid` column type is direct.

**Harder.** One more dependency to track through every future `cargo
audit` run.

**Forecloses.** Nothing — the trait seam means a future replacement
(including PostgreSQL 18's native `uuidv7()`, once the version floor
rises) is a generator swap, not a domain change.

**Security.** UUIDv7 embeds a creation timestamp. Per
[ADR 0013](0013-incident-identity.md), `incident_id` is already
documented as not a secret and not an authorization boundary, so this is
not a new exposure — but it must not be used as an external-facing
identifier where creation-time leakage is undesirable, consistent with
the existing 404-not-403 tenant-isolation rule.

**License.** Apache-2.0 OR MIT is compatible with the Apache-2.0 core.
Row pending in [dependency-license-matrix.md](../../dependency-license-matrix.md).

## Follow-Up

- [ ] Run the Phase 5B-1 dependency probe (measured `cargo tree`,
      `cargo audit`, `unsafe` inventory, Windows-GNU + Linux build) —
      entry gate for implementation.
- [ ] Add the completed matrix row before the crate is added to any
      `Cargo.toml`.
- [ ] Implement the concrete generator in `crates/incident-postgres`
      behind `IncidentGenerator`, with the same "refuse rather than
      repeat" contract 5A's placeholder generator already tests.
