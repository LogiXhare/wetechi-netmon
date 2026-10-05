# 0039. Phase 5D Manual Incidents

Status: **Accepted**
Date: 2026-10-05
Deciders: Repository owner

## Context

The [API plan](../incident-api-plan.md) lists `POST /incidents` for
manual incident creation: an operator opens an incident for something the
detector cannot see, such as a transit provider reporting spoofed
traffic below our thresholds. The domain had no way to do this. Every
incident was opened by correlation, from a detection event, by the
system actor.

Four questions needed answers:

- Who may open one?
- How does a manual incident relate to correlation?
- What does it hold where a detected incident holds evidence?
- How are retries handled?

On 5 Oct 2026 the owner chose `senior_operator` and above.

## Decision

**A new permission, `incident.create`, in the `senior_operator` bundle
and above.** Opening an incident pages people and starts a timeline, so
it sits with the role that may also resolve and close. `operator` may
not. Ingestion keeps its own `incident.ingest`, which an operator never
holds.

**A manual incident names its target in the detector's vocabulary and
takes the same correlation key.** The target is the four parts of the key
beyond the tenant: scope type (`host`, `prefix`, `slash24`,
`hostgroup_total`), scope identity, direction, and address family. Two
things follow:

- **No second incident for an active target.** If an incident for that key
  is already active, opened by detection or by hand, the request is
  `409 incident.duplicate_active` and names the active incident, which
  the caller may read (same tenant, by construction of the key).
- **Detections attach to the manual incident.** While it is active, a
  detection for the same target attaches to it rather than opening a
  second one. Once traffic is visible the operator's incident gathers the
  evidence, which is the deduplication operators expect from incident
  tools.

A separate namespace for manual incidents was rejected. It would let a
manual incident and a detected one for the same target be open side by
side, which is the duplicate the correlation key exists to prevent.

**Target spelling is the detector's.**

- A `host` is one address.
- A `prefix` is matched as the policy spells it, host bits and all,
  because the detector keys on the configured spelling.
- A `slash24` must be a canonical IPv4 /24.
- A `hostgroup_total` needs an explicit address family.

The domain refuses parts that disagree, for example an IPv6 address
declared as IPv4, with `422 incident.validation_failed`.

**What it holds.** It holds what the operator gave, and nothing invented:

- The operator supplies the title (required, up to 200 characters), an
  optional description (up to 8000), the severity, and optionally the
  priority. Priority defaults from the severity.
- `severity_source` is `operator`.
- The evidence ledger, matched metrics and policy references start empty,
  so the category is `unclassified` until detections attach.
- `created_by` is the operator.
- The incident opens with a timeline entry, an allowed audit entry under
  `incident.create`, and an `incident.opened` outbox event, exactly as a
  detected incident does.

**Retries.** `Idempotency-Key` is required, as on every transition.

- The fingerprint covers the whole request, so the same key and body
  replay the first outcome (`201` with the incident the first call
  opened).
- A different body under the same key is
  `409 incident.idempotency_key_reuse`.
- The persistence load takes the tenant's number allocator lock before
  checking for an active incident, so two concurrent creates for one
  tenant serialize. The loser sees the winner's incident and gets
  `duplicate_active`.

## Consequences

- `POST /api/v1/incidents` answers `201` with the incident and a
  `Location` header.
- `Permission` gains `IncidentCreate`. The audit trail records it under
  that name, so an auditor can tell manual openings from correlated ones
  as well as by the actor.
- Recurrence-based reopening does not apply to a manual create. An
  operator opens a new incident where the previous one is closed, rather
  than reopening it. To reopen, the operator uses `POST .../reopen`.
- A manual incident is not the ingestion path. Detection events still
  arrive only through the inbox
  ([ADR 0035](0035-phase5c-detection-event-inbox.md)).
