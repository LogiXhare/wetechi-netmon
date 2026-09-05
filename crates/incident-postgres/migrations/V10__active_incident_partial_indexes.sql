-- The active-incident invariant: three target-type-specific partial
-- unique indexes, not one generated canonical-key column. Verbatim from
-- incident-persistence.md's "Active-incident invariant" section (which
-- itself corrects an earlier, incorrect single-index
-- `WHERE state <> 'closed'` sketch -- see that section's own "corrected
-- 2026-08-24" note; `Resolved` is not an active state for correlation,
-- so a predicate of `state <> 'closed'` would wrongly block a legitimate
-- reopen-window recurrence).
--
-- The active states are exactly: open, acknowledged, investigating,
-- monitoring, recovering. Resolved and closed are not active -- hence
-- `state NOT IN ('resolved', 'closed')` below, not `state <> 'closed'`.
--
-- This is a separate, later migration from V2__incidents.sql
-- deliberately: it is the single invariant every ADR in this milestone
-- (0026's operation-isolation matrix, 0033's outbox design note on "the
-- second insert fails loudly") depends on existing, so it gets its own
-- migration and its own review attention rather than being one clause
-- among many in the base table's creation script.
CREATE UNIQUE INDEX incidents_active_host
    ON incidents (tenant_id, target_addr, direction, address_family)
    WHERE target_type = 'host'
      AND state NOT IN ('resolved', 'closed');

CREATE UNIQUE INDEX incidents_active_network
    ON incidents (tenant_id, target_network, direction, address_family)
    WHERE target_type = 'network'
      AND state NOT IN ('resolved', 'closed');

CREATE UNIQUE INDEX incidents_active_hostgroup
    ON incidents (tenant_id, target_hostgroup, direction)
    WHERE target_type = 'hostgroup'
      AND state NOT IN ('resolved', 'closed');
