# 0026. Phase 5B Transaction Isolation Model

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

[incident-persistence.md](../incident-persistence.md) already specifies
a transaction-boundary rule ("state change, timeline, audit, and outbox
all commit, or none of it") but does not fix an isolation level. Multiple
concurrent actors touch the same aggregate: the correlation worker
(detection-event linking, reopen, create), and operators (state
transitions, severity, assignment). ADR 0016 already fixed optimistic
`version` concurrency for operator commands and re-read-and-re-decide for
the correlator; this ADR fixes the *database* isolation level and lock
strategy those decisions run on top of.

## Options Considered

### Option A — Read Committed everywhere, plus explicit constraints and locks per operation

- Pros: PostgreSQL's default, well-understood performance
  characteristics, no serialization-failure retry storm under normal
  load; correctness comes from the database's own constraints (the
  partial unique indexes, `UNIQUE (tenant_id, dedup_key)`) and explicit
  row locks (`SELECT … FOR UPDATE`) exactly where a race is possible,
  not from a blanket isolation guarantee papering over undesigned races.
- Cons: requires actually identifying every race and giving it an
  explicit lock or constraint — more design work up front than "just use
  Serializable everywhere."

### Option B — Serializable for every operation

- Pros: the database detects any conflicting concurrent transaction
  automatically; less per-operation race analysis needed.
- Cons: **explicitly rejected by this task's own instruction** ("do not
  default to Serializable without operational justification"); Phase
  5A's own domain design already re-reads and re-decides for the
  correlator specifically because "the correct response to 'the incident
  changed under me' is to re-evaluate... not to force through a decision
  made against stale state" — Serializable's automatic conflict
  detection does not replace that re-evaluation, it just adds
  unconditional retry overhead on top of a design that already handles
  the race deliberately. Serializable also multiplies retry frequency
  under real concurrent load, which is a cost this project has not
  benchmarked a need for.

### Option C — Repeatable Read selectively

- Pros: stronger snapshot guarantee than Read Committed for
  read-then-decide sequences, without Serializable's full conflict
  detection cost.
- Cons: still does not replace the explicit locks and constraints Option
  A already requires for the genuinely racy operations; adds a second
  isolation level to reason about without a specific operation that
  needs it over Option A's targeted locking. Not selected as a default,
  but available per-operation if a specific future case justifies it.

## Decision

**Option A: Read Committed as the default isolation level for every
operation category**, with correctness enforced by constraints and
explicit locks rather than a stronger blanket isolation level.

### Operation isolation matrix

| Operation | Isolation | Locks / constraints relied on | Retryable SQLSTATE | Max retries | Conflict returned to caller |
|---|---|---|---|---|---|
| Detection ingestion (dedup) | Read Committed | `UNIQUE (tenant_id, dedup_key)` — insert *is* the check | `23505` (treated as duplicate, not an error) | 0 (not a retry, a normal outcome) | n/a — recorded as duplicate |
| Incident creation (race) | Read Committed | Target-specific partial unique index (§ [incident-persistence.md](../incident-persistence.md)'s "Active-incident invariant" schema — not ADR 0032, which covers tenant isolation, not this index; e.g. `incidents_active_host`) | `23505` | 3 | Retry into the update/link path, per [incident-persistence.md](../incident-persistence.md) |
| Event linked to open incident | Read Committed | Row read by primary key, `version` optimistic check on write | `40001` (rare at RC, but possible under concurrent linking) | 3 | `409` with current version |
| Reopen-candidate selection and reopen | Read Committed | `SELECT … FOR UPDATE` on the selected candidate row | `40001`, `40P01` | 3 | If lost, re-evaluate against the now-current state (may become a link or a fresh create) |
| Operator command (state, severity, priority, assignment, closure, suppress) | Read Committed | Optimistic `expected_version` match (ADR 0016) | `40001` on the update statement itself is unlikely at RC; a zero-row update is not an error, it is `409` | 0 retries — a version conflict is returned to the caller, never silently retried, per ADR 0016 | `409` with current version and state |
| Note append, tag add/remove | Read Committed | Append-only or set-semantic; ADR 0016 already excludes these from `expected_version` | n/a | n/a | n/a |
| Outbox claim (future consumer) | Read Committed | `SELECT … FOR UPDATE SKIP LOCKED` | `40P01` unlikely with `SKIP LOCKED` | 3 | Row remains `pending` for the next claimer |
| Idempotency check-then-record | Read Committed | `PRIMARY KEY (tenant_id, idempotency_key)` — insert conflict *is* the "already exists" signal | `23505` | 0 — a conflict here is read back and classified per ADR 0016's table (replay / in-progress / key-reuse), not retried | Per ADR 0016 (200 replay, 409 in-progress, 409 key-reuse) |

**Bounded retries: 3 attempts, exponential backoff with jitter, then the
conflict is returned to the caller.** No infinite retry loop under any
circumstance — a database outage must fail closed with `503`, per
[incident-persistence.md](../incident-persistence.md)'s failure table,
not retry forever.

**Never retried:** validation failures, authorization failures, tenant
mismatch, deterministic idempotency conflicts (key reuse with a
different fingerprint), unsupported schema version. These are not
transient — retrying them wastes a database round-trip to get the same
answer.

## Consequences

**Easier.** Read Committed's lower overhead and better-understood
failure modes than Serializable under real concurrent load; each race
gets an explicit, reviewable defense instead of relying on an isolation
level to catch what design missed.

**Harder.** Every operation category needed its own race analysis (the
table above) rather than one blanket rule — that analysis is now done,
but a *new* operation added later must extend this table deliberately,
not assume Read Committed alone is safe.

**Forecloses.** Nothing — a specific operation can be given a stronger
isolation level later if a real failure demonstrates the need, per
Option C's note.

**Security.** Bounded retries prevent a retry storm from becoming a
self-inflicted denial of service during a database outage, consistent
with R6 in [risk-register.md](../../risk-register.md).

**License.** N/A.

## Follow-Up

- [ ] Add a failure-injection integration test per row of the matrix
      above at Phase 5B-5, proving the stated retry/conflict behavior
      rather than asserting it from this document alone.
- [ ] Extend the matrix explicitly whenever a new command category is
      added.
