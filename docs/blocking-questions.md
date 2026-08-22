# Blocking Questions

Status: Phase 5 planning
Last updated: 2026-08-22

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

**Blocks** Milestone 5A, since it determines the primary key type.

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

**Blocks** Milestone 5A, since it determines the state set.

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

**Blocks** Milestone 5B entirely. Without it there is no persistence and
no API, leaving only the in-memory domain of 5A.

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

## BQ-9: What should the default reopen window be?

Recurrence inside the window reopens the existing incident; outside it,
a new incident is created referencing its predecessor. The proposed
default is **15 minutes**.

Too short and one attack becomes a stream of separate incidents. Too long
and genuinely distinct attacks are merged into one, hiding the second.
The right value depends on observed attack patterns on this network,
which is operational knowledge rather than an engineering judgement.

**Does not block** implementation.

## Non-Blocking Items (explicitly not asked here)

Collector language split (Rust vs. Go per component), event-transport
choice (NATS vs. Redpanda vs. Kafka), Recharts vs. ECharts, and specific
dependency version pins are **not** blocking questions — they are ADR-level
engineering decisions to be resolved in the phase that needs them, per
master prompt §31 ("do not ask minor questions").
