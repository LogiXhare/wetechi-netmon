# Capacity Planning

Status: Phase 3 — target defined, **not yet benchmarked**.

## Performance Target

> Sustain at least 100,000 normalized flow records per second on the
> documented test machine, without packet generation over public
> networks.

This is a **target**, not a benchmark result. No performance benchmark
has been executed against this target — per
`prompts/CLAUDE_MASTER_PROMPT.md` §30 rule 12 and the explicit Phase 3
instruction not to claim a performance target was met without an actual
benchmark run and recorded machine specifications, this document makes
no throughput claim. Benchmarking is a Phase 9 (production hardening)
deliverable; Phase 3 defines the target so Phase 9's benchmark has
something concrete to measure against.

## Memory Sizing (Aggregator)

Each aggregation dimension's worst-case memory is bounded by
`max_entries × (key size + TrafficCounters size)`. `TrafficCounters` is
12 × `u64` = 96 bytes. Approximate per-entry overhead (key + `HashMap`/
tracking overhead) varies by dimension:

| Dimension | Key | Approx. bytes/entry | Default `max_entries` | Approx. worst case |
|---|---|---|---|---|
| Hosts (v4+v6 combined) | `IpAddr` (up to 17 bytes) + counters | ~150 | 100,000 | ~15 MB |
| Networks (all prefix dimensions) | `(IpAddr, u8)` + counters | ~160 | 50,000 | ~8 MB |
| Hostgroups | `String` + counters | ~180 (varies with name length) | 1,000 | ~0.2 MB |
| ASNs | `u32` + counters | ~140 | 10,000 | ~1.4 MB |
| Exporters | `IpAddr` + counters | ~150 | 1,000 (default, not yet env-configurable) | ~0.15 MB |

These are architectural estimates from struct sizes, not measured
allocations — a real memory-under-load measurement is part of the Phase
9 benchmark, not asserted here as fact.

## Queue Memory

`WETECHINETMON_COLLECTOR_QUEUE_CAPACITY` (default 10,000) bounds the
in-process channel between UDP receive and classify/aggregate. Each
queued item is a raw datagram (`Vec<u8>`, up to `MAX_DATAGRAM_SIZE` =
65,535 bytes) plus a `SocketAddr`. Worst case: 10,000 × ~65KB ≈ 640MB if
every datagram were maximum-sized and the queue were completely full —
in practice, real IPFIX datagrams are far smaller (well under the 1500
byte path MTU), so actual queue memory under backpressure is expected to
be much lower. Not measured; documented as an upper bound from the
configuration, not an observed value.

## ClickHouse Export Data-Loss Trade-off

Per [ADR 0005](../architecture/decisions/0005-clickhouse-batching-and-retry.md):
under prolonged ClickHouse unavailability, this project **loses data by
design** rather than growing memory without bound. Specifically:

- The retry queue holds at most `RetryConfig::default().max_pending_batches`
  (100) batches. Once full, the **oldest** pending batch is dropped to
  make room for a new failure — not the newest.
- Each batch is retried at most `max_attempts` (5) times with exponential
  backoff (1s → 2s → 4s → 8s → 16s, capped at 60s) before being
  permanently dropped.
- At default settings (10,000 rows or 5s per batch, 100 pending batches),
  roughly 8–9 minutes of accumulated failed writes can be held before the
  oldest starts being dropped — a rough arithmetic estimate from the
  configured limits, not a measured outage-tolerance figure.

**Operator guidance:** if ClickHouse outages longer than this are
expected in your environment, either increase `RetryConfig`'s
`max_pending_batches` (accepting higher worst-case memory use during an
outage) or treat ClickHouse analytics data as best-effort and rely on
Prometheus metrics (which are not subject to this trade-off) for
operational alerting during an outage.

## What This Document Does Not Claim

- No sustained-throughput benchmark has been run.
- No memory-under-load measurement has been taken.
- No latency-under-load (P50/P95/P99) figures exist yet.

All of the above are legitimate Phase 9 deliverables once a documented
test machine and load-generation setup exist.
