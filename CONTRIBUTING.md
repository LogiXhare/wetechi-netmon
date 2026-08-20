# Contributing to WetechiNetMon

Thank you for considering a contribution. This document covers process;
for what the project is and why, start with
[docs/product-charter.md](docs/product-charter.md).

## Before You Start

1. Read [docs/clean-room-boundary.md](docs/clean-room-boundary.md) in
   full. This is not optional. WetechiNetMon must remain an independently
   engineered, clean-room implementation — contributions that copy,
   translate, or closely imitate any proprietary product's code, schemas,
   configuration syntax, CLI syntax, dashboards, or documentation will be
   rejected regardless of how useful they are.
2. Read [docs/security-principles.md](docs/security-principles.md),
   especially the rules around BGP mitigation safety (never enable or test
   mitigation against unauthorized networks; never generate real attack
   traffic).
3. Check [docs/roadmap.md](docs/roadmap.md) and [ROADMAP.md](ROADMAP.md)
   for the current phase. Contributions ahead of the current phase's scope
   (see [docs/out-of-scope.md](docs/out-of-scope.md)) are likely to be
   deferred, not rejected outright — open an issue to discuss timing
   first.

## Development Process

This project is built in explicit, sequential phases (see
[docs/roadmap.md](docs/roadmap.md)). Each phase has documented
deliverables and acceptance criteria before the next phase begins. Pull
requests should target the current phase's scope.

### Local Setup

See [docs/development/local-setup.md](docs/development/local-setup.md).

### Branching

- Branch from `main`.
- Use a descriptive branch name, e.g. `phase2/ipfix-template-cache` or
  `docs/fix-roadmap-typo`.

### Commit Messages — Conventional Commits

All commits must follow [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<optional scope>): <short summary>

<optional body>
```

Common types: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`, `ci`,
`build`, `perf`, `security`. Example:

```text
docs(phase1): add MkDocs skeleton and ADR template
```

### Pull Requests

Every pull request must:

- Update documentation in the same PR as the corresponding feature —
  documentation is not a follow-up task (see
  [docs/functional-requirements.md](docs/functional-requirements.md)
  traceability note).
- Include tests before a feature is marked complete, once the project has
  runnable code (Phase 2 onward). Untested code is not "done."
- Self-certify against [docs/clean-room-boundary.md](docs/clean-room-boundary.md)
  using the pull-request template checklist.
- Pass all validation CI checks (see `.github/workflows/`).
- Be small and focused. Prefer several small PRs over one large one.

### Dependency Additions

Before adding any new third-party dependency (Rust crate, npm package, or
otherwise), add a row to
[docs/dependency-license-matrix.md](docs/dependency-license-matrix.md)
with a completed license record. Never fabricate license information —
mark it `REQUIRES VERIFICATION` if you are not certain, and do not add the
dependency to a build manifest until that is resolved.

### Architecture Decisions

Significant, hard-to-reverse technical decisions (new protocol
implementation approach, storage schema, event transport, a service's
implementation language, etc.) require an Architecture Decision Record —
see [docs/architecture/decisions/index.md](docs/architecture/decisions/index.md).
Code that depends on an undecided architecture question should not be
merged; open the ADR first.

## Reporting Rules for Any Contributor (Human or AI Agent)

These rules apply to anyone contributing to this repository, including AI
coding agents operating under `prompts/CLAUDE_MASTER_PROMPT.md`:

- Do not claim tests passed unless they were actually executed.
- Do not generate fake benchmark results.
- Do not generate fake security claims.
- Do not enable production BGP, ever, by default.
- Do not commit secrets.
- Do not include real customer data in the repository, examples, or
  fixtures.
- Never generate real DDoS traffic, even for testing — use synthetic or
  sanitized flow fixtures only.
- Mark unverified assumptions clearly rather than presenting them as fact.

## Code of Conduct

Participation in this project is governed by
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Governance

Decision-making authority and escalation paths are described in
[GOVERNANCE.md](GOVERNANCE.md).
