# 0027. Phase 5B Durable Record Identity (Timeline, Audit, Outbox)

Status: **Accepted** — no new dependency required
Date: 2026-08-24
Deciders: Repository owner

## Context

**FU-39** records that 5A's three sequence counters
(`timeline_sequence`, `audit_sequence`, `outbox_sequence`) use
`saturating_add` and are explicitly "not suitable as durable identities"
— they were deliberately left unhardened in 5A because hardening them
would have required threading `Result` through ~18 call sites,
reintroducing exactly the post-write-failure risk FU-31 confirmed does
not currently exist. Phase 5B replaces the in-memory counters with real
persistence, which removes that constraint: a database sequence or
identity column does not share the in-memory counter's overflow-then-
silently-repeat failure mode.

This is a distinct decision from `incident_id`
([ADR 0013](0013-incident-identity.md), UUIDv7) and `incident_number`
(the amendment recorded in ADR 0013 itself) — those are the aggregate's
own identity and business-facing number. This ADR is about the three
supporting record types.

## Options Considered

### Option A — `BIGINT GENERATED ALWAYS AS IDENTITY` per table

- Pros: native PostgreSQL feature, **zero new Rust dependency**;
  monotonically increasing per table, giving `incident_timeline` and
  `incident_audit` a natural, efficient ordering index without a
  separate sequence-value round-trip from the application; simplest
  possible durable identity; no duplicate-identity risk (PostgreSQL's
  own sequence machinery, not an application-level counter).
- Cons: identity values are per-table, not globally unique across
  `incident_timeline`/`incident_audit`/`incident_outbox` — not a
  requirement anywhere in the approved design, so not a real cost.

### Option B — UUIDv7 per record

- Pros: consistent identity strategy with `incident_id`; globally
  unique across tables if that ever mattered.
- Cons: **adds no value here and one real cost**: `incident_timeline`
  and `incident_audit` need an efficient, naturally-ordered index for
  "everything for this incident, in order" queries — a `BIGINT` identity
  gives that directly, while UUIDv7's timestamp-ordering is coarser
  (millisecond) and the type is heavier (16 bytes vs. 8) across
  potentially the largest-row-count tables in the schema (every timeline
  and audit entry, indefinitely retained per the retention table).
  Reserved for a case that actually needs cross-table global uniqueness,
  which does not exist in this design.

### Option C — application-generated sequence (5A's pattern, hardened)

- Pros: reuses 5A's existing counter shape.
- Cons: reintroduces exactly the coordination problem a database
  identity column solves for free — a `checked_add`-hardened in-memory
  counter is still only correct for a single process; multiple
  correlation-worker instances would race on the same counter. Rejected
  once real persistence removes the reason 5A had for an in-memory
  counter at all.

## Decision

**Option A: `BIGINT GENERATED ALWAYS AS IDENTITY` for
`timeline_id`, `audit_id`, and `outbox_id`.** No new Rust dependency.
`IncidentId`/`IncidentNumber` keep their existing, separately-decided
strategies (UUIDv7 per ADR 0013/0019; continuous per-tenant sequence per
the ADR 0013 amendment). Durable record identity for supporting tables
does not need to match the aggregate's own identity strategy, and
choosing deliberately per table — rather than applying one strategy
everywhere by default — is the point of this ADR.

## Consequences

**Easier.** Closes FU-39's "not suitable as a durable identity" gap with
zero new dependency and a natural ordering index.

**Harder.** Nothing material — this is a strict improvement over 5A's
in-memory counters with no new cost identified.

**Forecloses.** A future requirement for globally-unique record
identifiers across timeline/audit/outbox (not currently a requirement
anywhere) would need its own follow-up, not silently assumed available.

**Security.** None distinct — these identifiers are already documented
elsewhere as non-secret, non-authorization-boundary values, consistent
with [ADR 0013](0013-incident-identity.md)'s treatment of `incident_id`.

**License.** N/A.

## Follow-Up

- [ ] Close **FU-39** explicitly against this ADR at Phase 5B-2 schema
      implementation.
- [ ] Confirm `BIGINT` (not `INTEGER`) is used everywhere — a 32-bit
      identity on an indefinitely-retained audit table is the kind of
      silent future limit this project's own philosophy
      (`checked_add`, never a silent wrap) argues against.
