-- Durable idempotency store, replacing 5A's unbounded in-memory
-- `IdempotencyStore` (FU-33). `request_fingerprint` is `BYTEA`, not
-- `TEXT` -- confirmed per ADR 0028's own follow-up: it stores exactly the
-- canonical JSON bytes `wetechinetmon_incident::idempotency::RequestFingerprint::of`
-- already produces (crates/incident/src/idempotency.rs), compared by
-- direct byte equality, never hashed.
--
-- Key length bound (16-255) matches
-- `IDEMPOTENCY_KEY_MIN_LEN`/`IDEMPOTENCY_KEY_MAX_LEN` in
-- crates/incident/src/idempotency.rs, so a key `IdempotencyKey::new`
-- would reject can never reach this table either.
CREATE TABLE incident_idempotency (
    tenant_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    operation TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    request_fingerprint BYTEA NOT NULL,
    response_status TEXT NOT NULL,
    response_body_ref TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    expires_at TIMESTAMPTZ NOT NULL,

    PRIMARY KEY (tenant_id, idempotency_key),

    CHECK (char_length(idempotency_key) BETWEEN 16 AND 255)
);

CREATE INDEX incident_idempotency_expires_at
    ON incident_idempotency (expires_at);
