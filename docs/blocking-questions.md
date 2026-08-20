# Blocking Questions

Status: Phase 0 draft
Last updated: 2026-08-20

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

## Non-Blocking Items (explicitly not asked here)

Collector language split (Rust vs. Go per component), event-transport
choice (NATS vs. Redpanda vs. Kafka), Recharts vs. ECharts, and specific
dependency version pins are **not** blocking questions — they are ADR-level
engineering decisions to be resolved in the phase that needs them, per
master prompt §31 ("do not ask minor questions").
