-- ADR 0035: the detection-event inbox. The detector's producer enqueues
-- each event once per (tenant_id, dedup_key); the correlation worker claims
-- rows under ADR 0033's lease query, ingests them, and records the outcome.
--
-- Deliberately separate from incident_outbox (V9), which carries events out
-- of the incident domain and is keyed by aggregate and version. A detection
-- event has no incident yet.
--
-- The queue columns (status, attempts, available_at, locked_at, locked_by,
-- last_error) mean exactly what they mean on incident_outbox. Every lease
-- and backoff comparison uses transaction_timestamp().
CREATE TABLE detection_event_inbox (
    inbox_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    dedup_key TEXT NOT NULL,
    detection_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    locked_at TIMESTAMPTZ,
    locked_by TEXT,
    last_error TEXT,
    outcome TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    processed_at TIMESTAMPTZ,
    CONSTRAINT detection_event_inbox_dedup UNIQUE (tenant_id, dedup_key),
    CHECK (status IN ('pending', 'retrying', 'processed', 'dead_letter')),
    CHECK (attempts >= 0),
    CHECK (schema_version >= 0),
    CHECK ((locked_at IS NULL) = (locked_by IS NULL)),
    CHECK (status <> 'processed' OR (processed_at IS NOT NULL AND outcome IS NOT NULL)),
    CHECK (outcome IS NULL OR outcome IN (
        'created', 'updated', 'reopened', 'linked_late', 'duplicate', 'quarantined', 'observe_only'
    ))
);

CREATE INDEX detection_event_inbox_claimable
    ON detection_event_inbox (status, available_at)
    WHERE status IN ('pending', 'retrying');

GRANT SELECT, INSERT, UPDATE, DELETE ON detection_event_inbox TO wetechinetmon_app;
