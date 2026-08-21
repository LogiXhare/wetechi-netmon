# Engineering Follow-Ups

Work that is known, scoped, and deliberately not done yet — each entry
says what blocks it. This is not the product roadmap
([roadmap.md](../roadmap.md) covers phases); it is the list of loose ends
that would otherwise live only in a maintainer's head.

Status: opened 2026-08-21 alongside the DCO and CI-consolidation work.

| # | Item | Blocked on | Notes |
|---|---|---|---|
| FU-1 | Resolve GitHub Actions billing | Account owner | Actions has been unable to start jobs since 2026-08-20; every run fails within seconds with "recent account payments have failed or your spending limit needs to be increased". No workflow change can fix this. |
| FU-2 | First remote execution of the consolidated workflow | FU-1 | `.github/workflows/validate.yml` was restructured from nine jobs to three and has been statically and locally validated only. It has never executed on a GitHub-hosted runner. |
| FU-3 | Enable nightly `cargo-fuzz` | A nightly Rust toolchain | The fuzz target (`crates/protocol-ipfix/fuzz/`) and its scheduled workflow exist from Phase 3 but have never been run. |
| FU-4 | Test the pre-push hook on Linux, WSL, Git Bash, and macOS | Access to each environment | `.githooks/pre-push` has been exercised on Git Bash only. `.gitattributes` pins it to LF specifically so the other platforms work, but that is reasoning, not evidence. |
| FU-5 | Revisit CLA vs. DCO | The first external contribution | See [ADR 0006](../architecture/decisions/0006-contribution-licensing-dco-not-cla.md). Reversible only while the contributor list is empty. |
| FU-7 | ~~Fix the 9 relative links that break `mkdocs build --strict`~~ | **Done 2026-08-21** | Fixed on `chore/dco-and-ci-consolidation`. Two were genuine wrong-depth paths; the rest pointed outside `docs/`. Root governance files now have documentation-native pages ([Contributing](contributing.md), [Security Policy](../security/security-policy.md)) and source/README references use explicit repository URLs, which `.github/mlc_config.json` ignores because the repository is private. `mkdocs build --strict` now exits 0 with zero warnings. |
| FU-6 | `clickhouse` 0.13.3 to 0.15.1 compatibility | A dependency-testing branch | Dependabot PR is open and deliberately unmerged. Two minor versions across a 0.x crate is a breaking-change risk for `crates/storage`; it needs its own branch and a real build, not a blind merge. |
| FU-8 | Decide whether DCO should also be enforced on direct pushes | A branch-protection decision | The check runs on `pull_request` only, so a commit pushed straight to `main` is never checked. Deliberate and documented in `CONTRIBUTING.md` and in the workflow, not an oversight. The durable fix is branch protection requiring the `history` check before merge, which forces changes through pull requests — but branch protection needs a public repository or GitHub Pro, so it is a project decision, not a code change. |

## Why these are not GitHub issues yet

The repository's CI cannot run (FU-1), so an issue tracker would be the
only moving part in a repository where nothing else can be verified
automatically. These are recorded here, in the branch that created them,
and should be opened as issues once Actions is working again.
