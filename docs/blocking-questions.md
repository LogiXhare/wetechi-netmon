# Blocking Questions

Status: Phase 5 planning — BQ-5, BQ-6, and BQ-7 **resolved** 2026-08-22.
BQ-8 and BQ-9 remain **OPEN**.
Last updated: 2026-08-22

## Decision summary

| # | Question | Status | Blocks |
|---|---|---|---|
| BQ-1 | Project licence | Resolved — Apache-2.0 | — |
| BQ-2 | Grafana bundling | Open | Phase 6 |
| BQ-3 | Reference hardware and traffic rates | Open | Benchmarking |
| BQ-4 | Phase 8 before or after v1.0.0 | Open | Phase 7 scheduling |
| BQ-5 | Incident identity (UUID vs ADR 0009) | **Resolved 2026-08-22** — UUIDv7, conditional | Was 5A |
| BQ-6 | Mitigation states in the incident machine | **Resolved 2026-08-22** — excluded | Was 5A |
| BQ-7 | PostgreSQL and HTTP dependencies | **Resolved 2026-08-22** — approved architecturally | Was 5B |
| BQ-8 | Manual closure for critical incidents | **OPEN** | Nothing — configuration default |
| BQ-9 | Default reopen window | **OPEN** | Nothing — configuration default |

These are the only questions treated as genuinely blocking for Phase 1+
architecture and legal posture. Per master prompt §31, minor questions are
intentionally excluded — engineering-level choices proceed via ADRs in the
relevant phase instead of stalling here.

## BQ-1: What license should WetechiNetMon itself use?

This determines the Phase 1 `LICENSE` file, the `NOTICE` file, and the
CONTRIBUTING model, and it interacts directly with the Enterprise/Managed
edition strategy in [commercial-boundaries.md](commercial-boundaries.md).

Options to choose between:

- Permissive (Apache-2.0 or MIT) — maximizes community adoption, but a
  competitor could fork and offer a competing managed service on the exact
  same code.
- Source-available / dual-license (e.g., a permissive core with certain
  modules under a more restrictive license, or a BSL-style delayed-open
  model) — protects WeTechi Solutions' commercial tiers, at the cost of not
  being a "pure" open-source project for community-adoption purposes.
- Copyleft (AGPLv3) — strong protection against SaaS competitors reusing
  the code without contributing back, but may deter enterprise adoption and
  needs care given Grafana's own AGPLv3 exposure (see
  [dependency-license-matrix.md](dependency-license-matrix.md)).

**This cannot be defaulted by the agent** — it is a business decision for
WeTechi Solutions.

**Resolved 2026-08-21.** Apache-2.0, with incoming contributions accepted
under the same license via DCO sign-off and no CLA — see
[ADR 0006](architecture/decisions/0006-contribution-licensing-dco-not-cla.md).
The permissive-license fork risk described above was accepted knowingly.

## BQ-2: Is Grafana bundled/modified, or treated strictly as an external, operator-supplied service?

Directly affects whether Grafana's AGPLv3 terms create obligations for
WetechiNetMon's own source availability. Phase 0's working assumption
(stated in the dependency matrix) is "external service only, ship
dashboard JSON/provisioning, never a modified Grafana binary." This needs
explicit confirmation before Phase 6 packaging work begins, since it
constrains how the web app and Grafana integration are packaged together.

## BQ-3: What are the actual reference/target hardware and traffic-rate expectations?

Non-functional performance targets (NFR-1) currently have no numeric
target because none were supplied. Before Phase 9 (production hardening)
load/soak testing can produce meaningful pass/fail criteria, WeTechi
Solutions needs to supply (or approve a proposed) target flow-record rate,
exporter count, and hardware profile — otherwise "production-ready" at
v1.0.0 has no objective bar.

## BQ-4: Does Phase 8 (multi-tenancy/RBAC) land before or after the v1.0.0 release cut?

Flagged in [roadmap.md](roadmap.md) — the master prompt's phase list
(0–10) places multi-tenancy at Phase 8 of 10, but the version list defines
v1.0.0 as the "production-ready single-tenant release" and v1.1.0/v1.2.0 as
multi-tenancy/enterprise-auth, implying tenancy is post-1.0. Phase 1
sequencing/roadmap planning benefits from an explicit confirmation of the
proposed resolution (tenancy ships after v1.0.0) before Phase 7/8 work is
scheduled. This is lower urgency than BQ-1–BQ-3 since it doesn't block
Phase 0/1 work, but should be confirmed before Phase 7 starts.

## BQ-5: Does FR-5.2's "UUID" requirement override the ADR 0009 no-`uuid` precedent for incidents?

[FR-5.2](functional-requirements.md) requires the incident record to
persist a **UUID**.
[ADR 0009](architecture/decisions/0009-detection-event-identity.md)
deliberately declined the `uuid` crate for *detection events*, because it
would have added a random-number dependency to a hot-path crate that did
not otherwise need one, and because those identifiers are correlation
keys rather than secrets.

That reasoning does not transfer unchanged. The incident manager will
already depend on a database driver and an HTTP stack, so `uuid` costs
proportionally nothing there.
[ADR 0013](architecture/decisions/0013-incident-identity.md) recommends
**UUIDv7** — satisfying FR-5.2 while avoiding UUIDv4's index
fragmentation and a serial key's enumerability. The fallback, if the
dependency is refused, is `BIGSERIAL` plus a random public token, which
is more moving parts and is not recommended.

Also to confirm: should the human-readable incident number
(`WNM-2026-000123`) reset its sequence annually, or run continuously per
tenant?

~~**Blocks** Milestone 5A, since it determines the primary key type.~~

### BQ-5 decision — 2026-08-22

**Approved conditionally: UUIDv7** for the internal `incident_id`,
subject to a focused dependency and licence review during implementation.

- Phase 4 detection-event identifiers are **unchanged**; ADR 0009 stands.
- The human-readable number is a separate database-backed display value.
- Identifiers are **not** authentication secrets, and authorization must
  never depend on their unpredictability.
- The `uuid` crate is **not** added by this decision — it goes through
  [ADR 0018](architecture/decisions/0018-phase5-dependency-selection.md).

**Not decided:** whether the incident number resets annually or runs
continuously per tenant. Affects a display value, not the primary key, so
it does not block 5A. Tracked as **FU-24**.

Recorded in
[ADR 0013](architecture/decisions/0013-incident-identity.md).

## BQ-6: Should the deferred mitigation states be absent from the incident state machine, or present but unreachable?

[FR-5.1](functional-requirements.md) specifies one state machine
including `AwaitingApproval`, `MitigationPending`, `Mitigating`, and
`HoldDown` — all mitigation concepts that Phase 5 is forbidden from
implementing and that Phase 7 will own. FR-5.1 also opens with
`Suspected` and `Confirmed`, which already exist in Phase 4 as
`PendingTrigger` and `Active`.

[ADR 0014](architecture/decisions/0014-incident-state-machine.md)
recommends implementing only the operator-facing states and deferring the
mitigation ones, on the grounds that an API advertising `Mitigating` when
nothing can mitigate is the same kind of capability lie that Phase 4's
`executed` field was added to prevent. The alternative is to define them
now as unreachable, which avoids a Phase 7 migration.

Either way FR-5.1 and the implementation will disagree until FR-5.1 is
updated to reference the ADR.

~~**Blocks** Milestone 5A, since it determines the state set.~~

### BQ-6 decision — 2026-08-22

**Approved: mitigation lifecycle stays outside the core incident state
machine.** Option C (present but unreachable) was considered and
rejected — an enum a client can read still advertises a capability that
does not exist.

- No `AwaitingApproval`, `MitigationPending`, `Mitigating`,
  `MitigationFailed`, `WithdrawalPending`, `Withdrawing`, or `HoldDown`.
- A **read-only, non-authoritative** mitigation reference seam remains:
  a summary field always `none` in Phase 5, plus reserved outbox event
  types nothing consumes.
- Mitigation status must never control the incident lifecycle; the two
  are independently queryable, and one incident may eventually have
  several mitigation operations.

**Two further state-machine decisions taken at the same review:**

- **`Reopened` is a transition, not a state** — already the design, now
  recorded as deliberate.
- **`Suppressed` becomes an attribute, not a state.** This is a change,
  and it fixes a defect: as a state, `UnsuppressIncident` had to send the
  incident somewhere and sent it to `Open`, silently discarding the
  progress of an incident that had been `Investigating`. The core
  lifecycle is now **seven** states.

FR-5.1 still needs updating to reference
[ADR 0014](architecture/decisions/0014-incident-state-machine.md) —
**FU-27**.

## BQ-7: May Phase 5 add PostgreSQL, a database driver, and an HTTP framework as dependencies?

Phase 4 added **zero** third-party crates. Phase 5 as designed cannot:
[ADR 0015](architecture/decisions/0015-incident-operational-storage.md)
selects PostgreSQL for operational state, which needs a driver
(`sqlx` or `tokio-postgres`, both MIT/Apache-2.0), and the REST API needs
an HTTP framework.

This is a genuine change in the project's dependency posture and is not a
decision to make silently inside an implementation commit. Each new crate
needs a
[dependency-license-matrix](dependency-license-matrix.md) row before use.

~~**Blocks** Milestone 5B entirely.~~ Without it there is no persistence
and no API, leaving only the in-memory domain of 5A.

### BQ-7 decision — 2026-08-22

**Approved architecturally.** Phase 5 may introduce PostgreSQL and HTTP
dependencies. This is approval of **capability, not of any crate**.

Before any crate is added, implementation must record a
dependency-selection ADR, **verify actual published package metadata**
rather than the values assumed in these planning documents, update the
licence matrix and `NOTICE`, and validate on both Windows and Linux.
Criteria and shortlist are in
[ADR 0018](architecture/decisions/0018-phase5-dependency-selection.md);
selection itself is **FU-25** and **FU-26**.

No dependency was added during planning or during this review.

## BQ-8: Should `critical` incidents require manual closure?

Auto-close is convenient and prevents a queue filling with resolved
incidents nobody closes. It also means a critical incident can close with
no human ever having looked at it, which for a severity that implies
customer impact may be unacceptable.

[ADR 0014](architecture/decisions/0014-incident-state-machine.md)
proposes `INCIDENT_AUTO_CLOSE_MIN_SEVERITY = critical`, meaning critical
incidents require manual closure and everything below auto-closes after
24 hours. This is a policy question about how the NOC works, not an
engineering one.

**Does not block** implementation — it is a configuration default — but
resolving it alongside BQ-5 to BQ-7 is cheaper than revisiting.

### BQ-8 status — OPEN, owner decision required

Reviewed 2026-08-22 and **deliberately not decided**, because the
trade-off is about how the NOC works rather than about engineering, and
inferring it would conceal a real choice behind a default.

**Blocks:** nothing. Not Milestone 5A. It is a configuration value
(`INCIDENT_AUTO_CLOSE_MIN_SEVERITY`) read at runtime, so implementation
can proceed and the value can be set before first production use. The
only cost of deferring is that the shipped default may need changing.

| Option | Consequence |
|---|---|
| **A. `critical` requires manual closure** (proposed default) | No critical incident closes without a human looking at it. Cost: the queue accumulates resolved-but-unclosed criticals if the team does not triage, and someone must notice |
| **B. Everything auto-closes after the delay** | The queue stays clean with no effort. Cost: a critical incident — a severity that implies customer impact — can open, resolve, and close with **no human ever having seen it**, and the first anyone hears of it is a customer call |
| **C. Manual closure at `major` and above** | Stricter than A; more manual work; more coverage |
| **D. Configurable per tenant** | Most flexible. Cost: more configuration surface, and a per-tenant default is one more thing to get wrong |

**Recommendation: Option A.** Auto-close is a convenience, and it is
reasonable for the severities where nobody would have acted anyway. At
`critical` the convenience is buying very little and risking the one
outcome that damages trust in the system — an incident nobody saw. Option
D is attractive later but adds configuration surface before there is
evidence anyone needs it.

**Security implication:** auto-closing criticals shortens the window in
which an operator would notice a genuine attack, and combines badly with
suppression. **Operational implication:** Option A needs a
"resolved-but-unclosed critical" view so the backlog is visible; that
view is already implied by the existing list filters. **Implementation
cost:** none beyond a default value — the mechanism exists either way.

## BQ-9: What should the default reopen window be?

Recurrence inside the window reopens the existing incident; outside it,
a new incident is created referencing its predecessor. The proposed
default is **15 minutes**.

Too short and one attack becomes a stream of separate incidents. Too long
and genuinely distinct attacks are merged into one, hiding the second.
The right value depends on observed attack patterns on this network,
which is operational knowledge rather than an engineering judgement.

**Does not block** implementation.

### BQ-9 status — OPEN, owner decision required

Reviewed 2026-08-22 and **deliberately not decided**. As the question
itself says, the right value depends on observed attack patterns on this
network, which is operational knowledge this review does not have.

**Blocks:** nothing. Not Milestone 5A. `INCIDENT_REOPEN_WINDOW_SECS` is a
runtime configuration value. The correlation *mechanism* — reopen inside
the window, new incident outside it, referencing its predecessor — is
already decided and is unaffected by the number.

| Option | Consequence |
|---|---|
| **A. 15 minutes** (proposed default) | Balances the two failure modes. A pause-and-resume attack stays one incident; a genuinely new attack an hour later is separate |
| **B. 5 minutes** | Fewer wrongly-merged incidents. Cost: a flapping attack produces a stream of separate incidents, each needing acknowledgement — the exact alert fatigue hysteresis exists to prevent |
| **C. 60 minutes** | Very few duplicates. Cost: a genuinely distinct second attack within the hour is absorbed into the first and **hidden**, which is the more dangerous direction of error |
| **D. Scale with severity** | Longer window for critical. Cost: reopen behaviour becomes severity-dependent and harder to reason about during an incident |

**Recommendation: Option A, 15 minutes**, explicitly as a starting value
to be revisited once real recurrence data exists — not as a considered
answer to a question that needs data.

**Security implication:** too long is the dangerous direction. A second
attack merged into a resolved incident may be missed entirely, and the
threat model's late-event rule (T-18) already prevents a stale event from
reopening, so a long window widens the window for *genuine* recurrences
to be absorbed. **Operational implication:** the value directly drives
incident counts, so changing it later makes historical trend comparisons
inconsistent — worth setting deliberately before first production use.
**Implementation cost:** none beyond a default value.

**Suggested follow-up regardless of the value chosen:** record the
distribution of gaps between a close and the next qualifying event on the
same correlation key, so the value can eventually be chosen from evidence
rather than judgement. Tracked as **FU-28**.

## Non-Blocking Items (explicitly not asked here)

Collector language split (Rust vs. Go per component), event-transport
choice (NATS vs. Redpanda vs. Kafka), Recharts vs. ECharts, and specific
dependency version pins are **not** blocking questions — they are ADR-level
engineering decisions to be resolved in the phase that needs them, per
master prompt §31 ("do not ask minor questions").
