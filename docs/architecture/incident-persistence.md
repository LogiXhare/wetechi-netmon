# Incident Persistence

Status: **Planning only.** Part of the
[Phase 5 plan](phase5-incident-management-plan.md). Decisions in
[ADR 0015](decisions/0015-incident-operational-storage.md) and
[ADR 0012](decisions/0012-incident-event-ingestion.md).

**No migration is written by this document.** The DDL sketches below are
design artefacts for review, not files in a migrations directory.

## Two stores, one source of truth

| Store | Holds | Authority |
|---|---|---|
| **PostgreSQL** | Incidents, timeline, notes, audit, idempotency, outbox | **Source of truth** for all operational state |
| **ClickHouse** | Immutable incident analytics events | Derived; never authoritative |

ClickHouse already holds `wetechinetmon_detection_events` with a 365-day
TTL and is excellent at what Phase 3 and 4 use it for: high-volume
immutable appends scanned analytically. It is a poor fit for incident
*state*, which needs row-level updates, transactions across several
tables, unique constraints, and foreign keys. Using it for both would
mean either dual-authoritative mutable state — the failure mode where two
stores disagree and nobody can say which is right — or reimplementing
transactions in application code.

So: PostgreSQL owns mutable operational state. ClickHouse receives
immutable analytics events through the outbox. There is exactly one
authority for any given fact.

## Tables

Sketches. Types and constraints are the design; exact DDL is Milestone 5B.

### `incidents`

Columns follow the [domain model](incident-domain-model.md). Key
constraints:

```text
PRIMARY KEY (incident_id)
UNIQUE (tenant_id, incident_number)
CHECK (state IN ('open','acknowledged','investigating','monitoring',
                 'recovering','resolved','closed'))
CHECK (suppressed_until IS NULL OR suppression_reason IS NOT NULL)
CHECK (severity IN ('info','minor','major','critical'))
CHECK (address_family IN (4,6))
CHECK (closed_at IS NOT NULL OR state <> 'closed')
```

The constraint that carries the most weight:

```sql
CREATE UNIQUE INDEX incidents_one_open_per_key
  ON incidents (tenant_id, correlation_key)
  WHERE state <> 'closed';
```

A **partial unique index** makes "two open incidents for one correlation
key" impossible at the database level. Without it, two concurrent
correlator instances — or one instance processing a redelivered event
during a retry — could each check "is there an open incident?", both see
none, and both insert. Enforcing it in application code means enforcing
it in a race. Enforcing it here means the second insert fails loudly and
the correlator retries into the update path.

Suppression is three columns — `suppressed_until`, `suppression_reason`,
`suppressed_by` — and **not** a state, per
[ADR 0014](decisions/0014-incident-state-machine.md). The `CHECK` above
makes a suppression without a reason unrepresentable; the mandatory
expiry is enforced at the command boundary because it is a policy bound
rather than a data invariant.

Supporting indexes: `(tenant_id, state, opened_at DESC)` for the default
queue view; `(tenant_id, suppressed_until) WHERE suppressed_until IS NOT
NULL` so an alerting consumer can find live suppressions cheaply; `(tenant_id, assigned_user_id) WHERE state <> 'closed'`;
`(tenant_id, target_id)`; `(tenant_id, closed_at DESC)` for reopen-window
lookups; GIN on `tags`.

### `incident_detection_events`

The link table. One row per detection event associated with an incident.

```text
PRIMARY KEY (incident_id, detection_event_id)
UNIQUE (tenant_id, dedup_key)      -- the duplicate gate
INDEX (incident_id, observed_at DESC)
```

The `UNIQUE (tenant_id, dedup_key)` constraint is how at-least-once
delivery becomes effectively-once *processing*. Phase 4's `dedup_key` is
`{detection_id}:{kind}:{sequence}` with a gapless sequence, so redelivery
is exactly detectable. The correlator does not need to ask "have I seen
this?" — it inserts, and a unique violation *is* the answer.

Columns store the correlation-relevant fields, not the whole event: the
authoritative copy stays in ClickHouse. Stored here: `detection_event_id`,
`dedup_key`, `detection_id`, `policy_id`, `policy_version`, `kind`,
`severity`, `observed_at`, `detected_at`, `matched` (JSONB), `rates`
(JSONB), `link_type` (`opening`, `update`, `closing`, `late`, `evidence`).

### `incident_timeline`

Append-only. No `UPDATE`, no `DELETE` — enforced by a revoked grant, not
just convention, so an application bug cannot rewrite history.

```text
PRIMARY KEY (timeline_id)
INDEX (incident_id, occurred_at, timeline_id)
```

Fields: `timeline_id`, `incident_id`, `tenant_id`, `occurred_at`,
`entry_type`, `actor_type` (`operator`, `system`, `service_account`),
`actor_id`, `correlation_id`, `command_id`, `source_event_id`,
`previous_value` (JSONB), `new_value` (JSONB), `payload` (JSONB),
`schema_version`.

Entry types: `opened`, `event_linked`, `state_changed`,
`severity_changed`, `priority_changed`, `assignment_changed`,
`note_added`, `note_superseded`, `evidence_added`, `recovery_detected`,
`recovery_aborted`, `resolved`, `closed`, `reopened`, `suppressed`,
`unsuppressed`, `duplicate_ignored`, `late_event_linked`,
`correlation_decision`, `category_changed`, `limit_reached`,
`persistence_retry`, plus reserved `notification_result` and
`mitigation_result` that nothing writes in Phase 5.

### `incident_notes`

Notes are **immutable with supersession**, not editable in place. An
edited note destroys the record of what the operator originally believed,
which during a post-mortem is often the interesting part.

```text
PRIMARY KEY (note_id)
INDEX (incident_id, created_at DESC)
FOREIGN KEY (supersedes_note_id) REFERENCES incident_notes(note_id)
```

Editing writes a new row with `supersedes_note_id` set; the superseded
row gets `superseded_at` and `superseded_by`. The API returns only
current notes by default and the full chain on request. Deleting is
soft — `redacted_at`, `redacted_by`, `redaction_reason`, body replaced —
because a note may contain something that genuinely must be removed, and
that removal must itself be auditable.

`visibility` is `internal` or `customer_visible`. **Phase 5 stores the
field and refuses to set `customer_visible`**, returning `501`, because
there is no customer-facing surface and no tenant authorization model to
decide who may publish to one. Storing the column now avoids a migration
later; honouring it now would be shipping an unreviewed disclosure path.

### `incident_assignments`, `incident_tags`

Assignment history is append-only, so "who owned this at 02:00?" is
answerable. The current assignment on the incident row is a denormalised
convenience, not the record.

### `incident_audit`

Separate from the timeline. Records authorization decisions including
**denials**, which have no incident to attach to and therefore cannot
live on a timeline. Fields: `audit_id`, `occurred_at`, `tenant_id`,
`actor_type`, `actor_id`, `action`, `resource_type`, `resource_id`,
`result` (`allowed`, `denied`, `error`), `reason`, `source_ip`,
`user_agent`, `request_id`, `trace_id`, `before` (JSONB), `after`
(JSONB), `schema_version`.

### Auditing a command that was denied

A denied command has no incident mutation, so it has no transaction to
ride along with — and it is exactly the record a security review needs.
Audit therefore has **two** write paths, and only one of them is the
mutation transaction:

| Path | When | Transaction |
|---|---|---|
| **In-transaction** | The command was authorized and mutated an incident | The same transaction as state, timeline, and outbox. If the audit write fails, the mutation rolls back |
| **Standalone** | Authorization denied, tenant mismatch, validation rejected, or the resource does not exist | Its own short transaction, committed independently, **before** the response is returned |

The standalone path must not be able to fail silently. If it cannot
commit, the request returns `503` rather than proceeding unaudited: a
denial that is not recorded is indistinguishable from one that never
happened, which is the whole value of the record.

The standalone path records what it can — actor, action, attempted
resource id, the tenant *of the caller*, result, and reason — and
deliberately **not** whether the target resource actually exists. Storing
that would rebuild, inside the audit log, the existence oracle the
404-not-403 rule exists to prevent.

Append-only, and `source_ip` and `user_agent` are recorded only where
they are trustworthy — behind a proxy that sets them, with the proxy
trusted. A forged `X-Forwarded-For` written into an audit log as fact is
worse than no field at all.

Tamper evidence — hash chaining each row to its predecessor — is
**deferred**, not dismissed. It is cheap to add later if the column is
reserved now, and the threat it addresses is an attacker who already has
database write access. Recorded as **FU-19**.

### `incident_idempotency`

```text
PRIMARY KEY (tenant_id, idempotency_key)
INDEX (expires_at)
```

Fields: `operation`, `resource_type`, `resource_id`,
`request_fingerprint` (hash of the canonicalised body),
`response_status`, `response_body_ref`, `created_at`, `expires_at`.
Retention 24 h by default. See
[ADR 0016](decisions/0016-incident-concurrency-and-idempotency.md).

### `incident_outbox`

```text
PRIMARY KEY (outbox_id)
INDEX (status, available_at) WHERE status IN ('pending','retrying')
```

Fields: `outbox_id`, `tenant_id`, `aggregate_type`, `aggregate_id`,
`event_type`, `payload` (JSONB), `status`, `attempts`, `available_at`,
`last_error`, `created_at`, `published_at`.

Consumers: the ClickHouse analytics exporter in Phase 5; notification in
Phase 6; mitigation in Phase 7. Phase 5 writes event types those later
phases will consume and publishes only the analytics ones. **Producing an
event nobody consumes is not implementing a notification** — it is
leaving the seam ADR 0011 requires.

### `incident_dead_letter`

Events that failed repeatedly or could not be parsed. Holds the raw
payload, the failure reason, attempt count, and first/last seen. Never
auto-purged while unreviewed — a dead-letter row that ages out silently
is an incident nobody knows was missed.

## Transaction boundaries

The central rule:

> **A command is acknowledged only if the state change, its timeline
> entry, its audit record, and its outbox row all committed. One
> transaction, or none of it.**

Opening an incident, in one transaction:

1. Allocate `incident_number` from the per-tenant sequence.
2. `INSERT` the incident.
3. `INSERT` the link row for the opening event — unique violation here
   means a duplicate, so roll back and treat it as one.
4. `INSERT` timeline `opened` and `event_linked`.
5. `INSERT` audit.
6. `INSERT` outbox `incident.opened`.
7. `INSERT` the idempotency record.
8. Commit.

Linking an event to an existing incident, and every operator command,
follow the same shape: read with the expected version, mutate, append
timeline, append audit, enqueue outbox, record idempotency, commit.

ClickHouse export is **outside** the transaction, by design. It reads
committed outbox rows and publishes them. A ClickHouse outage therefore
delays analytics and never blocks or corrupts operational state — the
outbox is exactly the mechanism that buys that separation.

## Failure behaviour

| Failure | Behaviour |
|---|---|
| PostgreSQL unavailable | Reject commands with `503`; ingestion stops consuming and does not acknowledge; no partial state |
| ClickHouse unavailable | Outbox backs up; operational state unaffected; `outbox_pending` rises and is alerted |
| Queue full | Backpressure to ingestion; `503` on the API; never drop silently |
| Malformed event | Quarantine to dead-letter, increment `events_rejected_total` |
| Unsupported schema version | Quarantine; **never** guess at unknown fields |
| Duplicate event | Unique violation, counted, not an error |
| Late or out-of-order event | Linked as evidence, no state change |
| Version conflict | `409` with current version; never a silent overwrite |
| Audit write fails | **Whole transaction rolls back.** An unauditable mutation must not happen |
| Outbox publish fails | Retry with backoff; then dead-letter; operational state untouched |
| Service restart | Uncommitted work is lost by construction; the outbox is re-read from `pending` |
| Disk full | PostgreSQL rejects writes; commands fail closed |
| Clock skew | Wall-clock only for display; ordering uses database sequence, not client time |

Retries use exponential backoff with jitter, capped attempts, then
dead-letter. No infinite retry loop: a poison event that can never
succeed must stop consuming resources and become visible instead.

## Retention

| Data | Default | Notes |
|---|---|---|
| Open incidents | Indefinite | |
| Closed incidents | 24 months | Owner decision; not a legal claim |
| Timeline | Follows its incident | |
| Notes | Follows its incident | |
| Audit | 24 months minimum | Longer than incidents deliberately |
| Idempotency | 24 h | |
| Outbox published | 7 days | Then purged |
| Dead-letter | 90 days, **never while unreviewed** | |
| ClickHouse analytics | 365 days | Matches detection events |

**No legal or regulatory retention requirement is asserted here.** These
are engineering defaults. Anything contractual or regulatory needs formal
review — recorded as **FU-20**.

Tenant deletion must never cascade into audit. The design is: purge or
anonymise incident content, retain audit rows with the tenant reference
intact, and require an explicit, separately authorized, separately
audited operation to remove audit history. Accidental cascade from a
`DELETE FROM tenants` is exactly the failure this prevents.

## Tenant isolation

`tenant_id` is present on **every** table, including timeline, notes,
audit, idempotency, and outbox — not only on `incidents`. Isolation must
not depend on a join being written correctly every time.

Every query carries a tenant predicate, and repository interfaces should
make a tenant-less query **impossible to express** rather than merely
discouraged: the tenant context is a constructor argument, not a
parameter a caller can forget.

PostgreSQL Row-Level Security is the right long-term enforcement and is
**evaluated, not claimed**: it belongs with the Phase 8 tenancy work,
because it needs a per-tenant database role model that does not exist
yet. Phase 5 designs the schema so RLS can be switched on without a
migration — every table has `tenant_id`, and no table relies on a join to
establish tenancy. Recorded as **FU-21**.
