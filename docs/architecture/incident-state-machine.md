# Incident State Machine

Status: **Planning only.** Part of the
[Phase 5 plan](phase5-incident-management-plan.md). Decision recorded in
[ADR 0014](decisions/0014-incident-state-machine.md).

## Why this is not the detector's state machine

Phase 4 already has a five-state machine — `Idle`, `PendingTrigger`,
`Active`, `PendingClear`, `Cooldown` — driven entirely by traffic. It
answers "is the threshold currently crossed, and has it been crossed long
enough to believe?"

The incident machine answers a different question: "where is the *human
response* up to?" An incident can be `Acknowledged` while traffic is
still `Active`, and it can sit in `Investigating` long after traffic has
returned to normal. Mirroring the detector's states into the incident
domain would produce a machine where half the transitions are driven by
traffic and half by people, with no clear rule about which wins.

This is why **`Suspected` and `Confirmed` stay in the detection domain**.
They are `PendingTrigger` and `Active` under other names —
`triggerFor` *is* the suspected-to-confirmed transition, and it is already
implemented, tested, and tuned. An incident exists only once the detector
has confirmed; there is no incident for a suspicion.

FR-5.1 lists a single machine that includes both — `Normal → Suspected →
Confirmed → AwaitingApproval → MitigationPending → Mitigating → HoldDown
→ Recovering → Closed / Failed`. Phase 5 implements the operator-facing
half. The mitigation states are **deferred to Phase 7**, not silently
dropped: see **BQ-6**, which asks the owner whether they should be absent
or present-but-unreachable.

## States

| State | Meaning | Terminal? |
|---|---|---|
| `Open` | Created, nobody has taken it | No |
| `Acknowledged` | A human has taken responsibility | No |
| `Investigating` | Active work in progress | No |
| `Monitoring` | Understood, being watched, no active work | No |
| `Recovering` | Traffic has cleared; confirmation period running | No |
| `Resolved` | Recovery confirmed or operator-resolved | No |
| `Closed` | Finished; the only terminal state | **Yes** |
| `Suppressed` | Known-noise, deliberately muted | No |

`Failed` from FR-5.1 is deliberately **not** an incident state. In FR-5.1
it means "mitigation failed", which is a Phase 7 concept. An incident
whose *processing* fails is not a state — it is a quarantined event plus
an alert. Inventing a `Failed` incident state now would mean inventing
semantics for it that Phase 7 would then have to change.

## Transition matrix

Rendered as a list rather than a wide table, so it stays readable in both
MkDocs and a plain-text diff. Every transition names its command, the
permission required, and its mandatory side effects.

### Automatic transitions

Driven by detection events or the clock. Actor is `system:correlator`.
No permission check — the correlator is not an operator — but every one
still writes a timeline entry and an audit record.

```text
(none) ──────────────► Open           on: first qualifying detection event
Open ────────────────► Recovering     on: Ended event, reason ClearSustained
Acknowledged ────────► Recovering     on: Ended event, reason ClearSustained
Investigating ───────► Recovering     on: Ended event, reason ClearSustained
Monitoring ──────────► Recovering     on: Ended event, reason ClearSustained
Open ────────────────► Recovering     on: staleness sweep, reason detector_silent
Acknowledged ────────► Recovering     on: staleness sweep, reason detector_silent
Investigating ───────► Recovering     on: staleness sweep, reason detector_silent
Monitoring ──────────► Recovering     on: staleness sweep, reason detector_silent
Recovering ──────────► Resolved       on: recovery confirmation window elapsed
Recovering ──────────► (prior state)  on: new qualifying event — recovery aborted
Resolved ────────────► Closed         on: auto-close delay, if enabled
Closed ──────────────► Open           on: qualifying event within reopen window
```

`Recovering → (prior state)` restores the state the incident held before
recovery began, which is why `state_before_recovering` is stored. An
incident that was `Investigating` when traffic briefly dipped returns to
`Investigating`, not to `Open` — losing the operator's progress because
traffic flickered would be a bug.

`Suppressed` is deliberately absent from every automatic transition. A
suppressed incident absorbs events silently: they are linked and counted,
metrics update, and no state changes. That is the entire point of
suppression, and an automatic transition out of it would defeat it.

### Operator transitions

| Command | From | To | Permission |
|---|---|---|---|
| `AcknowledgeIncident` | `Open` | `Acknowledged` | `incident.acknowledge` |
| `BeginInvestigation` | `Open`, `Acknowledged`, `Monitoring` | `Investigating` | `incident.investigate` |
| `MarkMonitoring` | `Acknowledged`, `Investigating` | `Monitoring` | `incident.investigate` |
| `ResolveIncident` | `Open`, `Acknowledged`, `Investigating`, `Monitoring`, `Recovering` | `Resolved` | `incident.resolve` |
| `CloseIncident` | `Resolved`, `Suppressed` | `Closed` | `incident.close` |
| `ReopenIncident` | `Closed`, `Resolved` | `Open` | `incident.reopen` |
| `SuppressIncident` | `Open`, `Acknowledged`, `Investigating`, `Monitoring` | `Suppressed` | `incident.suppress` |
| `UnsuppressIncident` | `Suppressed` | `Open` | `incident.suppress` |

Non-transitioning commands, which mutate the incident without changing
state: `AssignIncident`, `UnassignIncident`, `ClaimIncident`,
`ReleaseIncident` (`incident.assign`); `AddNote`
(`incident.note.create`); `ChangeSeverity`
(`incident.severity.change`); `ChangePriority`
(`incident.priority.change`); `AddTag` / `RemoveTag`
(`incident.update`).

Each still increments `version` and writes timeline and audit entries. A
severity change that left no trace would defeat the point of having
`severity_source`.

### Required fields

| Command | Required |
|---|---|
| `ResolveIncident` | `resolution_note` when no automatic recovery preceded it |
| `CloseIncident` | `closure_reason`, one of `resolved`, `false_positive`, `duplicate`, `expected_traffic`, `no_action_required`, `other`; free text required when `other` |
| `ReopenIncident` | `reason` |
| `SuppressIncident` | `reason` and `expires_at` — an indefinite suppression is how a real attack gets missed, so the API requires an expiry |
| `ChangeSeverity` | `reason` when lowering |
| `AddNote` | `body`, `visibility` |

### Illegal transitions

Everything not listed is illegal and returns `409 Conflict` with a
machine-readable code, never a silent no-op. Specifically:

- Nothing transitions **out of** `Closed` except `ReopenIncident` and the
  automatic reopen. `Closed` is the only terminal state.
- No transition may skip `Resolved` on the way to `Closed`. Closing
  directly from `Investigating` would lose the distinction between "we
  fixed it" and "we gave up".
- `Suppressed` may not be resolved. Unsuppress first, so the timeline
  records that someone consciously took it out of suppression.

Following the pattern Phase 4 used for `DetectionState`, an illegal edge
should be **refused by a guard** rather than assigned, so that a future
edit introducing an unreasoned edge fails loudly instead of silently
entering a state whose timers mean nothing.

### Idempotency

Every operator transition is idempotent on `(idempotency_key, tenant)`.
Re-issuing `AcknowledgeIncident` for an already-acknowledged incident:

- **same** idempotency key, same body → returns the original result,
  `200`, no new timeline entry;
- **same** key, different body → `409`, per
  [ADR 0016](decisions/0016-incident-concurrency-and-idempotency.md);
- **no** key, already in the target state → `409` with
  `incident.state.unchanged`.

## Automatic recovery and closure

The timing knobs, all configurable, all evaluated against an injectable
clock so tests never sleep:

| Setting | Default | Meaning |
|---|---|---|
| `recovery_confirmation` | 5 min | `Recovering` must hold this long before `Resolved` |
| `reopen_window` | 15 min | Recurrence within this reopens rather than creating new |
| `auto_close_after` | 24 h | `Resolved` → `Closed`, if enabled |
| `auto_close_enabled` | `true` | Master switch |
| `auto_close_min_severity` | — | Severities at or above this require manual closure |
| `detector_silence_timeout` | 3 × detection window, min 5 min | Silence before `detector_silent` recovery |

**Defaults are recommendations, not decisions.** `reopen_window` is
**BQ-9**; whether `critical` requires manual closure is **BQ-8**.

The recommended shape, for the owner to accept or change:

1. A clear event moves the incident to `Recovering`, never straight to
   `Resolved` — an attack that pauses for thirty seconds has not ended.
2. `Recovering` held for `recovery_confirmation` moves to `Resolved`.
3. A new qualifying event during `Recovering` aborts recovery and returns
   the incident to its prior state, with a timeline entry.
4. `Resolved` incidents auto-close after `auto_close_after`, unless
   severity is at or above `auto_close_min_severity`.
5. Manual close is always available from `Resolved`.
6. Recurrence inside `reopen_window` reopens; outside it, a new incident
   is created that references its predecessor.

### The two silences

A detection that ends because traffic genuinely cleared arrives as an
`Ended` event with reason `ClearSustained`. A detection that ends because
the exporter stopped reporting arrives with reason `Stale` — or does not
arrive at all, if the collector died. These must not be conflated:

| Situation | Incident goes to | Reason recorded |
|---|---|---|
| `Ended` / `ClearSustained` | `Recovering` | `traffic_cleared` |
| `Ended` / `Stale` | `Recovering` | `detector_stale` |
| No events at all past the timeout | `Recovering` | `detector_silent` |
| `Ended` / `PolicyWithdrawn` | `Recovering` | `policy_withdrawn` |
| `Ended` / `ManualReset` | `Recovering` | `detector_reset` |

All five converge on `Recovering`, but the recorded reason differs, and
the API exposes it. An operator closing an incident deserves to know
whether the attack stopped or the telemetry did. `policy_withdrawn` in
particular means somebody edited the policy out from under a live
incident, which is worth seeing.

### After a detector restart

The detector loses runtime state and re-derives from traffic (ADR 0010).
An ongoing attack produces a fresh `Started` event with a new
`detection_id`, which correlates onto the **existing** incident because
the key is scope-based. No `Ended` arrives for the pre-restart detection,
so `detector_silent` is the safety net that prevents an incident being
orphaned open forever. Both paths are covered by the
[testing plan](incident-testing-plan.md).
