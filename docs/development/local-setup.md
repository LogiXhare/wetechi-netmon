# Local Development Setup

Status: Phase 1 — documentation/validation tooling only. No product
services are runnable yet (first runnable component ships in Phase 2).

## What You Can Do Today

At Phase 1, the repository contains documentation, repository scaffolding,
and validation tooling only. There is no application code to build or run.
What you *can* do locally:

1. Validate Markdown formatting
2. Validate YAML syntax (workflows, Dependabot, issue templates)
3. Preview the documentation site (requires Python + MkDocs Material)

## Prerequisites

| Tool | Used for | Required now? |
|---|---|---|
| Git | Version control | Yes |
| Node.js + npm (npx) | Markdown/YAML linting via `npx` | Yes |
| Python 3.10+ and `pip` | MkDocs Material site build/preview | Only if previewing docs site |
| GitHub CLI (`gh`) | Optional, for PRs/issues from the terminal | No |

Rust (`cargo`/`rustc`), Node build tooling for the web app, and container
tooling are **not** required yet — they become relevant starting Phase 2
(collector) and Phase 6 (web app) respectively, and this document will be
updated when that happens.

## Clone

```bash
git clone https://github.com/badshashorif/wetechi-netmon.git
cd wetechi-netmon
```

## Validate Markdown

```bash
npx -y markdownlint-cli2 "**/*.md" "#node_modules"
```

Or via the Makefile/Taskfile wrapper:

```bash
make lint-markdown
# or
task lint:markdown
```

## Validate YAML (workflows, Dependabot, issue forms)

```bash
npx -y js-yaml .github/workflows/*.yml .github/dependabot.yml .github/ISSUE_TEMPLATE/*.yml mkdocs.yml
```

Or:

```bash
make lint-yaml
# or
task lint:yaml
```

## Preview the Documentation Site

Requires Python and `pip` (not currently installed in every contributor
environment — install separately per your OS):

```bash
pip install mkdocs-material
mkdocs serve
```

Or:

```bash
make docs-serve
# or
task docs:serve
```

## Run All Phase 1 Validation

```bash
make validate
# or
task validate
```

This runs Markdown lint and YAML lint. It will grow to include `cargo fmt`
/ `cargo clippy` / `cargo test` once Phase 2 introduces the first Rust
crate, and frontend lint/test once Phase 6 introduces the web app.

## Contribution Workflow

See [../../CONTRIBUTING.md](../../CONTRIBUTING.md) for branch naming,
commit conventions, and the pull-request checklist.
