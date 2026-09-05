-- Transactional outbox. Claim mechanics (`SELECT ... FOR UPDATE SKIP
-- LOCKED`, lease expiry via `locked_at` compared against a
-- caller-supplied lease interval, retry-then-dead-letter) are ADR 0033's
-- application-level responsibility (Milestone 5B-4) -- this migration
-- only provides the columns and the claim-supporting index the corrected
-- predicate in ADR 0033 needs:
--
--   SELECT outbox_id FROM incident_outbox
--   WHERE status IN ('pending', 'retrying')
--     AND available_at <= transaction_timestamp()
--     AND (locked_at IS NULL OR locked_at + :lease_interval <= transaction_timestamp())
--   ORDER BY outbox_id
--   FOR UPDATE SKIP LOCKED
--   LIMIT :batch_size
--
-- `outbox_id` is `BIGINT GENERATED ALWAYS AS IDENTITY` per ADR 0027
-- (closes FU-39 for this table), and doubles as the claim query's
-- deterministic `ORDER BY`.
--
-- No status value for "claimed"/"processing" exists deliberately -- ADR
-- 0033 is explicit that `locked_at` compared against the lease interval
-- is the sole lease marker, not a third status value kept in sync with
-- it.
CREATE TABLE incident_outbox (
    outbox_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_version BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    locked_at TIMESTAMPTZ,
    locked_by TEXT,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,

    CHECK (status IN ('pending', 'retrying', 'published', 'dead_letter')),
    CHECK (attempts >= 0),
    CHECK ((locked_at IS NULL) = (locked_by IS NULL)),
    CHECK (status <> 'published' OR published_at IS NOT NULL)
);

-- Matches incident-persistence.md's `incident_outbox` index sketch
-- exactly: `(status, available_at) WHERE status IN ('pending',
-- 'retrying')`.
CREATE INDEX incident_outbox_claimable
    ON incident_outbox (status, available_at)
    WHERE status IN ('pending', 'retrying');

-- Dead-letter: events that failed repeatedly or could not be parsed.
-- Deliberately no NOT NULL on `tenant_id`/`aggregate_*` and no foreign
-- key to `incidents` -- a row that could not be parsed may not reliably
-- contain a valid tenant or incident id at all, and "never auto-purged
-- while unreviewed" requires the row to exist unconditionally (see
-- incident-persistence.md's `incident_dead_letter` section).
-- `raw_payload` holds the original bytes when `payload` could not even be
-- parsed as JSON.
CREATE TABLE incident_dead_letter (
    dead_letter_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id TEXT,
    aggregate_type TEXT,
    aggregate_id TEXT,
    event_type TEXT,
    payload JSONB,
    raw_payload BYTEA,
    failure_reason TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    reviewed_at TIMESTAMPTZ,
    reviewed_by_type TEXT,
    reviewed_by_id TEXT,

    CHECK (attempts >= 0),
    CHECK (reviewed_by_type IS NULL OR reviewed_by_type IN ('operator', 'service_account', 'system')),
    CHECK ((reviewed_by_type IS NULL) = (reviewed_at IS NULL))
);

CREATE INDEX incident_dead_letter_unreviewed
    ON incident_dead_letter (first_seen_at)
    WHERE reviewed_at IS NULL;
