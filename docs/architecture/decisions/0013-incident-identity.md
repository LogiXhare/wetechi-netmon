# 0013. Incident Identity and the Human-Readable Incident Number

Status: Proposed — **blocked on BQ-5**
Date: 2026-08-22
Deciders: Repository owner (pending review)

## Context

An incident needs two identities that serve genuinely different
audiences, and conflating them produces something that serves neither
well.

1. An **internal identifier** for database keys, API paths, and
   cross-references. It must be unique, index well, and be safe to expose
   in a URL.
2. A **human-readable number** an operator can read aloud on a bridge
   call without spelling out thirty-two hex characters.

There is a direct conflict to resolve. [FR-5.2](../../functional-requirements.md)
says the incident record must persist a **UUID**. But
[ADR 0009](0009-detection-event-identity.md) deliberately declined the
`uuid` crate for detection events, because it would have pulled in `rand`
and a random-number dependency the detector did not otherwise need, and
because the identifiers were correlation keys rather than secrets.

That reasoning does not transfer unchanged. The detector is a hot-path,
dependency-minimal crate; the incident manager will already depend on a
database driver and an HTTP stack. The cost of `uuid` there is
proportionally near zero. But the requirement and the precedent still
disagree, and the disagreement is the owner's to settle.

## Options Considered

### Option A — UUIDv4, internal, from the `uuid` crate

- Pros: satisfies FR-5.2 literally; universally understood; every
  database and client library handles it.
- Cons: random ordering means poor B-tree locality and index
  fragmentation on a table with heavy insert volume; a new dependency
  (see BQ-7).

### Option B — UUIDv7

- Pros: satisfies FR-5.2; **time-ordered**, so inserts land at the end of
  the index and locality is good; sortable by creation time; standard
  since RFC 9562.
- Cons: same new dependency; the embedded timestamp leaks creation time
  to anyone holding an ID, which for an incident ID is close to
  irrelevant since the API returns `opened_at` anyway.

### Option C — ULID

- Pros: time-ordered; compact Crockford base32; human-transcribable.
- Cons: does not satisfy an FR that says "UUID" without an argument; a
  less universal ecosystem than UUID.

### Option D — Database-generated `BIGSERIAL`

- Pros: no dependency at all; perfect index locality; smallest key.
- Cons: **enumerable**, which is a genuine security problem — incident
  `4172` implies `4171` exists, and cross-tenant enumeration becomes a
  counting exercise; requires a database round trip before the ID exists,
  which complicates the outbox write; does not satisfy FR-5.2.

### Option E — The Phase 4 dependency-free approach

Reuse the `DefaultHasher`-based scheme from ADR 0009.

- Pros: no new dependency; consistent with the existing codebase.
- Cons: `DefaultHasher` is explicitly not guaranteed stable across Rust
  releases, which is tolerable for an ephemeral detection identifier and
  **not** tolerable for a database primary key that must remain valid for
  years; 64 bits of hash is too few for a primary key; does not satisfy
  FR-5.2.

## Decision

**Recommended: Option B, UUIDv7**, for the internal `incident_id` —
subject to the owner resolving **BQ-5** and **BQ-7**.

UUIDv7 is the only option that satisfies FR-5.2 as written while
avoiding the index-fragmentation cost of v4 and the enumerability of a
serial. The dependency objection that drove ADR 0009 does not carry the
same weight in a crate that already has a database driver.

If the owner declines the dependency, the fallback is **Option D with a
non-sequential public reference**: `BIGSERIAL` internally, never exposed,
with a random public token in the API. That is more moving parts and is
not recommended, but it is workable.

### The human-readable number

Separate from the internal ID, and generated per tenant:

```text
WNM-<year>-<zero-padded sequence>
WNM-2026-000123
```

- `WNM` is fixed.
- The **sequence is per tenant per year**, from a PostgreSQL sequence or
  an atomic counter row, allocated inside the same transaction that
  creates the incident.
- Zero-padded to six digits, and allowed to overflow to seven rather than
  wrapping. A tenant exceeding 999 999 incidents in a year has a
  different problem, and silently reusing a number would be worse than an
  ugly one.

The year-based form is a recommendation, not a requirement. The
alternative worth considering is a continuous per-tenant sequence with no
year segment, which avoids the "does the sequence reset on 1 January?"
question entirely. Recorded for the owner as part of BQ-5.

**Tenant-scoped numbering leaks tenant volume** to anyone who sees two
numbers from the same tenant. This is accepted: incident numbers are
shown to people already inside that tenant's boundary. It is *not*
acceptable across tenants, which is why the sequence is per tenant and
not global.

### Both identifiers are identifiers, not secrets

Stated explicitly, as ADR 0009 did for detection events:

- Neither `incident_id` nor `incident_number` is an authentication
  credential.
- Neither is an authorization boundary. Knowing an incident ID must never
  grant access to it — every read is authorized against the caller's
  tenant and permissions, and a valid ID from another tenant returns
  **404, not 403**, so the API does not confirm existence to someone not
  entitled to know.
- Neither may be used as an idempotency key or a CSRF token.

## Consequences

**Easier.** Time-ordered primary keys keep insert locality good.
Operators get a number they can say out loud. FR-5.2 is satisfied without
argument.

**Harder.** A new dependency, subject to BQ-7 and needing a
[license-matrix](../../dependency-license-matrix.md) row. Per-tenant
sequence allocation must be inside the incident-creation transaction, so
a rolled-back creation does not burn a number — or, if a gap is
acceptable, that must be a stated decision rather than an accident.

**Forecloses.** Little. Changing the internal ID type after incidents
exist is a data migration, so this should be settled before Milestone 5B
writes the schema.

**Security.** Avoids the enumeration weakness of serial IDs. The
timestamp leak in UUIDv7 is immaterial here. Cross-tenant ID probing is
addressed by the 404-not-403 rule above and tested per the
[threat model](../../security/incident-threat-model.md) T-04.

**License.** `uuid` is MIT/Apache-2.0, compatible with the Apache-2.0
core, but must be added to the matrix before use.

## Follow-Up

- [ ] **BQ-5** — owner resolves UUID versus the ADR 0009 precedent.
- [ ] **BQ-7** — owner approves or refuses new dependencies for Phase 5.
- [ ] Add `uuid` to
      [dependency-license-matrix.md](../../dependency-license-matrix.md)
      if BQ-7 is approved.
- [ ] Decide whether the incident number resets annually.
