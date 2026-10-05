-- 5D-4: keyset pagination for GET /api/v1/incidents.
--
-- The list is ordered by opened_at or last_detected_at, newest first by
-- default, with incident_id as the tie-breaker so every position is
-- unique. Each index serves the tenant-scoped scan for its sort in either
-- direction, with or without a state filter, so a page costs a bounded
-- index range rather than a sort of the tenant's whole history
-- (incident-api-plan.md: "every filter combination must be
-- index-supported"). The V2 index (tenant_id, state, opened_at) remains
-- for state-led lookups.
CREATE INDEX incidents_tenant_opened_keyset
    ON incidents (tenant_id, opened_at DESC, incident_id DESC);

CREATE INDEX incidents_tenant_last_detected_keyset
    ON incidents (tenant_id, last_detected_at DESC, incident_id DESC);
