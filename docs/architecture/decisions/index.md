# Architecture Decision Records (ADRs)

Status: Phase 3 — five decisions recorded.

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
| Mitigation Controller implementation language | Phase 7 | [architecture-options.md §3](../../architecture-options.md) |
| Recharts vs. Apache ECharts for the web UI | Phase 6 | [architecture-options.md §5](../../architecture-options.md) |
| NATS JetStream transport (deferred from Phase 3, see ADR 0004) | When Aggregator needs to scale independently | [0004](0004-collector-aggregator-event-transport.md) |

## Index

| # | Title | Status |
|---|---|---|
| [0001](0001-collector-implementation-language.md) | Telemetry Collector Implementation Language: Rust | Accepted |
| [0002](0002-prefix-lookup-data-structure.md) | Prefix Lookup Data Structure: Binary Trie | Accepted |
| [0003](0003-in-memory-aggregation-structure.md) | In-Memory Aggregation Structure: Bounded HashMap + Eviction | Accepted |
| [0004](0004-collector-aggregator-event-transport.md) | Collector-to-Aggregator Event Transport: In-Process Channel (NATS Deferred) | Accepted |
| [0005](0005-clickhouse-batching-and-retry.md) | ClickHouse Batching and Retry Behavior | Accepted |
| [0006](0006-contribution-licensing-dco-not-cla.md) | Contribution Licensing: DCO Sign-Off, No CLA | Accepted |
