-- The `incidents` table: current, mutable, versioned state for one
-- incident. See docs/architecture/incident-persistence.md's "Tables"
-- section and docs/architecture/incident-domain-model.md for the field
-- inventory this migration implements.
--
-- Design notes (read before extending this table):
--
-- 1. Column set scope. This table carries exactly the columns backed by
--    either (a) `wetechinetmon_incident::snapshot::IncidentSnapshot`'s
--    actual fields (verified against crates/incident/src/snapshot.rs,
--    2026-09-05) or (b) an explicit, dated requirement in
--    incident-persistence.md or an ADR. It deliberately does NOT include
--    several columns docs/architecture/incident-domain-model.md's older,
--    broader planning table lists (`current_metrics`, `peak_metrics`,
--    `baseline_metrics`, `opening_reason`, `detection_event_count`,
--    `mitigation_status`, `notification_status`, `customer_id`,
--    `site_id`, `datacenter_id`, `target_display`) because none of them
--    exist on `Incident`/`IncidentSnapshot` today and
--    incident-persistence.md's own "Tables" section never lists them
--    either -- only the older domain-model doc does. Adding speculative
--    columns nothing produces or reads is schema surface this milestone
--    cannot verify; a future milestone that actually needs one of these
--    adds it in its own reviewable migration. Flagged for owner review,
--    not silently decided either way.
--
-- 2. `maximum_detected_severity` IS included despite having no
--    corresponding `IncidentSnapshot` field today, because -- unlike the
--    columns excluded above -- incident-persistence.md explicitly and
--    recently (2026-08-24) requires it by name, and follow-ups.md FU-41's
--    "Phase 5B disposition" explicitly assigns persisting it (not
--    populating it with real escalation logic) to this milestone. 5B-3
--    must set it at row-creation time (e.g. to the opening `severity`)
--    until a future Phase 5C change adds the corresponding domain field;
--    that wiring is explicitly out of scope here.
--
-- 3. `target_type` is 3-valued (`host`/`network`/`hostgroup`), matching
--    the detector's `ScopeId` enum (`Host`/`Network`/`Hostgroup`,
--    crates/detector/src/input.rs) that
--    `IncidentSnapshot::target_identity` actually carries -- not the
--    detector's separate, 4-valued `ScopeType` enum
--    (`Host`/`Prefix`/`Slash24`/`HostgroupTotal`) that
--    `IncidentSnapshot::target_type` is typed as. incident-persistence.md
--    itself specifies the 3-valued CHECK reproduced below. How a policy
--    scope's `Prefix`/`Slash24` (`ScopeType`) maps onto this table's
--    `network` target (`ScopeId::Network`) is a 5B-3 application-mapping
--    question, not a schema question -- flagged here so it is not
--    mistaken for an oversight.
--
-- 4. `direction` allows the detector's full 5-value `TrafficDirection`
--    (`incoming`/`outgoing`/`internal`/`other`/`unknown`, verified
--    against crates/detector/src/input.rs), not
--    incident-domain-model.md's older 3-value list
--    (`incoming`/`outgoing`/`internal`) -- the domain's actual
--    `CorrelationKey.direction` field can carry all five.
--
-- 5. Actor fields (`created_by`, `updated_by`, `suppressed_by`) are each
--    split into a `_type` (`operator`/`service_account`/`system`,
--    matching `wetechinetmon_incident::authorization::Actor`) and a
--    nullable `_id` column, rather than one opaque string, so `system`
--    (which carries no id) is structurally distinguishable from an actor
--    whose id happens to be empty.
CREATE TABLE incidents (
    incident_id UUID NOT NULL,
    incident_number TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    tenant_id TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    address_family SMALLINT NOT NULL,
    direction TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_addr INET,
    target_network CIDR,
    target_hostgroup TEXT,
    created_by_type TEXT NOT NULL,
    created_by_id TEXT,

    title TEXT NOT NULL,
    description TEXT,

    state TEXT NOT NULL,
    severity TEXT NOT NULL,
    severity_source TEXT NOT NULL,
    ever_critical BOOLEAN NOT NULL DEFAULT FALSE,
    maximum_detected_severity TEXT NOT NULL,
    priority TEXT NOT NULL,
    closure_reason TEXT,
    state_before_recovering TEXT,
    suppressed_until TIMESTAMPTZ,
    suppression_reason TEXT,
    suppressed_by_type TEXT,
    suppressed_by_id TEXT,
    version BIGINT NOT NULL DEFAULT 1,

    category TEXT NOT NULL,
    matched_metrics JSONB NOT NULL DEFAULT '[]'::jsonb,

    first_detected_at TIMESTAMPTZ NOT NULL,
    opened_at TIMESTAMPTZ NOT NULL,
    last_detected_at TIMESTAMPTZ NOT NULL,
    last_updated_at TIMESTAMPTZ NOT NULL,
    acknowledged_at TIMESTAMPTZ,
    recovering_since TIMESTAMPTZ,
    resolved_at TIMESTAMPTZ,
    closed_at TIMESTAMPTZ,
    reopened_at TIMESTAMPTZ,
    reopen_count INTEGER NOT NULL DEFAULT 0,

    assigned_kind TEXT,
    assigned_id TEXT,
    updated_by_type TEXT NOT NULL,
    updated_by_id TEXT,

    evidence_summary JSONB NOT NULL DEFAULT '{"retained":[],"observed_total":0}'::jsonb,

    PRIMARY KEY (incident_id),

    -- Candidate key every tenant-owned child table's foreign key
    -- references (ADR 0032's defense-in-depth layer 2; see
    -- incident-persistence.md's "Tenant-aware composite foreign keys").
    -- The primary key itself stays `incident_id` alone.
    UNIQUE (tenant_id, incident_id),
    UNIQUE (tenant_id, incident_number),

    CHECK (char_length(title) BETWEEN 1 AND 200),
    CHECK (description IS NULL OR char_length(description) <= 8000),
    CHECK (state IN (
        'open', 'acknowledged', 'investigating', 'monitoring',
        'recovering', 'resolved', 'closed'
    )),
    CHECK (state_before_recovering IS NULL OR state_before_recovering IN (
        'open', 'acknowledged', 'investigating', 'monitoring',
        'recovering', 'resolved', 'closed'
    )),
    CHECK (suppressed_until IS NULL OR suppression_reason IS NOT NULL),
    CHECK (suppression_reason IS NULL OR char_length(suppression_reason) <= 500),
    CHECK (severity IN ('info', 'minor', 'major', 'critical')),
    CHECK (severity_source IN ('detection', 'operator')),
    CHECK (maximum_detected_severity IN ('info', 'minor', 'major', 'critical')),
    CHECK (address_family IN (4, 6)),
    CHECK (closed_at IS NOT NULL OR state <> 'closed'),
    CHECK (priority IN ('P1', 'P2', 'P3', 'P4')),
    CHECK (closure_reason IS NULL OR closure_reason IN (
        'resolved', 'false_positive', 'duplicate',
        'expected_traffic', 'no_action_required', 'other'
    )),
    CHECK (category IN (
        'tcp_syn_flood', 'fragmentation_flood', 'icmp_flood', 'udp_flood',
        'tcp_flood', 'packet_rate', 'bandwidth', 'drop_pressure',
        'multi_vector', 'unclassified'
    )),
    CHECK (direction IN ('incoming', 'outgoing', 'internal', 'other', 'unknown')),
    CHECK (
        (target_type = 'host' AND target_addr IS NOT NULL AND target_network IS NULL AND target_hostgroup IS NULL) OR
        (target_type = 'network' AND target_network IS NOT NULL AND target_addr IS NULL AND target_hostgroup IS NULL) OR
        (target_type = 'hostgroup' AND target_hostgroup IS NOT NULL AND target_addr IS NULL AND target_network IS NULL)
    ),
    CHECK (created_by_type IN ('operator', 'service_account', 'system')),
    CHECK ((created_by_type = 'system') = (created_by_id IS NULL)),
    CHECK (updated_by_type IN ('operator', 'service_account', 'system')),
    CHECK ((updated_by_type = 'system') = (updated_by_id IS NULL)),
    CHECK (suppressed_by_type IS NULL OR suppressed_by_type IN ('operator', 'service_account', 'system')),
    CHECK (suppressed_by_type IS NULL OR (suppressed_by_type = 'system') = (suppressed_by_id IS NULL)),
    CHECK (assigned_kind IS NULL OR assigned_kind IN ('user', 'team')),
    CHECK ((assigned_kind IS NULL) = (assigned_id IS NULL)),
    CHECK (reopen_count >= 0),
    CHECK (version >= 1),
    CHECK ((reopen_count > 0) = (reopened_at IS NOT NULL))
);

-- Supporting indexes (incident-persistence.md's "Active-incident
-- invariant" section). The active-incident partial unique indexes
-- themselves are a separate, later migration
-- (V10__active_incident_partial_indexes.sql) so that security/
-- correctness-critical invariant gets its own focused review.
CREATE INDEX incidents_tenant_state_opened_at
    ON incidents (tenant_id, state, opened_at DESC);

CREATE INDEX incidents_tenant_suppressed
    ON incidents (tenant_id, suppressed_until)
    WHERE suppressed_until IS NOT NULL;

CREATE INDEX incidents_tenant_assignee
    ON incidents (tenant_id, assigned_kind, assigned_id)
    WHERE state <> 'closed';

CREATE INDEX incidents_tenant_closed_at
    ON incidents (tenant_id, closed_at DESC)
    WHERE closed_at IS NOT NULL;

CREATE INDEX incidents_tenant_target_addr
    ON incidents (tenant_id, target_addr)
    WHERE target_addr IS NOT NULL;

CREATE INDEX incidents_tenant_target_network
    ON incidents (tenant_id, target_network)
    WHERE target_network IS NOT NULL;

CREATE INDEX incidents_tenant_target_hostgroup
    ON incidents (tenant_id, target_hostgroup)
    WHERE target_hostgroup IS NOT NULL;
