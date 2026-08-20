# Acceptance Criteria

Status: Phase 0 draft
Last updated: 2026-08-20

## Phase 0 Acceptance Criteria (this phase)

Phase 0 is complete when:

- [x] `prompts/CLAUDE_MASTER_PROMPT.md` has been read in full.
- [x] All 16 documents listed in master prompt §31 exist under `docs/`.
- [x] No production code (no `crates/`, `apps/`, application source) has
      been written.
- [x] Clean-room boundary is documented and unambiguous.
- [x] Product naming/branding decision is documented and does not use any
      proprietary vendor's name, trademark, CLI, or terminology.
- [x] Functional and non-functional requirements are captured at a
      traceable level (FR-/NFR- IDs) for later phases to reference.
- [x] At least one dependency-license matrix entry raises verification
      status honestly (`REQUIRES VERIFICATION`), with no fabricated
      license claims.
- [x] Risk register and blocking questions exist and are non-empty where
      genuine risk/ambiguity exists.
- [x] A repository (`git init`) exists locally with `main` as the default
      branch, ready for a Phase 0 commit.
- [ ] Phase 0 has been reviewed by WeTechi Solutions before Phase 1 begins
      (external gate — cannot be self-certified by the agent).

## Forward-Looking Acceptance Criteria (per future phase, for planning only)

These are stated now so later phases can be scoped against them; they are
**not** being evaluated in Phase 0.

### Phase 1 — Repository foundation
- Monorepo skeleton matches master prompt §22 layout.
- CI validates formatting/linting on an empty or skeleton codebase.
- LICENSE decision is made and documented (not left as TBD).
- ADR template exists and is used for at least the collector-language and
  event-transport decisions once Phase 1 lands.

### Phase 2 — IPFIX collector MVP
- IPFIX parser passes fuzz testing with no crashes on a defined corpus.
- Template cache correctly survives exporter restart in an integration
  test.
- Prometheus metrics for parser failures/unknown templates/socket drops
  are observable end to end.
- Replay tool can drive the collector from a synthetic fixture.

### Phase 3 — Aggregation and classification
- Direction classification has a passing unit test for each of
  Incoming/Outgoing/Internal/Other, including IPv6 and prefix-overlap
  cases.
- Aggregation stays within a documented bounded-memory target under a
  high-cardinality synthetic load test.
- ClickHouse output schema is documented (not proprietary-derived) before
  merge.

### Phase 4 — Detection engine
- Threshold detection has passing hysteresis and cooldown tests.
- Dry-run and alert-only modes never trigger a mitigation action, verified
  by test.

### Phase 5 — Incident management
- Full incident state-machine transition table is tested, including
  invalid-transition rejection.
- Audit trail entries are created for every state transition.

### Phase 6 — Dashboards and notifications
- Grafana dashboard JSON validates in CI.
- Each notification channel has an integration test using a mock/sandbox
  endpoint, not a real external send in CI.

### Phase 7 — BGP mitigation lab
- All mitigation actions default to dry-run in a fresh install.
- Unauthorized-prefix test suite proves the controller refuses to announce
  outside the allowlist.
- Production BGP remains disabled by default, verified by a test asserting
  the default configuration value.

### Phase 8 — Multi-tenancy and RBAC
- Multi-tenant isolation tests prove tenant A cannot read tenant B's data
  via API, DB query, or export.
- RBAC tests cover every role listed in FR-12.2 for at least one
  allow/deny case each.

### Phase 9 — Production hardening
- Load/soak test results are real, executed, and reported (not
  estimated).
- Backup/restore is demonstrated round-trip, not just documented.
- Security review findings are triaged with no open Critical/High items
  at release.

### Phase 10 — v1.0.0 release
- Release includes changelog, release notes, upgrade guide, rollback
  guide, checksums, SBOM, signed artifacts, migration notes, known issues,
  and a real test summary.
- Production checklist and security checklist are both signed off.

## General Rule

No phase may claim a criterion is met without the actual command/test
execution and result backing it, per master prompt §30 rules 9–13.
