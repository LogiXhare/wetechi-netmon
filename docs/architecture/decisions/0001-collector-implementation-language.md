# 0001. Telemetry Collector Implementation Language: Rust

Status: Accepted
Date: 2026-08-20
Deciders: WeTechi Solutions (badshashorif)

## Context

`prompts/CLAUDE_MASTER_PROMPT.md` §6 requires an explicit ADR comparing
Rust and Go for the Telemetry Collector before implementation begins,
evaluated on memory safety, parser safety, fuzzing support, concurrency,
performance, ecosystem maturity, ease of deployment, long-term
maintenance, and developer availability. This decision gates Phase 2
(IPFIX collector MVP).

[docs/architecture-options.md §3](../../architecture-options.md) already
recorded a preliminary "leaning" toward Rust for parsing specifically,
while leaving the Mitigation Controller's language (§3, same document) as
a separate, later decision because GoBGP is itself a Go library. This ADR
formalizes the collector-specific half of that leaning; it does not decide
the Mitigation Controller's language (tracked separately, due before
Phase 7).

The Telemetry Collector's defining characteristic is that it parses
**untrusted, attacker-reachable UDP input** (IPFIX/NetFlow/sFlow packets
from network exporters) at high volume. That single fact dominates this
decision more than any other criterion.

## Options Considered

### Option A — Rust

- Pros:
  - Compile-time memory safety with no garbage collector, so no GC-pause
    jitter under sustained high packet rates — directly relevant to the
    NFR-1 bounded-latency goal in
    [docs/non-functional-requirements.md](../../non-functional-requirements.md).
  - Exhaustive `match` on untrusted byte layouts, and the type system
    makes it comparatively hard to silently read past a buffer boundary
    when parsing template/data records — parser safety is the single
    highest-priority property for this component (see the threat model
    in [docs/security-principles.md](../../security-principles.md):
    malformed packets and parser vulnerabilities are the top two listed
    threats).
  - Mature, first-class fuzzing story (`cargo-fuzz`/`libFuzzer`,
    `proptest`) that master prompt §6 explicitly mandates for every
    protocol parser.
  - `tokio` gives fine-grained async control suitable for a
    single-process, many-exporter UDP listener with per-exporter template
    caches.
  - No `unsafe` is needed for straightforward byte-parsing logic, and the
    project rule (master prompt §6, restated in
    [docs/security-principles.md](../../security-principles.md)) is "no
    `unsafe` without a documented, reviewed reason" — Rust makes that an
    enforceable default rather than a convention.
- Cons:
  - Smaller contributor pool than Go; steeper ramp-up for new contributors
    unfamiliar with the borrow checker.
  - Slower initial development velocity than Go for straightforward CRUD-
    style code (less relevant here — the collector is not CRUD-style).
  - Async Rust (`tokio`) has more conceptual overhead than Go's goroutines.

### Option B — Go

- Pros:
  - Larger, more available contributor pool; faster onboarding.
  - Goroutines are simpler to reason about than async Rust for a
    straightforward "read socket, dispatch to handler" loop.
  - Native fuzzing since Go 1.18 (`go test -fuzz`) is workable, though
    less battle-tested for this exact use case than `cargo-fuzz`.
  - Static binaries and small footprint, comparable to Rust for
    deployment.
- Cons:
  - Garbage-collected: under sustained high flow-record rates, GC pauses
    are a latency risk for a component whose job is to not drop or delay
    packets. This is the decisive con — it works directly against
    NFR-1/NFR-8 (bounded, observable latency).
  - Historically more prone to panics from index-out-of-range or silent
    integer-overflow bugs in hand-rolled binary parsers than Rust's
    pattern-matched, bounds-checked approach — a real concern given the
    collector's parser-safety threat model is the top risk in
    [docs/security-principles.md](../../security-principles.md).
  - No `unsafe`-style compiler enforcement of memory-safety discipline;
    relies more on reviewer diligence for a component parsing untrusted
    input.

## Decision

**The Telemetry Collector (crates/collector, crates/protocol-ipfix,
crates/protocol-netflow, crates/protocol-sflow, crates/aggregator,
crates/classifier) is implemented in Rust.**

The decisive factors are parser safety and latency predictability under
untrusted, attacker-reachable input — both of which Rust's ownership model
and lack of GC address more directly than Go for this specific component.
Go's larger contributor pool and simpler concurrency model are real
advantages, but they matter less for a component whose primary risk is
memory-safety bugs in binary parsing, not developer ramp-up speed.

This decision applies to the **collector and everything upstream of
storage** (parsing, aggregation, classification). It does **not** extend
automatically to every other crate — `crates/mitigator` in particular has
a legitimate case for Go given GoBGP's native Go ecosystem, and is left
to its own ADR before Phase 7, per
[docs/architecture-options.md §3](../../architecture-options.md).

## Consequences

- Phase 2 crates (`collector`, `protocol-ipfix`, `protocol-netflow`,
  `protocol-sflow`, `common`) are Rust crates in a Cargo workspace,
  created starting with this phase.
- `cargo-fuzz` and `proptest` become required Phase 2 tooling; CI must run
  them (currently blocked by the account-level Actions billing issue — see
  memory note; this ADR's consequence is unaffected by that temporary
  operational blocker).
- Contributors need a working Rust toolchain. This repository currently
  targets the **GNU** toolchain (`x86_64-pc-windows-gnu` on Windows dev
  machines) specifically to avoid requiring a separate multi-gigabyte
  Visual Studio C++ Build Tools install for the MSVC toolchain — documented
  in [docs/development/local-setup.md](../../development/local-setup.md).
  Linux/macOS CI runners use the standard `x86_64-unknown-linux-gnu` /
  `aarch64-apple-darwin` targets, which need no special toolchain choice.
- No security, license, or clean-room implications beyond what's already
  documented: Rust itself is dual MIT/Apache-2.0
  (see [docs/dependency-license-matrix.md](../../dependency-license-matrix.md)
  row 1), and using it does not touch the clean-room boundary — the
  parsers are still independently implemented from public RFCs regardless
  of language.

## Follow-Up

- [x] `docs/architecture-options.md` §3 already recorded this leaning;
      no further edit needed there — this ADR is the formal record it
      pointed to.
- [ ] `docs/risk-register.md` R4 (collector parser vulnerability) should
      reference this ADR once fuzzing is actually running in Phase 2.
- [x] Linked from `docs/development/local-setup.md` (toolchain choice).
