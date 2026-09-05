-- Notes (immutable with supersession), tags (set semantics), and
-- assignment history (append-only) -- three tables, all tenant-scoped and
-- all referencing their incident through the tenant-aware composite
-- foreign key (incident-persistence.md's "Tenant-aware composite foreign
-- keys" section).

-- Notes. `note_id` is a durable, globally unique `BIGINT GENERATED
-- ALWAYS AS IDENTITY` row id; `note_index` is the domain's own
-- contiguous per-incident position (`wetechinetmon_incident::incident::Note.index`,
-- verified against crates/incident/src/incident.rs) that
-- `Incident::reconstitute` checks is gapless from zero -- the two serve
-- different purposes and are both kept.
CREATE TABLE incident_notes (
    note_id BIGINT GENERATED ALWAYS AS IDENTITY,
    incident_id UUID NOT NULL,
    tenant_id TEXT NOT NULL,
    note_index INTEGER NOT NULL,
    body TEXT NOT NULL,
    visibility TEXT NOT NULL,
    created_by_type TEXT NOT NULL,
    created_by_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    supersedes_note_id BIGINT,
    superseded_at TIMESTAMPTZ,
    superseded_by_type TEXT,
    superseded_by_id TEXT,
    redacted_at TIMESTAMPTZ,
    redacted_by_type TEXT,
    redacted_by_id TEXT,
    redaction_reason TEXT,

    PRIMARY KEY (note_id),
    -- Candidate key for the tenant-aware self-reference below.
    UNIQUE (tenant_id, note_id),
    FOREIGN KEY (tenant_id, incident_id)
        REFERENCES incidents (tenant_id, incident_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, supersedes_note_id)
        REFERENCES incident_notes (tenant_id, note_id),

    CHECK (char_length(body) <= 16000),
    CHECK (visibility IN ('internal', 'customer_visible')),
    CHECK (created_by_type IN ('operator', 'service_account', 'system')),
    CHECK ((created_by_type = 'system') = (created_by_id IS NULL)),
    CHECK (superseded_by_type IS NULL OR superseded_by_type IN ('operator', 'service_account', 'system')),
    CHECK (redacted_by_type IS NULL OR redacted_by_type IN ('operator', 'service_account', 'system')),
    CHECK (note_index >= 0)
);

CREATE INDEX incident_notes_by_incident
    ON incident_notes (incident_id, created_at DESC);

-- Tags: set semantics, keyed by (incident, key) -- one value per key per
-- incident, mirroring `Incident.tags: BTreeMap<String, String>`
-- (verified against crates/incident/src/incident.rs). Normalized into its
-- own table like `incident_notes` and `incident_policy_references`
-- rather than a denormalized JSONB column on `incidents`, so a 5B-3
-- repository reconstructs the map by the same join-then-assemble pattern
-- it already uses for notes and policy references. This supersedes
-- incident-persistence.md's stale "GIN on `tags`" supporting-index note
-- (which assumed a JSONB column on `incidents` predating this table's
-- 2026-08-30 addition to the same document) -- replaced below with an
-- index shaped for this normalized table instead.
CREATE TABLE incident_tags (
    incident_id UUID NOT NULL,
    tenant_id TEXT NOT NULL,
    tag_key TEXT NOT NULL,
    tag_value TEXT NOT NULL,

    PRIMARY KEY (incident_id, tag_key),
    FOREIGN KEY (tenant_id, incident_id)
        REFERENCES incidents (tenant_id, incident_id) ON DELETE CASCADE,

    CHECK (char_length(tag_key) <= 64),
    CHECK (char_length(tag_value) <= 256)
);

CREATE INDEX incident_tags_lookup
    ON incident_tags (tenant_id, tag_key, tag_value);

-- Assignment history: append-only, so "who owned this at 02:00?" is
-- answerable. The current assignment on `incidents` itself
-- (`assigned_kind`/`assigned_id`) is a denormalised convenience, not the
-- record.
CREATE TABLE incident_assignments (
    assignment_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    incident_id UUID NOT NULL,
    tenant_id TEXT NOT NULL,
    assigned_kind TEXT,
    assigned_id TEXT,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    changed_by_type TEXT NOT NULL,
    changed_by_id TEXT,

    FOREIGN KEY (tenant_id, incident_id)
        REFERENCES incidents (tenant_id, incident_id) ON DELETE CASCADE,

    CHECK (assigned_kind IS NULL OR assigned_kind IN ('user', 'team')),
    CHECK ((assigned_kind IS NULL) = (assigned_id IS NULL)),
    CHECK (changed_by_type IN ('operator', 'service_account', 'system')),
    CHECK ((changed_by_type = 'system') = (changed_by_id IS NULL))
);

CREATE INDEX incident_assignments_by_incident
    ON incident_assignments (incident_id, changed_at DESC);
