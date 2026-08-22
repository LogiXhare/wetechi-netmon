# 0014. The Incident State Machine Does Not Mirror the Detector's

Status: Proposed — **partly blocked on BQ-6, BQ-8, BQ-9**
Date: 2026-08-22
Deciders: Repository owner (pending review)

## Context

[FR-5.1](../../functional-requirements.md) specifies one state machine:

```text
Normal → Suspected → Confirmed → AwaitingApproval → MitigationPending
       → Mitigating → HoldDown → Recovering → Closed / Failed
```

Two problems make this un-implementable as written in Phase 5.

**First, its opening states already exist in Phase 4.** `Suspected` and
`Confirmed` are `PendingTrigger` and `Active`. The `triggerFor` timer
*is* the suspected-to-confirmed transition, and it is implemented,
tested, and tunable per policy. Re-implementing it in the incident domain
would mean two components deciding the same thing from the same data,
with no rule for which wins when they disagree.

**Second, four of its states are mitigation states.**
`AwaitingApproval`, `MitigationPending`, `Mitigating`, and `HoldDown`
describe a mitigation lifecycle that Phase 5 is explicitly forbidden from
implementing and that Phase 7 will own.

A third, subtler force: incident state is about *human response*, not
traffic. An incident can be acknowledged while the flood continues, and
investigated long after it stops. A machine driven by both traffic and
people needs a clear rule about which drives which, or transitions become
unpredictable.

## Options Considered

### Option A — Implement FR-5.1 literally

- Pros: matches the requirement exactly; no reconciliation needed later.
- Cons: duplicates Phase 4 logic in a second place; requires four states
  Phase 5 must not implement, which would be dead code that lies about
  capability — precisely the failure ADR 0007 and the `executed` field
  exist to prevent.

### Option B — Operator-facing states only, mitigation deferred

Detection keeps `Suspected`/`Confirmed`; incidents model human response;
mitigation states arrive in Phase 7.

- Pros: no duplicated logic; every state is reachable and meaningful; the
  detector stays the single authority on whether traffic is abnormal;
  Phase 7 extends rather than rewrites.
- Cons: deviates from FR-5.1 as written; needs the deviation documented
  and approved; Phase 7 must add states to a live machine.

### Option C — Full machine with unreachable mitigation states

Define all FR-5.1 states, leave the mitigation ones unreachable.

- Pros: FR-5.1 satisfied on paper; no migration in Phase 7.
- Cons: an API that advertises `Mitigating` in its enum while nothing can
  ever mitigate is a capability lie of the same kind the `executed` field
  was added to prevent; consumers write handling for states that never
  occur.

## Decision

**Option B**, with the deviation from FR-5.1 recorded explicitly and
**BQ-6** raised so the owner can accept it or choose Option C.

Eight states: `Open`, `Acknowledged`, `Investigating`, `Monitoring`,
`Recovering`, `Resolved`, `Closed`, `Suppressed`. `Closed` is the only
terminal state. Full matrix in
[the state machine document](../incident-state-machine.md).

Key sub-decisions:

- **`Suspected` and `Confirmed` stay in the detection domain.** An
  incident is created only once the detector has confirmed. There is no
  incident for a suspicion.
- **`Failed` is not an incident state.** In FR-5.1 it means mitigation
  failed — a Phase 7 concept. A *processing* failure is a quarantined
  event plus an alert, not an incident state. Inventing semantics for it
  now would only constrain Phase 7.
- **Automatic and operator transitions are separated** and listed
  separately, so it is always clear whether traffic or a person caused a
  change.
- **Illegal transitions are refused by a guard**, following Phase 4's
  `DetectionState` pattern, so an unreasoned edge fails loudly instead of
  entering a state whose timers mean nothing.
- **Recovery restores the prior state on abort.** An incident that was
  `Investigating` when traffic dipped returns to `Investigating`.
- **The five ways a detection can end are recorded distinctly** —
  `traffic_cleared`, `detector_stale`, `detector_silent`,
  `policy_withdrawn`, `detector_reset`. All converge on `Recovering`, but
  an operator must be able to tell "the attack stopped" from "we stopped
  hearing about it".
- **No transition triggers a notification or a mitigation.** Transitions
  emit outbox events; nothing consumes the notification and mitigation
  types in Phase 5.

Timing defaults — 5-minute recovery confirmation, 15-minute reopen
window, 24-hour auto-close — are **recommendations**. **BQ-8** (must
critical incidents be closed manually?) and **BQ-9** (default reopen
window?) are the owner's.

## Consequences

**Easier.** One authority for "is traffic abnormal". Every state is
reachable and means something. Operator workflow is modelled on its own
terms. Phase 7 adds mitigation states without unpicking anything.

**Harder.** A documented deviation from FR-5.1 that needs approval.
Phase 7 will add states to a machine already carrying live incidents,
which means a migration and a version bump on the incident schema.
Reasoning about an incident requires looking at both machines.

**Forecloses.** Little. Adding states later is additive. What it does
foreclose is the incident manager ever deciding on its own that traffic
is abnormal — that stays the detector's job, permanently.

**Security.** Every transition carries a permission and writes an audit
record. Suppression requires a mandatory expiry, because an indefinite
suppression is how a real attack is missed. Severity reduction requires a
reason and is separately permissioned.

**Operational.** Five distinct recovery reasons mean an operator can tell
a genuine recovery from telemetry loss — the difference between closing
an incident correctly and closing one that is still happening.

## Follow-Up

- [ ] **BQ-6** — accept the FR-5.1 deviation, or choose Option C.
- [ ] **BQ-8** — manual closure for critical incidents?
- [ ] **BQ-9** — default reopen window.
- [ ] Update [functional-requirements.md](../../functional-requirements.md)
      FR-5.1 to reference this ADR once approved, so the requirement and
      the design stop disagreeing.
