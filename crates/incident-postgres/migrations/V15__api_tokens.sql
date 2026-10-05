-- ADR 0038 (gate 4): API tokens for the incident REST API.
--
-- Only SHA-256 of each token is stored. The token itself, `wnm_` and 64 hex
-- characters (256 bits from the OS CSPRNG), is shown once, when it is
-- created. A leak of this table yields no usable credential, and a token
-- with 256 bits of entropy needs no salt or slow hash.
--
-- A token is scoped to one tenant and acts as one operator or service
-- account with one tenant role. platform_admin is not a permitted role:
-- cross-tenant authority never travels over the API.
--
-- Every token expires (at most 366 days after creation), and revocation is
-- a timestamp, so the history of who held access stays.
CREATE TABLE api_tokens (
    token_id UUID PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_type TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    role TEXT NOT NULL,
    token_hash BYTEA NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    CONSTRAINT api_tokens_hash_unique UNIQUE (token_hash),
    CHECK (octet_length(token_hash) = 32),
    CHECK (char_length(tenant_id) BETWEEN 1 AND 128),
    CHECK (actor_type IN ('operator', 'service_account')),
    CHECK (char_length(actor_id) BETWEEN 1 AND 128),
    CHECK (role IN ('viewer', 'operator', 'senior_operator', 'noc_lead')),
    CHECK (char_length(description) <= 200),
    CHECK (expires_at > created_at),
    CHECK (expires_at <= created_at + interval '366 days'),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX api_tokens_tenant ON api_tokens (tenant_id, created_at);
