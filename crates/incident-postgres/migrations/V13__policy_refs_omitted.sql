-- FU-34: counts the linked events whose policy was not recorded in
-- `incident_policy_references` because the incident already held the
-- domain's per-incident cap (`POLICY_REFS_MAX`, 64). The evidence ledger
-- counts what it stops retaining (`evidence_summary.observed_total`); this
-- does the same for policy references, so a 65th distinct policy is never
-- dropped silently. The omitted policy's identity stays on its
-- `incident_detection_events` row.
--
-- V7 added no omitted-count column on the assumption that a normalized,
-- uncapped table leaves nothing to omit. The domain still caps
-- `policy_refs` before a row is written, so the count is needed after all.
ALTER TABLE incidents
    ADD COLUMN policy_refs_omitted BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT incidents_policy_refs_omitted_non_negative CHECK (policy_refs_omitted >= 0);
