# 0030. Phase 5B Aggregate Reconstitution

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

Verified against source: `Incident` derives only `Debug, Clone,
PartialEq`. It has no `Serialize`/`Deserialize` implementation and no
public constructor. Two fields the domain logic itself depends on —
`state_before_recovering` (drives `abort_recovery`) and `matched_metrics`
(drives category derivation) — are `pub(crate)` with no accessor
(`crates/incident/src/incident.rs:109,115`). A row
read back from PostgreSQL cannot become a valid `Incident` today by any
means available outside `crates/incident` itself.

This is a real design problem, not a missing derive: `Incident`'s own
module doc states its invariant is "every mutation... goes through a
method with intent to change it, and every path they use also appends a
timeline entry, an audit entry, and bumps `version`." A bare `#[derive(Deserialize)]`
on `Incident` would let a persistence adapter construct an `Incident` in
any state whatsoever — including states the 20-commit adversarial-review
history spent its Blocker and High findings specifically preventing
(illegal transitions, `ever_critical` unset for a Critical incident,
etc.) — bypassing every guard `IncidentUnitOfWork` currently enforces on
construction.

## Options Considered

### Option A — A separate `IncidentSnapshot` DTO plus a controlled `Incident::reconstitute` constructor that validates invariants

- Pros: the wire/row format (`IncidentSnapshot`, serializable, owned by
  the persistence layer's needs) stays decoupled from the domain type's
  internal representation, so a schema-shape change does not force a
  domain-type change and vice versa; `reconstitute` is the **one** path
  by which a `pub(crate)`-shaped internal state becomes a live
  `Incident`, and it can assert the same invariants construction inside
  `crates/incident` already relies on (e.g. `ever_critical` must be true
  if `severity == Critical` and the row claims otherwise is a corrupt-
  data error, not a silent acceptance); matches the "typed domain, JSON
  only at the boundary" rule this crate already documents for
  `Suppression::to_display` and the timeline/audit payloads.
- Cons: a snapshot type is one more thing to keep in sync with
  `Incident`'s field set as it evolves — an accepted cost, mitigated by
  a round-trip test asserting every `Incident` field has a corresponding
  `IncidentSnapshot` field.

### Option B — `#[derive(Serialize, Deserialize)]` directly on `Incident`

- Pros: no new type.
- Cons: **rejected outright, per this task's explicit instruction**
  ("no arbitrary Deserialize directly into Incident"). A blind
  `Deserialize` accepts any combination of field values a `serde_json`
  payload happens to contain, with no invariant check — the exact
  bypass the two adversarial reviews' Blocker/High findings (B1, B2, R4)
  spent their effort closing at the *method* level would reopen at the
  *deserialization* level, silently.

### Option C — Public setters for every field, letting the adapter build an `Incident` field-by-field

- Pros: maximal flexibility for the adapter.
- Cons: **rejected outright, per this task's explicit instruction** ("no
  public mutable aggregate fields"). This is the same bypass as Option B
  in a different shape — a public `set_severity` with no accompanying
  guard is exactly the kind of setter `Incident`'s own module doc says
  does not exist.

## Decision

**Option A.** `crates/incident` gains:

- `IncidentSnapshot` — a plain, `Serialize`/`Deserialize` DTO mirroring
  `Incident`'s fields (including the currently-inaccessible
  `state_before_recovering` and `matched_metrics`), owned by the
  persistence boundary, not embedded in the domain's own mutation logic.
- `Incident::reconstitute(snapshot: IncidentSnapshot) -> Result<Incident,
  IncidentError>` — the **only** path from a snapshot to a live
  `Incident`. It validates invariants a corrupt or hand-crafted row
  could otherwise violate (structurally impossible states — e.g. a
  `state_before_recovering` set while `state != Recovering` — return an
  error rather than a silently-accepted invalid aggregate). The exact
  invariant list is Phase 5B-0 implementation work, informed by the
  guards `IncidentUnitOfWork`'s existing mutation methods already
  enforce.
- No `pub fn` returning `&mut Incident` or any of its fields is added.
  `IncidentSnapshot` is produced by a read-only accessor, not a mutable
  handle.

`crates/incident-postgres` ([ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md))
maps PostgreSQL rows to `IncidentSnapshot`, then calls `reconstitute`.
No PostgreSQL row type crosses into `crates/incident`.

## Consequences

**Easier.** A row from PostgreSQL becomes a domain object through the
same kind of validated path every command already uses to mutate one —
no new bypass class introduced by persistence.

**Harder.** `IncidentSnapshot` and `Incident` must be kept in sync as the
domain model evolves; a round-trip test is the safeguard, not manual
discipline alone.

**Forecloses.** Nothing — `IncidentSnapshot`'s shape is free to differ
from the eventual PostgreSQL column layout; the adapter, not this ADR,
owns that mapping.

**Security.** This is the primary defense against a corrupted or
maliciously-crafted database row producing an `Incident` the domain's own
guards would never have permitted to exist — directly relevant given
[incident-security-model.md](../incident-security-model.md)'s threat
model already treats the database as part of the trusted boundary but
not as infallible.

**License.** N/A.

## Follow-Up

- [ ] Implement `IncidentSnapshot` and `reconstitute` at Phase 5B-0,
      before any SQL exists.
- [ ] Add the field-parity round-trip test (every `Incident` field has a
      corresponding `IncidentSnapshot` field and back) as a compile-time
      or test-time check, not manual review alone.
- [ ] Enumerate the specific structural invariants `reconstitute` checks,
      cross-referenced against the guards `IncidentUnitOfWork`'s
      existing mutation methods enforce.
