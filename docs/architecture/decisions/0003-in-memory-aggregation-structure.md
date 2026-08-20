# 0003. In-Memory Aggregation Structure: Bounded HashMap + Deterministic Eviction Ring

Status: Accepted
Date: 2026-08-20
Deciders: WeTechi Solutions (badshashorif)

## Context

FR-2.4 requires bounded memory, deterministic expiration, and
high-cardinality protection for aggregation across many dimensions
(hosts, networks, hostgroups, ASNs, exporters, interfaces, protocols).
Master prompt §7 explicitly calls out "bounded memory," "deterministic
expiration," and "configurable top-N limits" as hard requirements, not
aspirations — an aggregator that can be driven into unbounded memory
growth by a high-cardinality or adversarial flow stream is itself a
security/availability risk (see docs/security-principles.md, "high-
cardinality attacks").

## Options Considered

### Option A — Bounded `HashMap<Key, Counters>` per dimension, with an explicit max-entries limit and an inactivity-based eviction ring

Each aggregation dimension (per-host, per-network, per-hostgroup, ...) is
its own `HashMap` capped at a configurable maximum entry count. Every
entry records its last-updated time. A background sweep (or lazy
check-on-insert) evicts the least-recently-updated entries once the map
is at capacity, and independently expires entries that have been
inactive past a configurable TTL regardless of capacity pressure.

- Pros: eviction policy (LRU-by-last-update) is simple to reason about,
  test deterministically (insert entries with controlled timestamps,
  assert the right one gets evicted), and matches operator intuition
  ("the quietest host/network gets dropped first when we're full").
  Per-dimension caps let an operator size hosts vs. networks vs.
  hostgroups independently, matching FR-2.4's "configurable top-N
  limits."
- Cons: a true LRU needs either a doubly-linked list alongside the map or
  an O(n) scan to find the least-recently-updated entry; for the cap
  sizes this project targets (thousands, not millions, of concurrently
  tracked entries per dimension) an O(n) scan on the rare "at capacity"
  path is an acceptable, simple trade-off — optimizing this before it's
  measured to be a problem would be premature.

### Option B — Unbounded `HashMap`, rely on periodic external flush/reset

Let each dimension's map grow freely, periodically clearing it wholesale
when writing out to ClickHouse.

- Pros: simplest possible code.
- Cons: directly violates FR-2.4 ("bounded memory") and is exactly the
  high-cardinality-attack shape flagged in docs/security-principles.md —
  a burst of traffic to many distinct hosts/networks in the window before
  a flush could grow memory without bound. Rejected.

### Option C — Probabilistic/approximate structures (e.g. count-min sketch, HyperLogLog) for high-cardinality dimensions

- Pros: constant memory regardless of true cardinality; used by some
  large-scale flow-analytics systems for exactly this problem.
- Cons: gives approximate, not exact, per-key counters — acceptable for
  cardinality *estimation* but not for the per-host/per-network traffic
  totals this phase needs to report accurately and write to ClickHouse.
  Worth revisiting later specifically for very-high-cardinality
  dimensions (e.g. "distinct source IPs seen," not "bytes per source IP")
  if that need arises — not a Phase 3 requirement today.

## Decision

**Option A** — per-dimension bounded `HashMap` with a configurable
max-entries cap and last-updated-based eviction, plus an independent
inactivity TTL. Implemented in `crates/aggregator`.

## Consequences

- Memory usage per dimension is bounded by `max_entries × size_of(Key +
  Counters)`, a number an operator can reason about and configure (FR-2.4,
  master-prompt §21 "maximum tracked hosts/networks" controls).
- Eviction and expiration are two independent, separately testable
  mechanisms: capacity-triggered eviction (deterministic — evict the
  least-recently-updated entry) and time-triggered expiration (an entry
  untouched for longer than the configured TTL is removed regardless of
  overall map fullness).
- No new third-party dependency; no license implications.
- This structure is reused identically across every aggregation
  dimension (host, network, /24, hostgroup, ASN, exporter, interface,
  protocol) rather than inventing a bespoke structure per dimension —
  keeps the eviction/expiration logic in one place to test once, not once
  per dimension.

## Follow-Up

- [ ] If profiling later shows the O(n) eviction scan is a real bottleneck
      at production cap sizes, revisit with a proper LRU (e.g. an
      intrusive linked list alongside the map) — not needed until
      measured.
- [ ] Revisit probabilistic structures (Option C) if a future phase needs
      true high-cardinality *distinct-count* estimation rather than exact
      per-key totals.
