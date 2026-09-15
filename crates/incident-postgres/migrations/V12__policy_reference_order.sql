-- Records each policy reference's position in `Incident::policy_refs`.
--
-- The domain keeps policy references in first-seen order: the opening
-- policy first, then each later policy in the order its first event
-- arrived (crates/incident/src/unit_of_work.rs). V7 stores the references
-- but nothing that carries that order, and no existing column reproduces
-- it (`first_seen_sequence` is per detection, so two detections can share
-- a value). Without this column a load could not rebuild the vector the
-- incident was flushed with. 5B-3(b) found this while writing the row
-- mapping.
--
-- A new migration rather than an edit to V7: applied migrations are
-- checksummed and must never change (ADR 0024).
--
-- The mapping writes 0..n-1 for an incident's n references and refuses to
-- load a gap or a duplicate, so no unique index is added here.

ALTER TABLE incident_policy_references
    ADD COLUMN ref_index INTEGER NOT NULL DEFAULT 0,
    ADD CONSTRAINT incident_policy_references_ref_index_non_negative
        CHECK (ref_index >= 0);
