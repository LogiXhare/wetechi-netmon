-- Audit trail, separate from the timeline: records authorization
-- decisions including denials, which have no incident to attach to.
-- Deliberately no foreign key to `incidents` -- see
-- incident-persistence.md's `incident_audit` section for the full
-- reasoning (polymorphic `resource_type`/`resource_id`, denial can occur
-- before an incident is ever identified, and audit's retention must
-- outlive its incident's own retention).
--
-- `audit_id` is `BIGINT GENERATED ALWAYS AS IDENTITY` per ADR 0027
-- (closes FU-39 for this table); `occurred_at`/`recorded_at` are
-- `transaction_timestamp()`-sourced per ADR 0031 (closes FU-35).
--
-- Column-set note: `wetechinetmon_incident::audit::AuditEntry` (verified
-- against crates/incident/src/audit.rs) carries `schema_version,
-- sequence, tenant, actor, permission, resource, outcome, reason` only --
-- it has no `source_ip`, `user_agent`, `request_id`, `trace_id`, `before`,
-- or `after` fields. Those columns are kept here anyway, nullable,
-- because incident-persistence.md explicitly designs this table to also
-- carry request-context enrichment the future API layer (Phase 5D) will
-- supply, not only what the domain's `AuditEntry` produces -- audit rows
-- are assembled from more than one source, per the "Auditing a command
-- that was denied" section's two-write-path design. This is forward
-- schema, not a claim that 5B populates every column.
CREATE TABLE incident_audit (
    audit_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    tenant_id TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    actor_type TEXT NOT NULL,
    actor_id TEXT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    result TEXT NOT NULL,
    reason TEXT,
    source_ip INET,
    user_agent TEXT,
    request_id TEXT,
    trace_id TEXT,
    before JSONB,
    after JSONB,

    CHECK (actor_type IN ('operator', 'system', 'service_account')),
    CHECK ((actor_type = 'system') = (actor_id IS NULL)),
    CHECK (result IN ('allowed', 'denied', 'error'))
);

CREATE INDEX incident_audit_by_tenant_time
    ON incident_audit (tenant_id, occurred_at DESC);

CREATE INDEX incident_audit_by_resource
    ON incident_audit (tenant_id, resource_type, resource_id, occurred_at DESC);
