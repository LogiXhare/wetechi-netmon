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
| FU-7 | Fix the 9 relative links that break `mkdocs build --strict` | A separate docs branch | Present on `main` since before the DCO/CI work and never caught, because the strict docs build runs only in CI (billing-blocked since day one) and is not part of `make validate`. Verified 2026-08-21: `origin/main` aborts with 9 warnings. Links to files outside `docs/` (`../../CONTRIBUTING.md`, `../../SECURITY.md`, source files) resolve correctly on GitHub but not in the built site, so the fix is a judgement call about which rendering wins — it needs its own branch, not a drive-by edit. |
| FU-6 | `clickhouse` 0.13.3 to 0.15.1 compatibility | A dependency-testing branch | Dependabot PR is open and deliberately unmerged. Two minor versions across a 0.x crate is a breaking-change risk for `crates/storage`; it needs its own branch and a real build, not a blind merge. |

## Why these are not GitHub issues yet

The repository's CI cannot run (FU-1), so an issue tracker would be the
only moving part in a repository where nothing else can be verified
automatically. These are recorded here, in the branch that created them,
and should be opened as issues once Actions is working again.
