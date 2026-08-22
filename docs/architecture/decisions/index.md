# Architecture Decision Records (ADRs)

Status: Phase 5 planning — seventeen decisions recorded; 0011-0017 are
Proposed and await owner review.

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
| [0007](0007-detection-engine-cannot-mitigate.md) | The Detection Engine Cannot Mitigate | Accepted |
| [0008](0008-detection-policy-configuration.md) | Detection Policy Configuration: JSON, Not YAML | Accepted |
| [0009](0009-detection-event-identity.md) | Detection Event Identity and Deduplication | Accepted |
| [0010](0010-detector-owns-its-windowed-counters.md) | The Detector Keeps Its Own Windowed Counters | Accepted |
| [0011](0011-incident-domain-boundary.md) | The Incident Domain Is Separate From the Detection Domain | Proposed |
| [0012](0012-incident-event-ingestion.md) | Detection-Event Ingestion: Transactional Outbox, At-Least-Once | Proposed |
| [0013](0013-incident-identity.md) | Incident Identity and the Human-Readable Incident Number | Proposed |
| [0014](0014-incident-state-machine.md) | The Incident State Machine Does Not Mirror the Detector's | Proposed |
| [0015](0015-incident-operational-storage.md) | PostgreSQL Is the Operational Source of Truth for Incidents | Proposed |
| [0016](0016-incident-concurrency-and-idempotency.md) | Optimistic Concurrency and Fingerprinted Idempotency | Proposed |
| [0017](0017-incident-community-enterprise-boundary.md) | The Community/Enterprise Seam Is an Extension Point, Not a Limitation | Proposed |
