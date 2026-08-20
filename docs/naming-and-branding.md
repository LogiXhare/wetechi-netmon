# Naming and Branding

Status: Phase 0 draft — binding decision
Last updated: 2026-08-20

## Approved Identity

| Item | Value |
|---|---|
| Company | WeTechi Solutions |
| Product | WetechiNetMon |
| Core detection/analytics engine | SentinelFlow Engine |
| CLI | `wetechinetmonctl` |
| Tagline | "See Every Flow. Defend Every Network." |
| Repository | `wetechi-netmon` |
| GitHub organization | `wetechi` |
| Service namespace | `wetechinetmon` |

## Container Images

- `ghcr.io/wetechi/wetechi-netmon-collector`
- `ghcr.io/wetechi/wetechi-netmon-aggregator`
- `ghcr.io/wetechi/wetechi-netmon-detector`
- `ghcr.io/wetechi/wetechi-netmon-api`
- `ghcr.io/wetechi/wetechi-netmon-web`
- `ghcr.io/wetechi/wetechi-netmon-mitigator`

## Rationale

- All names are original to WeTechi Solutions and do not reuse any
  proprietary vendor's name, trademark, repository identity, package name,
  CLI name, UI terminology, service name, or dashboard identity — per the
  clean-room boundary (see [clean-room-boundary.md](clean-room-boundary.md)).
- `wetechinetmonctl` (rather than any shortened form resembling a known
  competitor's CLI name) makes the product/company relationship obvious
  and avoids any naming collision or confusable-similarity risk.
- "SentinelFlow Engine" is used specifically for the detection/analytics
  core, distinct from the product name, to allow independent evolution
  (and potential separate licensing/packaging) of the detection engine
  from the rest of the platform.

## Rules for All Future Contributors (Human or Agent)

1. Never use "FastNetMon" or any other proprietary product's name, in code,
   comments, commit messages, documentation, UI strings, dashboard titles,
   metric names, or CLI help text — including as a comparison ("like X",
   "compatible with X's config format").
2. Never describe WetechiNetMon as a clone, replica, copy, alternative
   build, reverse-engineered edition, or replacement edition of any
   product, named or unnamed.
3. Always use the approved product description from
   [clean-room-boundary.md](clean-room-boundary.md) in product-facing
   copy.
4. Dashboard UIDs, panel layouts, table names, and CLI command names must
   be independently designed, not copied from any reference product's
   documentation or screenshots.
5. Any proposed new branded sub-component name (e.g., a future module)
   should follow the `Wetechi<Noun>` or `<Descriptive><Noun> Engine`
   pattern established by "WetechiNetMon" and "SentinelFlow Engine" —
   this is a style convention, not a hard rule, and can be revisited.

## Trademark Note

This document records a naming *decision*, not a trademark clearance. No
trademark search has been performed as part of Phase 0. If WeTechi
Solutions intends to register "WetechiNetMon," "SentinelFlow," or
"wetechinetmonctl" as a trademark, or to publish under the `wetechi`
GitHub organization publicly, a proper trademark clearance search is
recommended before public launch — this is noted for awareness, not
treated as a Phase 0 blocking question, since it doesn't block engineering
work.
