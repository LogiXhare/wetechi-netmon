# 0031. Phase 5B Durable Time Semantics

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

Verified against source: `crate::clock::Timestamp` is
`{ monotonic: Instant, wall: SystemTime }`
(`crates/incident/src/clock.rs`). Its only public
constructor is `Timestamp::now(clock)`. Every decision that matters for
correctness — `ReopenPolicy::reopens` (BQ-9's 15-minute inclusive
window), `Suppression::is_active`, `elapsed_since`, `is_before` — compares
the **monotonic** (`Instant`) half, deliberately, because `Instant`
cannot go backward under a wall-clock correction (matching the
detector's own monotonic-only philosophy,
`crates/detector/src/clock.rs`).

`std::time::Instant` is defined by the standard library as opaque and
**process-local** — it has no fixed epoch and no cross-process meaning.
It cannot be serialized to PostgreSQL, and a value read back after a
process restart is not comparable to one taken before it. Persist-then-
reload therefore breaks 5A's own reopen-window and suppression-expiry
logic if the monotonic comparison is naively carried forward. This is
the single largest semantic gap between the approved 5A domain and a
durable store, larger than any dependency selection in this planning
pass.

## Options Considered

### Option A — PostgreSQL `transaction_timestamp()` as the authoritative decision time for durable state transitions; `Instant` never persisted, kept only for process-local latency/timeout measurement

- Pros: a single, database-assigned wall-clock instant per transaction
  is comparable across processes and across restarts, which `Instant`
  fundamentally cannot be; `transaction_timestamp()` is stable within one
  transaction (unlike `clock_timestamp()`, which advances during a long
  transaction) — the right primitive for "the instant this transition
  was decided"; keeps the detector's monotonic-first philosophy for what
  it is actually good at (in-process latency, timeout enforcement)
  without asking it to do a job (durable ordering across restarts) it
  was never suited for.
- Cons: a real behavioral change from 5A's exact comparison — a
  wall-clock instant can, in principle, be affected by NTP correction
  in a way a monotonic instant cannot, so this ADR must define what
  happens on skew (below) rather than silently inheriting 5A's
  "monotonic never goes backward" guarantee.

### Option B — Persist `SystemTime` and attempt to reconstruct an `Instant`-equivalent comparison after reload

- Pros: superficially closer to "just persist what 5A already computes."
- Cons: **not possible as stated, and this task explicitly forbids
  implying it is.** There is no general mapping from a persisted
  `SystemTime` back to a new process's `Instant` epoch — `Instant` has no
  fixed reference point across process boundaries by design. Any
  implementation claiming to do this would be quietly falling back to a
  wall-clock comparison anyway, while advertising a monotonic guarantee
  it cannot actually provide. Rejected as a category error, not merely a
  worse option.

### Option C — Application-server wall-clock time (`chrono::Utc::now()` or equivalent) as the decision time, computed before the transaction starts

- Pros: avoids a round-trip to ask PostgreSQL for its own clock.
- Cons: multiple application server instances, and PostgreSQL itself,
  can each have slightly different wall clocks; using the *database's*
  transaction timestamp as authoritative removes that source of
  disagreement entirely — the database is already the single source of
  truth for every other piece of state in this design
  ([incident-persistence.md](../incident-persistence.md)'s "one
  authority for any given fact"), and time should be no different.

## Decision

**Option A.**

- **Durable UTC timestamps**, sourced from PostgreSQL's
  `transaction_timestamp()`, are authoritative for: reopen decisions,
  suppression expiry, every lifecycle timestamp
  (`opened_at`/`resolved_at`/`closed_at`/etc.), closure eligibility, and
  every `occurred_at`/`recorded_at` pair on timeline and audit entries
  (see [ADR 0027](0027-phase5b-durable-record-identity.md) for their
  identity, this ADR for their time).
- **Monotonic (`Instant`) time is never persisted and never restored.**
  It remains available only for process-local latency measurement and
  timeout enforcement (e.g. "has this connection-acquire attempt taken
  too long") — the same role the detector already uses it for, scoped
  correctly to what a single process's uptime can answer.
- **Clock-skew behavior, stated explicitly:** if the database's decision
  timestamp for a new event is *earlier* than the persisted reference
  timestamp the decision is being compared against (e.g. a reopen
  decision computing a negative or implausible elapsed duration), the
  system must:
  - **not clamp silently** to zero or to the reference time,
  - **not reopen** on the strength of an unreliable comparison,
  - **not create a duplicate incident** either, since that could be the
    worse outcome of two bad defaults,
  - **return a structured clock-skew or retryable error** instead, so
    the caller (the correlation worker or the operator-facing service)
    can decide — retry, alert, or surface to an operator — rather than
    the persistence layer silently guessing.
  - **emit a bounded metric and a structured log entry** for the event,
    so recurring skew is visible operationally rather than only
    discoverable by an incident's timestamps looking wrong after the
    fact.
- The reopen-window and suppression-expiry *policies themselves*
  (`ReopenPolicy::reopens`, `Suppression::is_active`) are unchanged in
  their comparison logic (inclusive `<=`, strict `<`) — only the
  timestamp representation they compare against changes from
  process-local `Instant` to durable UTC.

## Consequences

**Easier.** Reopen and suppression decisions become comparable across
process restarts, which they fundamentally could not be under 5A's
`Instant`-based design — this is required, not optional, for a durable
store.

**Harder.** This is a genuine semantic change to code the final sanity
review certified as correct for its (in-memory, single-process,
never-restarted) scope. It must be implemented and tested as its own
unit of work at Phase 5B-0
([ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md)), not
folded silently into the schema migration where a reviewer might mistake
it for a mechanical port.

**Forecloses.** A design where `Instant` is trusted for anything beyond
single-process latency measurement.

**Security.** Clock-skew abuse (an attacker or a misconfigured NTP
client causing a database's or application's wall clock to disagree with
reality) is now an explicit threat-model entry — see
[incident-threat-model.md](../../security/incident-threat-model.md).

**License.** N/A.

## Follow-Up

- [ ] Implement `Incident::reconstitute`
      ([ADR 0030](0030-phase5b-aggregate-reconstitution.md)) to accept a
      durable UTC timestamp representation, not an `Instant`, for every
      restored field.
- [ ] Add the clock-skew integration test (an event whose
      `transaction_timestamp()` predates the persisted reference)
      to the Phase 5B-5 test plan.
- [ ] Add the bounded clock-skew metric to the observability plan.
- [ ] Close **FU-35** (durable timeline/audit timestamps) against this
      ADR plus [ADR 0027](0027-phase5b-durable-record-identity.md).
