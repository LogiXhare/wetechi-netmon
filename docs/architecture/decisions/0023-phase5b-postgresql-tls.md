# 0023. Phase 5B PostgreSQL TLS

Status: **Conditionally Accepted** — pending the Phase 5B-1 dependency
probe
Date: 2026-08-24
Deciders: Repository owner

## Context

`tokio-postgres` does not implement TLS itself; a separate crate bridges
it to a TLS backend. A remote PostgreSQL connection needs one.

## Verified evidence (2026-08-24)

| | `rustls` (via `tokio-postgres-rustls`) | `native-tls` |
|---|---|---|
| `rustls` version | 0.23.43 | — |
| `tokio-postgres-rustls` version | 0.14.0 | — |
| License | Apache-2.0 OR ISC OR MIT (`rustls`); MIT (`tokio-postgres-rustls`) | Typically MIT OR Apache-2.0, but wraps the OS TLS stack |
| Advisories | 2 found in `rustsec/advisory-db`, **both patched**: RUSTSEC-2024-0336 (patched ≥0.23.5) and RUSTSEC-2024-0399 (patched ≥0.23.18) — current 0.23.43 is well past both | Not separately researched — rejected on the criterion below before advisory research was needed |
| Build dependency | Pure Rust, no system library | Links OpenSSL on Linux, Schannel on Windows, Secure Transport on macOS |

## Options Considered

### Option A — `rustls` via `tokio-postgres-rustls`

- Pros: pure Rust, **no OpenSSL build dependency** — decisive for a
  project whose primary development machine is Windows
  ([ADR 0018](0018-phase5-dependency-selection.md)'s "Windows + Linux:
  non-negotiable" criterion), where an OpenSSL build step is a
  historically common source of environment-specific breakage; both
  known advisories are patched well below the current version;
  consistent TLS behavior across Windows and Linux since it does not
  delegate to the OS TLS stack.
- Cons: one more dependency in the closure; less battle-tested against
  every possible server TLS configuration quirk than a system library
  with decades of real-world exposure.

### Option B — `native-tls`

- Pros: uses the platform's own TLS stack (Schannel on Windows,
  OpenSSL on Linux), which some operators trust more for FIPS or
  corporate-policy reasons.
- Cons: **requires a system OpenSSL installation or build on Linux**,
  and behaves differently across platforms by design — the opposite of
  the "Windows + Linux: non-negotiable, verify identically" posture this
  project has held since Phase 2's build tooling. Rejected primarily on
  this ground, not evaluated to advisory depth.

## Decision

**Option A, conditionally: `rustls` 0.23.43 + `tokio-postgres-rustls`
0.14.0.**

Requirements, binding regardless of implementation timing:

- **Development default:** TLS optional only for an isolated loopback
  test database (matches [ADR 0029](0029-phase5b-repository-and-unit-of-work-seam.md)'s
  test-database plan). Never optional for any non-loopback connection.
- **Production requirement:** full certificate chain validation and
  hostname verification, `sslmode=verify-full` or equivalent. **Never
  disable certificate verification in a production code path** — no
  `danger_accept_invalid_certs`-equivalent outside a clearly isolated
  test helper.
- **Custom CA:** supported via an explicit trust-store configuration,
  not a global "accept anything" flag.
- **Client certificates:** the design must not preclude them, though
  Phase 5B does not require mutual TLS by default.
- **Secrets:** TLS private keys and CA bundles are never committed to
  Git, consistent with existing R7 in
  [risk-register.md](../../risk-register.md).
- **TLS version:** whatever `rustls` 0.23.x negotiates by default (TLS
  1.2 minimum, 1.3 preferred) — no downgrade override.
- **Rotation:** certificate rotation is an operational procedure, not a
  code path; the design must not hardcode a certificate's validity
  assumption.
- **Failure behavior:** a TLS handshake failure is a connection failure,
  classified the same as any other unavailable-database condition (see
  the error-classification section of
  [incident-persistence.md](../incident-persistence.md)) — never a
  silent fallback to plaintext.

## Consequences

**Easier.** Identical TLS behavior on Windows and Linux, no OpenSSL
build-environment dependency.

**Harder.** One more dependency (two, counting `rustls` and the bridge
crate together) to track through `cargo audit`.

**Forecloses.** A `native-tls`-based deployment path is not built by
default; an operator requiring the OS TLS stack specifically (e.g. a
corporate PKI policy tied to Schannel) would need a documented
alternative build, not supported by this ADR.

**Security.** Both known `rustls` advisories verified patched at the
selected version. Certificate verification is never disabled in
production — stated explicitly per this task's own instruction and
consistent with this project's existing security posture.

**License.** `rustls`: Apache-2.0 OR ISC OR MIT. `tokio-postgres-rustls`:
MIT. Both compatible with the Apache-2.0 core.

## Follow-Up

- [ ] Run the Phase 5B-1 probe including both TLS crates in the measured
      closure.
- [ ] Document the exact production connection-string / configuration
      shape (`sslmode=verify-full`, CA path) in the operational runbook
      once Phase 5B-3 implements it.
