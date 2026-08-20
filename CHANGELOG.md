# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once versioned releases begin (see [ROADMAP.md](ROADMAP.md)).

## [Unreleased]

### Added — Phase 1: GitHub repository and documentation foundation

- Full monorepo directory skeleton per `prompts/CLAUDE_MASTER_PROMPT.md`
  §22 (apps/, crates/, deployments/, docs/, grafana/, database/, tests/,
  tools/, scripts/, examples/, branding/), with reserved-status README
  placeholders and no production code.
- Professional root `README.md`.
- `LICENSE` (Apache License 2.0, recommendation pending confirmation —
  see `docs/license-recommendation.md`) and `NOTICE`.
- `SECURITY.md`, `SUPPORT.md`, `CONTRIBUTING.md`, `GOVERNANCE.md`,
  `CODE_OF_CONDUCT.md` (Contributor Covenant v2.1).
- Root `ROADMAP.md` (public summary of `docs/roadmap.md`).
- MkDocs Material skeleton (`mkdocs.yml` + `docs/*/index.md` section
  landing pages).
- Architecture Decision Record process and template
  (`docs/architecture/decisions/`).
- GitHub issue templates (bug report, feature request, documentation) and
  issue-template config linking to `SECURITY.md`.
- Pull-request template with a clean-room self-certification checklist.
- `.github/CODEOWNERS`.
- `.github/dependabot.yml` (github-actions ecosystem).
- Validation-only GitHub Actions workflow (Markdown lint, YAML lint,
  Markdown link check) — no build, deploy, or release automation yet.
- `Makefile` and `Taskfile.yml` with `lint-markdown`, `lint-yaml`,
  `docs-serve`, `docs-build`, and `validate` targets.
- `docs/development/local-setup.md` documenting how to work with the
  repository at this phase.

### Added — Phase 0: Product foundation and clean-room boundary

- Product charter, clean-room boundary, functional and non-functional
  requirements, architecture and technology options, dependency license
  matrix, commercial boundaries, security principles, MVP scope,
  out-of-scope list, risk register, roadmap, acceptance criteria, blocking
  questions, and naming/branding decisions under `docs/`.

[Unreleased]: https://github.com/badshashorif/wetechi-netmon/compare/main...HEAD
