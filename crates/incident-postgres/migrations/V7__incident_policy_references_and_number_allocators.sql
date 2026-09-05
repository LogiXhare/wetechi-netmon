-- `incident_policy_references`: normalized replacement for 5A's
-- in-memory `Vec<PolicyRef>` capped at 64 with silent omission past the
-- cap (FU-34). A normalized relation has no inherent per-incident cap, so
-- no omitted-count column is added here -- there is nothing left to omit.
-- Column names mirror `wetechinetmon_incident::incident::PolicyRef`
-- exactly (verified against crates/incident/src/incident.rs).
CREATE TABLE incident_policy_references (
    incident_id UUID NOT NULL,
    tenant_id TEXT NOT NULL,
    policy_id TEXT NOT NULL,
    policy_version INTEGER NOT NULL,
    first_seen_sequence BIGINT NOT NULL,
    last_seen_sequence BIGINT NOT NULL,

    PRIMARY KEY (incident_id, policy_id),
    FOREIGN KEY (tenant_id, incident_id)
        REFERENCES incidents (tenant_id, incident_id) ON DELETE CASCADE,

    CHECK (last_seen_sequence >= first_seen_sequence)
);

-- `incident_number_allocators`: one row per tenant, holding the current
-- counter for the continuous per-tenant incident-number sequence (ADR
-- 0013's amendment, FU-24). Deliberately no foreign key to `incidents` --
-- the row exists before, during, and after any single incident's
-- lifetime; see incident-persistence.md's own reasoning for this table.
-- Allocation is `SELECT ... FOR UPDATE` plus a checked increment done by
-- the future repository (5B-3), not by this migration.
CREATE TABLE incident_number_allocators (
    tenant_id TEXT NOT NULL,
    next_value BIGINT NOT NULL DEFAULT 1,

    PRIMARY KEY (tenant_id),

    CHECK (next_value >= 1)
);
