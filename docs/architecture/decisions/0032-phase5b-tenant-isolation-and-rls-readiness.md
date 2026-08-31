# 0032. Phase 5B Tenant Isolation and Row-Level Security Readiness

Status: **Accepted**
Date: 2026-08-24
Deciders: Repository owner

## Context

[incident-persistence.md](../incident-persistence.md) already states
PostgreSQL RLS "belongs with the Phase 8 tenancy work, because it needs a
per-tenant database role model that does not exist yet" and that Phase 5
"designs the schema so RLS can be switched on without a migration" —
recorded as **FU-21**. **R16** in
[risk-register.md](../../risk-register.md) already tracks tenant
isolation as application-code-enforced until Phase 8. This ADR formalizes
that posture as a decision record rather than leaving it as a follow-up
note, and fixes the defense-in-depth layers Phase 5B actually builds.

## Options Considered

### Option A — RLS-ready schema now, identity-aware activation in Phase 8, defense-in-depth in application code and role model in the meantime

- Pros: matches the already-approved posture (FU-21) exactly, so this
  ADR is a formalization, not a new decision; does not require solving
  Phase 8's per-tenant identity/role model to ship Phase 5B; every table
  already has `tenant_id` per [incident-persistence.md](../incident-persistence.md)'s
  existing design, so RLS activation later is additive, not a migration.
- Cons: tenant isolation is enforced by application discipline (the
  tenant-context-as-constructor-argument pattern) plus schema-level
  defenses (composite foreign keys, no table without `tenant_id`), not
  by the database refusing a mis-scoped query outright — a single
  missing tenant predicate in a hand-written query is still possible
  until RLS actually activates.

### Option B — Activate RLS now, with a single shared application role

- Pros: database-enforced isolation immediately.
- Cons: **explicitly rejected by this task's instruction** ("do not
  claim RLS protection when using a role that bypasses it"). A single
  shared application role connecting as itself provides no RLS
  protection unless PostgreSQL's session-level tenant context is set
  per-request and the policy references it correctly — which requires
  exactly the per-tenant role/session model Phase 8 owns. Activating RLS
  without that model would be **claiming** a protection the
  implementation does not actually provide, which is precisely what this
  task instructs against.

### Option C — No tenant_id-based design at all; defer everything to Phase 8

- Pros: less schema work now.
- Cons: contradicts the entire approved persistence plan, which already
  requires `tenant_id` "not only on `incidents`" but on timeline, notes,
  audit, idempotency, and outbox. Not a real option — already decided
  against in prior planning.

## Decision

**Option A**, with the defense-in-depth layers Phase 5B actually
delivers, listed explicitly so "RLS-ready" is not read as "RLS-equivalent":

1. `tenant_id` on **every** tenant-owned table (already the approved
   design).
2. **Composite tenant-aware foreign keys** — a foreign key referencing
   another tenant-owned table includes `tenant_id` in the key, so a
   cross-tenant foreign-key reference is structurally rejected by the
   database, not only by application logic. **Made concrete 2026-08-30**
   in [incident-persistence.md](../incident-persistence.md)'s "Tenant-aware
   composite foreign keys" section: `incidents` gains
   `UNIQUE (tenant_id, incident_id)` as the candidate key every
   tenant-owned child table's foreign key references, and every table
   that references a specific incident does so through the composite
   pair rather than `incident_id` alone. Not every table gets this
   foreign key — a table using a polymorphic reference
   (`incident_audit`, `incident_idempotency`, `incident_outbox`,
   `incident_dead_letter`) cannot target one fixed parent table by
   design, and that document states why for each.
3. **Application queries always tenant-scoped** — the existing 5A
   pattern (tenant context as a repository constructor argument, making
   a tenant-less query inexpressible) is the Phase 5B-0 seam's
   requirement too, not weakened by the PostgreSQL implementation.
4. **A separate migration/admin role** distinct from the application's
   runtime role — schema changes and RLS policy definition happen under
   a role the application itself never connects as.
5. **The application's runtime role does not carry `BYPASSRLS`** — even
   before RLS policies are activated, the role is provisioned without
   that attribute, so activating RLS later (Phase 8) does not silently
   no-op because the connecting role was exempt from it all along.
6. **Connection-pool tenant-context reset** — Phase 5B's pool
   configuration ([ADR 0022](0022-phase5b-connection-pool.md)) must
   ensure no session-local state (were RLS's `SET app.tenant_id` context
   to be introduced early, which this ADR does not require but does not
   preclude) can leak from one pooled connection's prior tenant to the
   next borrower.
7. **The platform-admin path is isolated** — any cross-tenant
   administrative operation is a separate, explicitly authorized code
   path, never an accidental consequence of an unscoped query.
8. **Backup and restore tenant considerations** are noted in the backup
   plan; a tenant-scoped export/restore is not designed in Phase 5B.

**RLS activation itself, with actual per-tenant identity-aware
policies and roles, is Phase 8 scope**, unchanged from the existing FU-21
posture. **This ADR does not claim RLS protection exists in Phase 5B.**

## Consequences

**Easier.** Phase 8 can activate RLS as an additive policy layer without
a schema migration, and without discovering a table that was never given
`tenant_id`.

**Harder.** Isolation correctness in Phase 5B still depends on every
query actually being tenant-scoped — the composite foreign keys and
constructor-argument pattern reduce but do not eliminate the risk a
single hand-written query gets this wrong, which is exactly R16's
existing framing.

**Forecloses.** Nothing — this ADR is additive preparation, not a
different schema shape than already planned.

**Security.** This is R16's disposition, formalized. The cross-tenant
integration-test suite (already required by
[incident-security-model.md](../incident-security-model.md) T-05) is the
actual enforcement mechanism until RLS activates, and must run against
every Phase 5B repository method, not only the ones ADR 0029's
first-pass trait happens to cover.

**License.** N/A.

## Follow-Up

- [x] Add the composite tenant-aware foreign key to every relevant table
      in the Phase 5B-2 schema — **designed 2026-08-30** in
      [incident-persistence.md](../incident-persistence.md); still to be
      implemented as actual DDL at Phase 5B-2.
- [ ] Provision the application runtime role without `BYPASSRLS` at
      Phase 5B-2, even though no policy exists yet to bypass.
- [ ] Extend the cross-tenant isolation integration-test suite to cover
      every Phase 5B repository method.
- [ ] Confirm Phase 8's eventual RLS activation plan references this
      ADR rather than starting from an unscoped schema question.
