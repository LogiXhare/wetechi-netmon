# License Recommendation — REQUIRES LEGAL VERIFICATION

Status: Phase 1 — recommendation only, not a final decision
Last updated: 2026-08-20

## ⚠️ Warning

The [`LICENSE`](../LICENSE) file at the repository root currently contains
the standard **Apache License 2.0** text. This is a **recommendation
selected by the engineering process**, not a confirmed legal decision by
WeTechi Solutions. It is placed in the repository now so the repository is
structurally complete and so CI/tooling that expects a `LICENSE` file
functions correctly — it must not be read as final.

This is tracked as an open item since Phase 0:
[`docs/blocking-questions.md` — BQ-1](blocking-questions.md).

**Do not treat this repository's license as settled until WeTechi
Solutions explicitly confirms it**, ideally with input from qualified
legal counsel, especially given the commercial-tier strategy discussion in
[`docs/commercial-boundaries.md`](commercial-boundaries.md).

## Why Apache-2.0 Was Recommended

- Widely adopted for infrastructure/networking software; low friction for
  enterprise and ISP adoption (a named target audience — see
  [`docs/product-charter.md`](product-charter.md)).
- Includes an explicit patent grant and patent-retaliation clause, which
  matters for a security/networking product where patent risk is
  non-trivial.
- Permissive — does not, by itself, force WetechiNetMon's own source to
  stay open if WeTechi Solutions later wants closed-source Enterprise
  modules built *alongside* (not modifying) the Apache-licensed core.
- Compatible with the majority of the candidate dependencies listed in
  [`docs/dependency-license-matrix.md`](dependency-license-matrix.md)
  (most are MIT or Apache-2.0 themselves).

## Why This Still Needs Real Verification

1. **Competitive exposure**: Apache-2.0 is permissive enough that a
   competitor could take the entire codebase and offer a competing managed
   DDoS-mitigation service without contributing anything back. If WeTechi
   Solutions' business model depends on being the only viable managed-
   service provider for this code, a source-available or dual-license
   model may be a better fit — this is a business decision, not an
   engineering one.
2. **Interaction with Grafana's AGPLv3 terms**: if the eventual
   integration approach changes from "external Grafana, original
   dashboard JSON only" to something that bundles or modifies Grafana
   itself, Apache-2.0 for WetechiNetMon's own code would not resolve that
   separate AGPLv3 exposure — see
   [`docs/dependency-license-matrix.md`](dependency-license-matrix.md).
3. **No legal review has occurred.** This document and the `LICENSE` file
   were prepared by the engineering/documentation process described in
   `prompts/CLAUDE_MASTER_PROMPT.md`, which explicitly states license
   information must never be fabricated and uncertain status must be
   marked `REQUIRES VERIFICATION` — this whole license selection is that
   marker.

## What Happens Once WeTechi Solutions Decides

- If Apache-2.0 is confirmed: this document is updated to remove the
  warning and record the confirmation date/decision-maker; the `LICENSE`
  file itself needs no change.
- If a different license is chosen: the `LICENSE` file is replaced with
  the new license's standard text, `NOTICE` is updated accordingly, and
  this document records the change with rationale (effectively becoming
  the first real ADR-style record for this decision — see
  [`docs/architecture/decisions/`](architecture/decisions/index.md)).

## Related Documents

- [`docs/blocking-questions.md`](blocking-questions.md) — BQ-1
- [`docs/commercial-boundaries.md`](commercial-boundaries.md)
- [`docs/dependency-license-matrix.md`](dependency-license-matrix.md)
- [`NOTICE`](../NOTICE)
