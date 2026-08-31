# 0028. Phase 5B Durable Idempotency Fingerprint

Status: **Accepted** — no new dependency required
Date: 2026-08-24
Deciders: Repository owner

## Context

[ADR 0016](0016-incident-concurrency-and-idempotency.md) already fixed
the idempotency *semantics* (same key/same fingerprint replays; same
key/different fingerprint is `409 incident.idempotency_key_reuse`). Phase
5A's `RequestFingerprint::of` (`crates/incident/src/idempotency.rs`)
implements this today as `serde_json::to_vec(&(incident_id, &command))`
— the canonical JSON bytes of the command plus its target, stored and
compared directly. This ADR decides whether that mechanism is durable as
designed or needs to change for PostgreSQL persistence.

## Options Considered

### Option A — Persist the canonical bytes directly (5A's mechanism, unchanged)

- Pros: **zero new dependency** — no hashing crate needed;
  bit-for-bit comparison is simpler to reason about than a hash
  (no collision possibility to even consider); every command 5A defines
  is already bounded (`TITLE_MAX_LEN`, `DESCRIPTION_MAX_LEN`,
  `NOTE_BODY_MAX_LEN`, `SUPPRESSION_REASON_MAX_LEN`, and the smaller
  fixed-shape commands), so the fingerprint's stored size is bounded by
  those same limits, not unbounded; matches this crate's own module doc,
  which already states this is "not a hash" as a deliberate choice.
- Cons: a large command body (bounded, but the bound is generous —
  `DESCRIPTION_MAX_LEN` is 8,000 characters) stores that many bytes
  per idempotency record, rather than a fixed-size hash.

### Option B — Cryptographic hash (e.g., SHA-256) of the canonical bytes

- Pros: fixed-size fingerprint regardless of command size.
- Cons: **a new dependency for no demonstrated need.** This task's own
  instruction is explicit: "If cryptographic hashing is proposed, verify
  the dependency and explain why it is needed." No requirement in this
  design needs collision resistance — idempotency-key reuse detection
  needs *equality*, not a security property, and a hash only trades a
  bounded byte comparison for an unbounded (if small) probability of
  a false-positive replay match. Rejected: it adds a dependency to solve
  a problem (storage size) that does not exist given the existing
  bounds, at the cost of a problem (hash collision, however remote) that
  did not exist before.

### Option C — Non-cryptographic fast hash (e.g., a checksum crate)

- Pros: smaller than Option B's dependency concern, still bounds storage
  size.
- Cons: same objection as Option B in kind, smaller in degree — still an
  unjustified new dependency against Option A's zero-dependency,
  already-bounded alternative. Not selected.

## Decision

**Option A: persist the canonical JSON bytes directly, unchanged from
5A's mechanism.** `incident_idempotency.request_fingerprint` is a
`BYTEA` column storing exactly what `RequestFingerprint::of` already
produces. No hashing dependency is introduced.

If a future command type's bound grows large enough that per-record
storage becomes a measured operational concern (not the case for any
command 5A defines), that is a new ADR with a measured cost, not a
default assumption now.

## Consequences

**Easier.** Zero new dependency; the durable fingerprint is provably
identical to what 5A already computes and tests, so no new fingerprint-
equivalence risk is introduced by persistence.

**Harder.** Nothing identified — this is the status-quo mechanism made
durable, not a new design.

**Forecloses.** Nothing — a hash-based fingerprint remains available
later if a specific, measured need arises.

**Security.** No cryptographic property is claimed or needed for this
use — idempotency keys are already documented (ADR 0016) as not
credentials and not authorization boundaries; the fingerprint's job is
change detection, not tamper resistance.

**License.** N/A.

## Follow-Up

- [ ] Confirm `incident_idempotency.request_fingerprint` is `BYTEA`, not
      `TEXT`, at Phase 5B-2 schema implementation — the value is already
      JSON-serialized bytes, not text requiring further encoding.
- [ ] Retention remains 24 hours per ADR 0016 (technical default, not a
      legal claim).
