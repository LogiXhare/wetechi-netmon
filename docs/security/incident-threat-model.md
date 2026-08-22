# Incident Management Threat Model

Status: **Planning only.** Part of the
[Phase 5 plan](../architecture/phase5-incident-management-plan.md).
Nothing here is implemented; every control is a requirement on the future
implementation, and every one has a corresponding entry in the
[testing plan](../architecture/incident-testing-plan.md).

Twenty-four threats. Each names the asset, the attacker, the path, a
preventive control, a detective control, the test that must exist, and
the residual risk that remains after both controls.

Attacker classes used throughout:

- **EXT** — unauthenticated external
- **TEN** — authenticated user of *another* tenant
- **USR** — authenticated low-privilege user of *this* tenant
- **SVC** — holder of a compromised service credential
- **INS** — insider with database or host access

---

## T-01 Forged detection event

**Asset** incident integrity · **Attacker** EXT, SVC

**Path** An attacker who can write to the outbox or impersonate the
ingestion service injects events, creating incidents that mask a real
attack or exhaust operators with noise.

**Prevent** Ingestion is not an HTTP endpoint — events arrive through the
outbox inside the trust boundary. The ingestion credential holds only
`incident.ingest` and cannot acknowledge, resolve, or close. Database
credentials come from the environment, never source. **Detect**
`events_ingested_total` against detector-side event counts; a divergence
means events from somewhere else. **Test** ingestion credential is
refused for every operator command. **Residual** An attacker with
PostgreSQL write access can forge anything; that is equivalent to owning
the system, and is addressed by host and database hardening, not here.

## T-02 Duplicate event flood

**Asset** availability · **Attacker** EXT, SVC

**Path** Mass redelivery drives correlation work and inflates incidents.

**Prevent** `UNIQUE (tenant_id, dedup_key)` rejects duplicates at the
database before any mutable work; bounded ingestion batches apply
backpressure. **Detect** `events_duplicate_total` rate. **Test** the same
event ingested 1 000 times produces one incident and one link row.
**Residual** Duplicate *processing* still costs a database round trip;
sustained volume is a capacity concern, tracked by the alert on ingestion
lag.

## T-03 Correlation-key collision

**Asset** incident integrity · **Attacker** EXT

**Path** Traffic crafted so unrelated attacks share a correlation key,
merging distinct incidents and hiding one inside another.

**Prevent** The key is composed of five exact, canonicalised values —
tenant, target type, target id, direction, family — not a hash, so
collision requires the values to genuinely be equal. IPv6 is normalised
before comparison so two spellings of one address cannot split or merge
unexpectedly. **Detect** `incidents_opened_total` against detection
volume. **Test** canonicalisation property test: equivalent addresses
produce one key; different addresses never produce one. **Residual** Two
genuinely different attacks on the same target, direction, and family
*are* one incident by design. Accepted, and visible because
`policy_refs` and the category record the multiple vectors.

## T-04 Incident ID enumeration

**Asset** confidentiality · **Attacker** TEN, USR

**Path** Iterating IDs to discover incidents, or their count, in another
tenant.

**Prevent** Non-sequential IDs (ADR 0013); **404 rather than 403** for
another tenant's resource, so existence is never confirmed; incident
numbers are per-tenant, so they leak nothing across tenants. **Detect**
`incident_authz_denied_total` and `404` rate per actor. **Test** a valid
ID from tenant B, requested by tenant A, returns 404 — asserted for every
endpoint including audit. **Residual** Per-tenant numbering leaks that
tenant's own volume to its own users. Accepted.

## T-05 Tenant escape

**Asset** all tenant data · **Attacker** TEN, USR

**Path** A query missing its tenant predicate, or a cursor or ID crossing
the boundary.

**Prevent** `tenant_id` on every table; tenant context is a repository
constructor argument so a tenant-less query cannot be expressed; cursors
are signed and carry the tenant; RLS designed for and enabled in Phase 8.
**Detect** audit records with mismatched tenant; denial counter. **Test**
a dedicated isolation suite running every endpoint as tenant A against
tenant B's data. **Residual** Enforcement is application-level until
Phase 8 RLS. This is the largest residual risk in Phase 5 and is recorded
as **R16**.

## T-06 Unauthorized state transition

**Asset** incident integrity · **Attacker** USR

**Path** A viewer closes an incident, or an operator resolves one they
should not.

**Prevent** Per-transition permissions, checked at the command boundary,
not the handler; guard-refused illegal edges. **Detect** every attempt
audited with `result`. **Test** every command × every role, asserting
allow and deny. **Residual** A correctly-permissioned user acting
maliciously is an insider problem; the audit trail is the control.

## T-07 Malicious note content

**Asset** operators and downstream consumers · **Attacker** USR

**Path** A note containing markup, control characters, or a payload aimed
at whatever renders it.

**Prevent** Stored as text, length-bounded, never interpreted by the API.
**Detect** length and rejection counters. **Test** notes containing
markup, null bytes, and very long lines round-trip byte-identically.
**Residual** Rendering safety belongs to consumers; see T-08.

## T-08 Stored XSS

**Asset** UI users · **Attacker** USR

**Path** Note or title content executes in a future web UI.

**Prevent** The API returns JSON and never HTML; content type is pinned;
notes are documented as untrusted. **Deliberately no input sanitisation**
— it destroys the operator's actual words and gives false assurance. The
control belongs at output, in Phase 6. **Detect** UI-side CSP reporting
when the UI exists. **Test** Phase 6 must escape on output; noted as a
Phase 6 acceptance item. **Residual** Real until a UI exists and escapes
correctly. Carried as **R17**.

## T-09 Audit-log injection

**Asset** audit integrity · **Attacker** USR

**Path** Crafted user agent or reason forges audit lines or breaks
parsers.

**Prevent** Structured JSON logging with escaped values, never
concatenation; client-supplied audit fields length-capped with control
characters stripped. **Detect** parse failures in log ingestion.
**Test** newlines and JSON fragments in a reason produce one well-formed
record. **Residual** Minimal.

## T-10 SQL injection

**Asset** everything · **Attacker** USR, TEN

**Path** Unparameterised query construction.

**Prevent** Parameterised queries only; no string interpolation into SQL,
ever; IDs shape-validated before lookup; sort and filter fields mapped
through an allowlist rather than interpolated. **Detect** database error
rate; query logging. **Test** injection payloads in every string
parameter including sort, filter, tag, and cursor. **Residual** Minimal
with a typed query layer; the allowlist for sort fields is the part most
often got wrong.

## T-11 Query exhaustion

**Asset** availability · **Attacker** USR

**Path** Broad or deep queries consuming database capacity.

**Prevent** Server-enforced maximum page size; 90-day range cap; cursor
pagination so deep paging does not degrade; `q` minimum length with no
leading wildcard; non-indexable combinations rejected as
`incident.query_too_broad`; per-actor rate limits; `total` omitted by
default. **Detect** `command_duration_seconds` and rate-limit counters.
**Test** unbounded and deep-page queries are rejected or bounded.
**Residual** A permitted user can still issue expensive-but-legal
queries; rate limiting bounds the damage.

## T-12 Export exhaustion

**Asset** availability, confidentiality · **Attacker** USR

**Path** Repeated large exports as a bulk-extraction or DoS technique.

**Prevent** Separate `incident.export` permission; 10 000-row cap; 5 per
hour per tenant; every export audited with its filter. **Detect** export
audit records and counter. **Test** cap and rate limit enforced; each
export produces an audit row. **Residual** A permitted user can
legitimately extract a lot over time; the audit trail is the control.

## T-13 Optimistic-lock bypass

**Asset** incident integrity · **Attacker** USR

**Path** Omitting `expected_version` to force a write over another
operator's change.

**Prevent** `expected_version` is **required** for state and
safety-relevant mutations; the request is rejected without it rather than
defaulting to the current version. **Detect**
`command_conflicts_total`. **Test** transitions without
`expected_version` are rejected; concurrent conflicting commands produce
exactly one winner. **Residual** Append-only operations do not use
versions by design and cannot conflict.

## T-14 Idempotency-key abuse

**Asset** integrity · **Attacker** USR, TEN

**Path** Reusing a key with different content to have a change silently
dropped, or guessing another tenant's key.

**Prevent** Fingerprint comparison rejects same-key-different-body with
`409`; records scoped to `(tenant_id, key)` so another tenant's key is
invisible; keys are never credentials; length bounded. **Detect**
`incident.idempotency_key_reuse` counter. **Test** the same key with a
different body conflicts; a key never crosses tenants. **Residual**
Minimal.

## T-15 Outbox replay

**Asset** integrity · **Attacker** INS

**Path** Re-marking published rows pending to reprocess events.

**Prevent** Idempotent consumption via `dedup_key` uniqueness makes
replay a no-op for correlation; published rows are purged after 7 days.
**Detect** duplicate counter rising without a corresponding ingestion
rise. **Test** full outbox replay produces no additional incidents.
**Residual** An attacker with database write access has larger
capabilities; see T-01.

## T-16 Dead-letter poisoning

**Asset** availability · **Attacker** EXT

**Path** Malformed events crafted to fill the dead-letter table or stall
the consumer.

**Prevent** Capped retries then quarantine, so a poison event stops
consuming resources; the consumer continues past it; dead-letter rows are
bounded and alerted. **Never executed, interpolated, or rendered.**
**Detect** `dead_letter_pending` alerts at greater than zero. **Test** a
poison event does not stall the queue and lands in dead-letter exactly
once. **Residual** Sustained malformed input is a capacity concern,
surfaced by the alert.

## T-17 Clock manipulation

**Asset** integrity of timings · **Attacker** INS, SVC

**Path** Skewed clocks cause premature auto-close, wrong reopen decisions,
or misordered timelines.

**Prevent** Ordering uses database sequence, not client time; wall clock
is display-only; durations from Phase 4 are monotonic-measured; all
Phase 5 timing uses an injectable clock. **Detect** events with
`observed_at` far from server time counted as skewed. **Test** skewed and
backwards clocks do not reorder a timeline or trigger early auto-close.
**Residual** Gross NTP failure still affects display timestamps.

## T-18 Stale event reopening

**Asset** operator trust · **Attacker** EXT, or simply a backlog

**Path** A delayed `Started` event arrives after resolution and
resurrects an incident an operator has closed.

**Prevent** Late events — older than `last_detected_at` — may link but
may not transition or reopen; reopen requires an event inside the reopen
window measured on server time. **Detect** `events_late_total`.
**Test** a late event after resolution links as evidence and does not
reopen. **Residual** An event delayed by less than the reopen window is
indistinguishable from a genuine recurrence. Accepted.

## T-19 Unauthorized suppression

**Asset** detection coverage · **Attacker** USR, INS

**Path** Suppressing an incident to hide a live attack — the highest-value
single action for an attacker who has a foothold.

**Prevent** Separate `incident.suppress` permission, not bundled into
`operator`; **mandatory `expires_at`**, so no suppression is indefinite;
mandatory reason; suppressed incidents still ingest, link, and count.
**Detect** `incidents_suppressed_total`, audit record, and suppressed
incidents remain visible in list views with a distinct state. **Test**
suppression without expiry is rejected; a suppressed incident still
accumulates events and metrics. **Residual** A `noc_lead` can suppress
for the maximum window; audited, and the expiry bounds it.

## T-20 Unauthorized severity reduction

**Asset** operator trust · **Attacker** USR

**Path** Lowering severity to bury an incident below attention
thresholds.

**Prevent** `incident.severity.change` is separate from `incident.update`;
a reason is **required when lowering**; `severity_source` records that a
human overrode the detection, so the next event does not silently restore
it and mask the override. **Detect** severity-change audit records with
before and after. **Test** lowering without a reason is rejected; every
change is audited with both values. **Residual** A permitted user may
legitimately lower severity; the audit trail is the control.

## T-21 Cross-tenant assignment

**Asset** confidentiality · **Attacker** USR

**Path** Assigning an incident to a user in another tenant, exposing it
to them.

**Prevent** The assignee must belong to the incident's tenant or to an
explicitly cross-tenant platform team; validated server-side against the
directory, never trusted from the request. **Detect** denial counter and
audit. **Test** assigning across tenants is refused. **Residual** Depends
on directory correctness, which Phase 8 hardens.

## T-22 Sensitive evidence leakage

**Asset** confidentiality · **Attacker** USR, TEN

**Path** Evidence references or note content exposing data beyond the
tenant boundary.

**Prevent** Evidence is stored as references, never inline binaries;
references are tenant-scoped and authorized on dereference, not only on
creation; `customer_visible` notes are **refused in Phase 5** because
there is no authorization model for publishing to a customer. **Detect**
audit on evidence access. **Test** an evidence reference from another
tenant returns 404. **Residual** Binary evidence storage is out of scope,
so its access model is undesigned — a gap that must be closed before
evidence attachment ships. **FU-23**.

## T-23 Log leakage

**Asset** confidentiality · **Attacker** anyone with log access

**Path** Note bodies, credentials, or PII in logs shipped to a
centralised system with broader access than the incident API.

**Prevent** Note bodies never logged at `INFO`; credentials and auth
headers never logged; raw detection payloads not logged; structured
fields only. **Detect** log-content review as a release checklist item.
**Test** an assertion that no log line at `INFO` or above contains a note
body. **Residual** `DEBUG` may include more; deployments enabling it
accept that.

## T-24 Prometheus cardinality explosion

**Asset** monitoring availability · **Attacker** EXT, or an own goal

**Path** A high-cardinality label — tenant, target, incident ID —
multiplying series until Prometheus degrades, taking monitoring down
during the attack it is meant to observe.

**Prevent** Every label a closed set; **tenant ID explicitly forbidden**
as a label despite looking bounded, because a managed deployment grows
tenants over time; per-tenant analysis belongs in ClickHouse. **Detect**
series count monitoring. **Test** an assertion enumerating every metric's
label set against an allowlist, mirroring Phase 4's metric tests.
**Residual** Minimal if the test exists; without it, one careless label
added later is enough.

---

## Summary of residual risk

Three carry real residual exposure and are added to the
[risk register](../risk-register.md):

| Risk | Threat | Why it persists |
|---|---|---|
| **R16** | T-05 | Tenant isolation is application-enforced until Phase 8 RLS |
| **R17** | T-08 | Output escaping cannot be proven until a UI exists (Phase 6) |
| **R18** | T-22 | Evidence storage is out of scope, so its access model is undesigned |

The rest reduce to "an attacker with database or host access can do
anything", which is a hardening problem outside this model, or to
"a correctly-permissioned user may act maliciously", for which the audit
trail is the designed control.
