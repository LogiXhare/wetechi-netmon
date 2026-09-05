-- Append-only incident timeline. `timeline_id` is `BIGINT GENERATED
-- ALWAYS AS IDENTITY` per ADR 0027 (closes FU-39 for this table);
-- `occurred_at`/`recorded_at` are `transaction_timestamp()`-sourced per
-- ADR 0031 (closes FU-35 for this table).
--
-- `entry_type` CHECK values are the union of two sources, verified
-- separately and reconciled here because they disagree:
--   - the 16 variants `wetechinetmon_incident::timeline::TimelinePayload`
--     actually implements today (verified against
--     crates/incident/src/timeline.rs, 2026-09-05), including
--     `tag_added`/`tag_removed` (FU-37) -- which
--     incident-persistence.md's own entry-type prose list omits, an
--     apparent documentation gap predating that fix;
--   - the additional values incident-persistence.md's prose explicitly
--     names as not-yet-implemented-but-reserved
--     (`note_superseded`, `evidence_added`, `recovery_detected`,
--     `recovery_aborted`, `resolved`, `closed`, `correlation_decision`,
--     `persistence_retry`) plus the two it explicitly says "nothing
--     writes in Phase 5" (`notification_result`, `mitigation_result`).
-- Pre-declaring the reserved values now (rather than only the 16 live
-- ones) means a later phase that starts writing one of them needs no
-- migration -- the same "no migration later" reasoning already applied
-- throughout incident-persistence.md's schema. `append_timeline` is a
-- pure INSERT-only path (below) so this CHECK is the only gate on what
-- can ever be written.
--
-- The append-only contract itself (no UPDATE, no DELETE) is enforced by
-- role grants, not by this migration -- see
-- V11__rls_ready_roles.sql, which the application's runtime role is
-- granted SELECT/INSERT but never UPDATE/DELETE on this table. A
-- migration file cannot itself "revoke from PUBLIC" meaningfully, since
-- the table owner (whichever role runs migrations) always bypasses
-- ordinary grants; the real control is which role the application
-- connects as.
CREATE TABLE incident_timeline (
    timeline_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    incident_id UUID NOT NULL,
    tenant_id TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    schema_version INTEGER NOT NULL,
    entry_type TEXT NOT NULL,
    actor_type TEXT NOT NULL,
    actor_id TEXT,
    correlation_id TEXT,
    command_id TEXT,
    source_event_id TEXT,
    previous_value JSONB,
    new_value JSONB,
    payload JSONB NOT NULL,

    FOREIGN KEY (tenant_id, incident_id)
        REFERENCES incidents (tenant_id, incident_id) ON DELETE CASCADE,

    CHECK (actor_type IN ('operator', 'system', 'service_account')),
    CHECK ((actor_type = 'system') = (actor_id IS NULL)),
    CHECK (entry_type IN (
        -- Implemented today (crates/incident/src/timeline.rs).
        'opened', 'event_linked', 'late_event_linked', 'duplicate_ignored',
        'state_changed', 'category_changed', 'severity_changed',
        'priority_changed', 'assignment_changed', 'note_added',
        'suppressed', 'unsuppressed', 'reopened', 'limit_reached',
        'tag_added', 'tag_removed',
        -- Reserved, not yet implemented (incident-persistence.md).
        'note_superseded', 'evidence_added', 'recovery_detected',
        'recovery_aborted', 'resolved', 'closed', 'correlation_decision',
        'persistence_retry', 'notification_result', 'mitigation_result'
    ))
);

CREATE INDEX incident_timeline_by_incident
    ON incident_timeline (incident_id, occurred_at, timeline_id);
