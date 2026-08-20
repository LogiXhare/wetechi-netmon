# 0002. Prefix Lookup Data Structure: Binary Trie (Radix Tree)

Status: Accepted
Date: 2026-08-20
Deciders: WeTechi Solutions (badshashorif)

## Context

The Direction Classifier (FR-3) and per-network aggregation (FR-2.2) both
need longest-prefix-match lookups against a set of configured local
prefixes (IPv4 and IPv6, tenant-scoped), on every decoded flow record.
This needs to be: deterministic (same input always yields the same
match), safe for adversarial/high-cardinality lookups, and support
duplicate/overlap detection at configuration time (FR-3.2).

## Options Considered

### Option A — Binary trie (radix tree / PATRICIA-style), one bit per level

A tree where each node represents one bit of the prefix; a lookup walks
the tree bit-by-bit from the address, tracking the most specific (longest)
matching prefix seen along the path.

- Pros: O(prefix length) lookup — 32 steps worst case for IPv4, 128 for
  IPv6, both small constants; longest-prefix-match falls out naturally
  from "keep the deepest marked node visited"; insertion trivially
  detects an exact-duplicate prefix (same node already marked) and an
  overlap (a marked ancestor or descendant node) by construction; no
  external crate needed — this is a well-understood, publicly documented
  structure (no proprietary reference).
- Cons: more implementation code than reaching for a general-purpose
  interval/tree crate; needs care to keep IPv4 and IPv6 tries genuinely
  separate (different max depth, different address types).

### Option B — Sorted `Vec` of prefixes, linear scan per lookup

Keep every configured prefix in a `Vec`, scan linearly on every lookup,
tracking the longest match.

- Pros: trivial to implement and reason about.
- Cons: O(n) per lookup where n is the number of configured prefixes —
  fine for a handful of prefixes, but this is exactly the kind of
  per-flow hot path where a large prefix list (many tenants, many sites)
  would degrade linearly; does not scale with the multi-tenant future
  this project is explicitly designed toward (NFR-3).

### Option C — Third-party crate (e.g. a generic IP-prefix-trie crate)

- Pros: less code to write and test ourselves.
- Cons: adds a new dependency requiring its own
  docs/dependency-license-matrix.md entry and security-maintenance
  review, for a data structure that is not hard to implement correctly
  and test exhaustively ourselves; also increases the audit surface for
  a component sitting in the direct hot path of untrusted-derived data
  (the prefixes themselves are operator-configured, not attacker-
  controlled, but the *lookups* run once per received flow).

## Decision

**Binary trie (Option A)**, implemented from scratch in
`crates/classifier`, with separate tries for IPv4 (32-bit) and IPv6
(128-bit). No new third-party dependency.

## Consequences

- Lookup cost is bounded and independent of the number of configured
  prefixes — matches the "no unbounded per-flow cost" spirit of NFR-1/
  NFR-3.
- Duplicate-prefix and overlapping-prefix detection (FR-3.2) become a
  direct property of trie insertion (has this exact node already been
  marked? does a marked ancestor/descendant already exist?), which is
  easier to test exhaustively than deriving the same checks from a flat
  list.
- This is now the reference structure any future prefix-adjacent feature
  (e.g. tenant prefix ownership queries) should reuse rather than
  re-implementing its own matching logic.
- No license/security implications — no new dependency.

## Follow-Up

- [x] Recorded here rather than left as a "leaning" in architecture-options.md.
- [ ] Revisit if/when a single deployment's prefix count grows large enough
      that trie memory overhead (not lookup speed) becomes the bottleneck
      — no evidence of that yet; not a Phase 3 concern.
