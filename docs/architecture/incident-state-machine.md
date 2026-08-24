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
dropped. **BQ-6 was resolved on 2026-08-22**: they are absent, not
present-but-unreachable, because an enum a client can read still
advertises a capability that does not exist. FR-5.1 itself still needs
amending to match — **FU-27**.

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

**Seven states.** Two things that look like states are deliberately not
states, decided 2026-08-22 and recorded in
[ADR 0014](decisions/0014-incident-state-machine.md).

**`Reopened` is a transition, not a state.** A state should describe a
durable condition, and "was reopened" is an event in the past. An
incident that has been reopened is `Open` — the fact that it was reopened
lives in `reopened_at`, `reopen_count`, and a timeline entry. Modelling
it as a state would mean answering "what does a reopened incident that is
now being investigated look like?" with either a second state field or a
lie.

**`Suppressed` is an attribute, not a state.** This was originally
modelled as a state and the change fixes a real defect rather than
tidying the diagram. Suppression is orthogonal to lifecycle position: it
describes whether an incident *alerts*, not where the human response has
got to. As a state it collided with the lifecycle — `UnsuppressIncident`
had to send the incident somewhere, and it sent it to `Open`, **silently
destroying the progress of an incident that had been `Investigating`**.
That is precisely the bug `Recovering` avoids by storing
`state_before_recovering`, and needing a second restoration field was the
signal that the modelling was wrong. As an attribute, the lifecycle state
is simply never touched, so there is nothing to restore.

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
Resolved ────────────► Closed         on: auto-close delay — NOT for critical
Resolved ────────────► Open           on: qualifying event within reopen window
Closed ──────────────► Open           on: qualifying event within reopen window
```

**`Resolved → Closed` never fires automatically for a `critical`
incident** under the default configuration. See "Critical incidents close
by hand" below.

`Recovering → (prior state)` restores the state the incident held before
recovery began, which is why `state_before_recovering` is stored. An
incident that was `Investigating` when traffic briefly dipped returns to
`Investigating`, not to `Open` — losing the operator's progress because
traffic flickered would be a bug.

Suppression is deliberately absent from every automatic transition. A
suppressed incident absorbs events silently: they are linked and counted,
metrics update, and its lifecycle state advances normally underneath.
What suppression withholds is *attention* — it is the flag a future
notification phase must consult — and an automatic change to it would
defeat the point.

### `Resolved` and `Closed` mean different things

This distinction carries the whole of the BQ-8 decision, so it is worth
stating before the table rather than after it.

| State | Means |
|---|---|
| `Resolved` | **The traffic condition recovered.** A statement about the network |
| `Closed` | **NOC review and operational handling are complete.** A statement about the humans |

They are not two words for finished. An incident can be `Resolved`
minutes after an attack stops and stay open for review for hours, and
that gap is exactly where the post-incident work happens. Collapsing them
would mean a system that cannot tell "the flood stopped" from "somebody
looked at it".

### Critical incidents close by hand

**Decided 2026-08-22 (BQ-8): a `critical` incident does not auto-close
under the default configuration.** It still moves automatically to
`Recovering` when detection clears, and automatically to `Resolved` after
the recovery confirmation period — automation handles the *network*
claim. It stops there. The *human* claim requires a human.

The reasoning is that auto-close buys convenience, and at `critical` —
the severity that implies customer impact — the convenience is worth very
little against the risk of an incident nobody ever saw. Below `critical`
the trade runs the other way: nobody would have reviewed those anyway,
and a queue full of resolved-but-unclosed noise is its own failure.

| Severity | `Recovering` → `Resolved` | `Resolved` → `Closed` |
|---|---|---|
| `critical` | Automatic | **Manual only, by default** |
| `major`, `minor`, `info` | Automatic | Automatic after the closure delay, if enabled |

This is **Community** behaviour. A correctness and safety default is not
an Enterprise feature, and
[ADR 0017](decisions/0017-incident-community-enterprise-boundary.md)
already forbids reserving one.

#### Overriding it

`criticalManualClosureRequired` is configurable, and its secure default
is `true`. Turning it off is a deliberate act with five requirements,
none of them optional:

1. **Explicit.** No implicit inheritance and no "unset means false".
2. **Tenant-aware.** An override applies to a named tenant, never
   globally by accident.
3. **Policy-aware where supported.** A per-policy override is permitted
   where the deployment models policies; absent that, tenant scope is the
   finest granularity.
4. **Permissioned.** Requires `incident.closure_policy.override`, which
   is deliberately *not* in any default operator bundle.
5. **Audited immutably, and visible.** Every override writes an audit
   record, and the effective value appears in
   [effective-configuration diagnostics](../configuration/incident-management-plan.md)
   so an operator can answer "will this critical auto-close?" without
   reading source or guessing at precedence.

**No notification and no mitigation is implied by manual closure.**
Closing an incident sends nothing and does nothing to traffic; it records
that a human finished with it.

### Operator transitions

Every row writes a timeline entry **and** an audit record inside the same
transaction as the state change, and increments `version`. There is no
transition that mutates state without both.

| Command | From | To | Permission | Audit |
|---|---|---|---|---|
| `AcknowledgeIncident` | `Open` | `Acknowledged` | `incident.acknowledge` | `allowed`/`denied`, before+after state |
| `BeginInvestigation` | `Open`, `Acknowledged`, `Monitoring` | `Investigating` | `incident.investigate` | as above |
| `MarkMonitoring` | `Acknowledged`, `Investigating` | `Monitoring` | `incident.investigate` | as above |
| `ResolveIncident` | `Open`, `Acknowledged`, `Investigating`, `Monitoring`, `Recovering` | `Resolved` | `incident.resolve` | as above |
| `CloseIncident` | `Resolved` | `Closed` | `incident.close` | as above, plus `closure_reason` |
| `ReopenIncident` | `Closed`, `Resolved` | `Open` | `incident.reopen` | as above, plus `reason` and new `reopen_count` |

Automatic transitions are audited too, with `actor_type = system` and
`actor_id = system:correlator`. An automatic closure that left no audit
record would be indistinguishable from a manual one afterwards.

Non-transitioning commands, which mutate the incident without changing
state: `AssignIncident`, `UnassignIncident`, `ClaimIncident`,
`ReleaseIncident` (`incident.assign`); `AddNote`
(`incident.note.create`); `ChangeSeverity`
(`incident.severity.change`); `ChangePriority`
(`incident.priority.change`); `AddTag` / `RemoveTag`
(`incident.update`); and `SuppressIncident` / `UnsuppressIncident`
(`incident.suppress`), which now set and clear the suppression attribute
without moving the incident through the lifecycle.

### Suppression as an attribute

Three fields on the incident, none of them the lifecycle state:

| Field | Notes |
|---|---|
| `suppressed_until` | `TIMESTAMPTZ`, **mandatory** when suppressing |
| `suppression_reason` | Mandatory free text |
| `suppressed_by` | Actor reference |

`suppressed` is derived: `suppressed_until IS NOT NULL AND
suppressed_until > now()`. Deriving rather than storing a boolean means
suppression cannot outlive its own expiry through a missed sweep — an
expired suppression stops applying whether or not anything ran.

An indefinite suppression is how a real attack gets missed, so the expiry
stays mandatory. Suppression is independently queryable from state:
`GET /incidents?state=investigating&suppressed=true` is a meaningful and
answerable question, which it was not while the two shared one field.

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

Everything not listed as legal is illegal and returns `409 Conflict` with
a machine-readable code, never a silent no-op. The complete invalid set,
stated rather than left as "whatever is missing above":

| Attempted | Why refused | Code |
|---|---|---|
| `Closed` → anything except `Open` | `Closed` is the only terminal state; the sole exit is a reopen | `incident.illegal_transition` |
| Any state → `Closed` other than from `Resolved` | Skipping `Resolved` erases the difference between "the traffic recovered" and "we gave up" | `incident.illegal_transition` |
| `Resolved` → `Closed` automatically, severity `critical`, default config | BQ-8 | `incident.manual_closure_required` |
| `Open` → `Monitoring` | Monitoring means understood-and-watched; nothing has been understood yet | `incident.illegal_transition` |
| `Recovering` → `Acknowledged`/`Investigating`/`Monitoring` directly | Recovery either completes to `Resolved` or aborts to its **prior** state; an operator cannot hand-steer it sideways | `incident.illegal_transition` |
| `Open` → `Open`, and every self-edge | A state does not transition to itself; a repeated command is an idempotency question, not a transition | `incident.state_unchanged` |
| Any transition on an incident of another tenant | Tenant isolation | `incident.not_found` (404, never 403) |
| Any transition without `expected_version` | Optimistic concurrency is mandatory for state changes | `incident.validation_failed` |
| Any transition whose permission the caller lacks | Authorization | `incident.forbidden` |

Two cases that look illegal and are **not**:

- **A suppressed incident may be resolved and closed.** Suppression
  withholds attention; it does not freeze the lifecycle, and forcing an
  unsuppress before closing known noise is friction with no audit value.
  Both events land on the timeline.
- **A `Resolved` incident may be reopened**, not only a `Closed` one. A
  recurrence during the review window is the common case, and it should
  land on the incident somebody is already reviewing.

Following the pattern Phase 4 used for `DetectionState`, an illegal edge
is **refused by a guard** rather than assigned, so a future edit that
introduces an unreasoned edge fails loudly instead of silently entering a
state whose timers mean nothing.

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
| `reopen_window` | **15 min** | Recurrence within this reopens rather than creating new (BQ-9) |
| `automatic_closure_enabled` | `true` | Master switch for non-critical auto-close |
| `automatic_closure_delay` | **30 min** | `Resolved` → `Closed` for non-critical incidents |
| `critical_manual_closure_required` | **`true`** | Critical incidents never auto-close (BQ-8) |
| `detector_silence_timeout` | 3 × detection window, min 5 min | Silence before `detector_silent` recovery |

**Two defaults changed on 2026-08-22.** `auto_close_min_severity` — a
severity threshold — is replaced by the explicit boolean
`critical_manual_closure_required`, which states the rule instead of
encoding it in a comparison. And the closure delay moved from 24 hours to
**30 minutes**: with critical incidents now excluded from auto-close
entirely, the delay only governs incidents nobody was going to review, and
a day of those sitting in the queue serves nobody.

**These defaults are now decided, not recommended.** BQ-8 and BQ-9 were
resolved on 2026-08-22; see
[blocking questions](../blocking-questions.md) and
[ADR 0014](decisions/0014-incident-state-machine.md).

The recommended shape, for the owner to accept or change:

1. A clear event moves the incident to `Recovering`, never straight to
   `Resolved` — an attack that pauses for thirty seconds has not ended.
2. `Recovering` held for `recovery_confirmation` moves to `Resolved`.
3. A new qualifying event during `Recovering` aborts recovery and returns
   the incident to its prior state, with a timeline entry.
4. Non-critical `Resolved` incidents auto-close after
   `automatic_closure_delay`, if `automatic_closure_enabled`.
5. **Critical incidents never auto-close by default** and wait for an
   operator holding `incident.close`.
6. Manual close is always available from `Resolved`, at every severity.
7. Recurrence inside `reopen_window` reopens; outside it, a new incident
   is created that references its predecessor.

### Reopening, precisely

**Decided 2026-08-22 (BQ-9): the default reopen window is 15 minutes**, a
technical starting value and **not** a legal, regulatory, contractual, or
SLA requirement. It is configurable.

**The boundary is inclusive.** Elapsed time **≤** `reopen_window`
reopens; **>** `reopen_window` creates a new incident. Stating which side
the boundary falls on matters because "15 minutes" otherwise has two
defensible readings, and a test written against one and code against the
other passes review and fails in production.

Elapsed time is measured from `resolved_at` while the incident is
`Resolved`, and from `closed_at` once it is `Closed` — the anchor moves
with the state, not with how the incident arrived there. (`Closed` is
reachable only from `Resolved` under either guard, so "closed without
passing through resolution" cannot occur; anchoring on `resolved_at` even
after closure would put an incident closed after its automatic closure
delay outside the reopen window before an operator could ever see it —
the same unreachability the state-anchored rule exists to avoid.)

| Configuration | Meaning |
|---|---|
| Minimum `0` | Recurrence **always** creates a new incident; reopening is off |
| Default `15m` | The approved starting value |
| Maximum accepted `24h` | A **validation** bound, not an operational recommendation |

The maximum exists to stop a typo turning into a month-long window, not
because 24 hours is sensible. A window that long absorbs genuinely
distinct attacks into one incident, which is the more dangerous direction
of error — a merged incident hides the second attack, while a split one
merely annoys.

A reopen does **all** of the following, in the one authoritative
transaction, or none of it:

- transitions the incident back to `Open`
- increments `reopen_count`
- sets `reopened_at`
- appends an immutable `reopened` timeline entry
- appends a mandatory audit record
- links the new detection evidence
- **preserves all previous timeline entries and evidence** — a reopen
  adds history, it never rewrites it
- **preserves the original incident identity** — same `incident_id`, same
  `incident_number`
- increments `version`
- writes the outbox event

**A reopen must never produce two simultaneously active incidents for one
correlation key.** The partial unique index in
[persistence](incident-persistence.md) makes that impossible at the
database rather than merely unlikely in the correlator, so two concurrent
reopen attempts resolve to one winner and one retry.

Correlation itself is unchanged by this decision and is restated here
because a reopen is where people expect it to bend:
`tenant | target_type | target_id | direction | family`. Policy id stays
out. Category stays out and remains a mutable derived summary. Host and
parent prefix stay separate — different target type *and* identity.
Incoming and outgoing stay separate. IPv4 and IPv6 stay separate. And a
detector restart, which mints a fresh `detection_id`, must not prevent
recurrence correlating: the key is semantic, never detection-derived.

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
