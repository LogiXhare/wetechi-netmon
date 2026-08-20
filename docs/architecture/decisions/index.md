# Architecture Decision Records (ADRs)

Status: Phase 1 — template established, no decisions recorded yet.

## Purpose

An ADR captures one significant, hard-to-reverse architecture or
technology decision: the context, the options considered, the decision,
and its consequences. WetechiNetMon requires an ADR before implementation
begins for any decision flagged as an open "leaning" in
[docs/architecture-options.md](../../architecture-options.md) or
[docs/technology-options.md](../../technology-options.md).

## Process

1. Copy [0000-adr-template.md](0000-adr-template.md) to
   `NNNN-short-title.md`, where `NNNN` is the next sequential four-digit
   number.
2. Fill in every section — do not leave "Options Considered" as a single
   option; a decision with no real alternative considered is not an ADR.
3. Open a pull request. The ADR must be reviewed and merged **before** the
   code that depends on it, per `prompts/CLAUDE_MASTER_PROMPT.md` §30
   rule 20 ("Stop when a major architecture decision requires approval").
4. Once merged, an ADR is immutable history. If a decision changes later,
   write a new ADR that supersedes the old one — do not edit the original
   decision away.

## Known Upcoming ADRs

These are anticipated based on Phase 0 findings and are **not** decided
yet:

| Topic | Needed before | Reference |
|---|---|---|
| Collector implementation language (Rust vs. Go split) | Phase 2 | [architecture-options.md §3](../../architecture-options.md) |
| Event transport (NATS JetStream vs. Redpanda vs. Kafka) | Phase 3 | [architecture-options.md §2](../../architecture-options.md) |
| Mitigation Controller implementation language | Phase 7 | [architecture-options.md §3](../../architecture-options.md) |
| Recharts vs. Apache ECharts for the web UI | Phase 6 | [architecture-options.md §5](../../architecture-options.md) |
| WetechiNetMon's own open-source license | Phase 1 (blocking) | [blocking-questions.md BQ-1](../../blocking-questions.md) |

## Index

No ADRs have been recorded yet. This index will list them by number once
the first one is merged.
