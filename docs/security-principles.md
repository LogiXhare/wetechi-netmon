# Security Principles

Status: Phase 0 draft
Last updated: 2026-08-20

## 1. Threat Model (initial — expanded formally in Phase 9)

| Threat | Notes |
|---|---|
| Malformed flow packets | Primary attack surface — collector parses untrusted UDP input from exporters |
| Parser vulnerabilities | Mitigated by Rust + no unsafe without review + fuzzing (Phase 2) |
| Template-cache poisoning | Exporter-sent templates must be validated and scoped per exporter/observation-domain |
| Exporter spoofing | Requires exporter allowlisting; authentication where technically possible |
| UDP collector DoS | Bounded queues, backpressure, rate limiting at socket layer |
| High-cardinality attacks | Bounded memory, top-N limits, configurable cardinality protection (NFR-3) |
| Queue exhaustion | Event transport must have bounded backlog and observable lag metrics |
| Database exhaustion | Write-rate limits, retention/TTL enforcement, alerting on write failures |
| API abuse | Rate limiting, authN/authZ, structured errors that don't leak internals |
| Authentication attacks | Argon2 password hashing, MFA compatibility, token rotation |
| Authorization bypass | RBAC enforced server-side at every layer, not just UI |
| Tenant escape | Tenant scoping enforced in API, DB queries, and audit — tested explicitly (multi-tenant isolation tests) |
| Privilege escalation | Least-privilege roles, no implicit admin defaults |
| Secret leakage | No secrets in Git; redaction in logs; secret managers only |
| Webhook SSRF | Outbound webhook targets validated/restricted; no fetching of arbitrary internal URLs on operator input |
| Log injection | Structured logging (not string concatenation) prevents log-forging |
| Dashboard injection | Grafana dashboard JSON validated in CI; no unsanitized user input rendered into dashboards |
| BGP route leaks | Authorized-prefix allowlist, max announcement scope, min/max prefix length enforcement |
| Overly broad blackhole routes | Prefix-length bounds enforced by the Mitigation Controller, not just documented |
| Stale mitigation routes | Automatic withdrawal, restart reconciliation, max mitigation duration |
| Compromised CI/CD | Pinned action versions, least-privilege tokens, environment approval gates for production deploys |
| Dependency compromise | Dependency review, vulnerability scanning, SBOM generation, signed images |
| Backup compromise | Backups encrypted at rest where supported, access-controlled, integrity-verified on restore |
| Restore compromise | Restore tested (not assumed), restore path itself access-controlled and audited |

## 2. Core Principles

1. **Least privilege everywhere** — service accounts, database roles, API
   scopes, RBAC roles.
2. **Rootless services where practical**, dedicated service accounts,
   read-only filesystems, seccomp/AppArmor on Linux deployments.
3. **TLS required** on all external interfaces; **optional mTLS**
   internally between services.
4. **Secrets never in Git.** Environment variables, secret managers,
   GitHub environment secrets, Kubernetes/Docker secrets, or clearly
   documented external configuration only.
5. **Signed artifacts and SBOMs** for every release starting from the
   first packaged release (Phase 9/10), not retrofitted.
6. **Dependency pinning** and reproducible builds where possible.
7. **Input validation** at every trust boundary — especially the flow
   collector's UDP input, which is the platform's largest untrusted-input
   surface.
8. **Rate limiting** on the public API and notification/webhook paths.
9. **Audit logging** for every state-changing operator or automation
   action, especially anything touching BGP/mitigation.
10. **Secure defaults** — BGP mitigation ships disabled and dry-run by
    default; this is a safety principle as much as a security one (see
    NFR-5 in [non-functional-requirements.md](non-functional-requirements.md)).

## 3. Mitigation-Specific Safety Controls (restated from master prompt §11)

- Dry-run by default; BGP disabled by default.
- Authorized-prefix allowlist and tenant prefix ownership required before
  any real announcement.
- Manual approval required for the first production mitigation.
- Emergency global disable switch.
- Maximum mitigation duration with automatic withdrawal.
- Idempotency and duplicate-action protection on every mitigation action.
- Complete audit trail for every announce/withdraw.
- **Never enable or test mitigation against unauthorized networks.**
- **Never generate real attack traffic** — synthetic/sanitized telemetry
  only, in lab or authorized environments only.

## 4. Security Testing Requirements (summary — full list in master prompt §24)

Fuzz tests and property-based tests for all protocol parsers; malformed-
packet tests; BGP dry-run and reconciliation tests; unauthorized-prefix
tests; multi-tenant isolation tests; RBAC tests; API authorization tests.
None of these are optional or deferred past the phase that introduces the
corresponding feature.

## 5. What Phase 0 Does Not Do

This document does not perform a security review of code (none exists
yet), does not select specific cryptographic libraries (deferred to the
phase that needs them), and does not claim any security control is
implemented — it states the principles new code must be held to.
