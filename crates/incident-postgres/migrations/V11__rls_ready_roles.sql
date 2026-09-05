-- ADR 0032 (tenant isolation and RLS readiness), defense-in-depth layers
-- 4 and 5: a separate migration/admin role distinct from the
-- application's runtime role, and an application runtime role explicitly
-- provisioned WITHOUT `BYPASSRLS` -- even though no RLS policy exists yet
-- to bypass, so activating RLS later (Phase 8) cannot silently no-op
-- because the connecting role was exempt from it all along.
--
-- This migration does NOT activate Row-Level Security. ADR 0032 is
-- explicit that RLS activation itself, with real per-tenant
-- identity-aware policies, is Phase 8 scope -- this file only provisions
-- the role Phase 8's policies will eventually apply to, and grants it
-- exactly the privileges the application needs today.
--
-- No password is set here, and none should ever be committed to this
-- repository -- see this project's standing credentials rule
-- (R7 in docs/risk-register.md). An operator sets one out-of-band
-- (`ALTER ROLE wetechinetmon_app WITH PASSWORD '...'`, sourced from a
-- vault or the environment) as part of deployment, documented in
-- crates/incident-postgres/README.md.
--
-- Idempotent: `CREATE ROLE` is not itself idempotent (no `IF NOT EXISTS`
-- clause in PostgreSQL), so the existence check below is required for
-- this migration to be safely re-runnable in principle -- refinery's own
-- checksum guard already prevents a genuine re-application of an applied
-- migration, but a fresh database that happens to share a cluster with a
-- previous one (the same Postgres instance hosting more than one
-- migrated database, as a throwaway test cluster commonly does) must not
-- fail on a role that already exists cluster-wide.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT FROM pg_catalog.pg_roles WHERE rolname = 'wetechinetmon_app'
    ) THEN
        CREATE ROLE wetechinetmon_app LOGIN NOBYPASSRLS NOSUPERUSER NOCREATEDB NOCREATEROLE;
    END IF;
END
$$;

GRANT USAGE ON SCHEMA public TO wetechinetmon_app;

-- Ordinary read/write tables.
GRANT SELECT, INSERT, UPDATE, DELETE ON
    incidents,
    incident_detection_events,
    incident_notes,
    incident_tags,
    incident_assignments,
    incident_policy_references,
    incident_number_allocators,
    incident_idempotency,
    incident_outbox,
    incident_dead_letter
TO wetechinetmon_app;

-- Timeline and audit are append-only: SELECT and INSERT only, never
-- UPDATE or DELETE, so "no UPDATE, no DELETE" is enforced by what this
-- role can do rather than by convention alone
-- (incident-persistence.md's `incident_timeline` section).
GRANT SELECT, INSERT ON incident_timeline, incident_audit TO wetechinetmon_app;

-- Every `BIGINT GENERATED ALWAYS AS IDENTITY` column above is backed by
-- an implicit sequence; the application role needs USAGE on it to
-- INSERT at all.
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO wetechinetmon_app;
