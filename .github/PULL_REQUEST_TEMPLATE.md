# Pull Request

## Summary

<!-- What does this PR do and why? Link related issues. -->

## Phase / Scope

<!-- Which phase (docs/roadmap.md) does this belong to? Confirm it's in scope: docs/mvp-scope.md / docs/out-of-scope.md -->

## Checklist

### Always required

- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] Every commit is signed off under the [DCO](../DCO) (`git commit -s`)
- [ ] Documentation updated in this same PR (not deferred to a follow-up)
- [ ] No secrets, real customer data, or hardcoded credentials/IPs/ASNs added
- [ ] No fabricated test results, benchmark numbers, or security claims

### Clean-room self-certification (required for any protocol, detection, dashboard, schema, or CLI change)

- [ ] I have read [docs/clean-room-boundary.md](../docs/clean-room-boundary.md)
- [ ] This change does not copy, translate, reconstruct, or closely imitate any proprietary vendor's source code, configuration syntax, CLI syntax, dashboards, table definitions, or documentation
- [ ] This change was built from public RFCs/specifications, vendor documentation, or independent design — cite sources in the PR description if implementing a protocol or algorithm
- [ ] Product-facing text uses the approved description in docs/clean-room-boundary.md and does not describe this as a "clone", "replica", "alternative build", or similar

### If this PR adds a new dependency

- [ ] A row was added to [docs/dependency-license-matrix.md](../docs/dependency-license-matrix.md) with a completed license record
- [ ] License status is `Approved`, not left as `REQUIRES VERIFICATION`

### If this PR adds or changes application code (Phase 2+)

- [ ] Tests were added and pass locally
- [ ] `cargo fmt` / `cargo clippy` (or frontend equivalent) run clean
- [ ] New protocol parsers include fuzz/property tests
- [ ] Prometheus metrics and structured logs added for new failure modes

### If this PR touches BGP/mitigation logic (Phase 7+)

- [ ] Dry-run remains the default
- [ ] No change weakens the authorized-prefix allowlist or min/max prefix-length enforcement
- [ ] No real attack traffic or unauthorized-network testing was performed

## Testing Performed

<!-- Commands run and their actual results. Do not claim tests passed without running them. -->

## Risks / Follow-ups

<!-- Anything to flag for docs/risk-register.md, or explicit follow-up work. -->
