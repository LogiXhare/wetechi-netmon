-- The detection-event link table. One row per detection event associated
-- with an incident; the `UNIQUE (tenant_id, dedup_key)` constraint is the
-- at-least-once-delivery duplicate gate (incident-persistence.md).
--
-- Column types mirror `wetechinetmon_incident::evidence::EvidenceReference`
-- (verified against crates/incident/src/evidence.rs): `detection_event_id`,
-- `dedup_key`, `detection_id` are all opaque strings there, and
-- `link_type` matches `EvidenceLinkType`'s four variants exactly --
-- incident-persistence.md's prose additionally lists a fifth,
-- `evidence`, which does not exist on `EvidenceLinkType` today; included
-- in the CHECK below as an explicitly reserved, not-yet-produced value
-- (same treatment as the timeline `entry_type` reserved values in
-- V4__incident_timeline.sql) rather than silently dropped.
CREATE TABLE incident_detection_events (
    incident_id UUID NOT NULL,
    detection_event_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    dedup_key TEXT NOT NULL,
    detection_id TEXT NOT NULL,
    policy_id TEXT NOT NULL,
    policy_version INTEGER NOT NULL,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL,
    detected_at TIMESTAMPTZ NOT NULL,
    matched JSONB NOT NULL,
    rates JSONB NOT NULL,
    link_type TEXT NOT NULL,

    PRIMARY KEY (incident_id, detection_event_id),
    UNIQUE (tenant_id, dedup_key),
    FOREIGN KEY (tenant_id, incident_id)
        REFERENCES incidents (tenant_id, incident_id) ON DELETE CASCADE,

    CHECK (severity IN ('info', 'minor', 'major', 'critical')),
    CHECK (link_type IN ('opening', 'update', 'closing', 'late', 'evidence'))
);

CREATE INDEX incident_detection_events_by_incident
    ON incident_detection_events (incident_id, observed_at DESC);
