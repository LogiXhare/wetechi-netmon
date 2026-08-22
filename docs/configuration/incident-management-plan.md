# Incident Management Configuration — Plan

Status: **Planning only.** Part of the
[Phase 5 plan](../architecture/phase5-incident-management-plan.md).
**None of these settings exist yet.** No environment variable listed here
is read by any binary at `3f0cf3e`.

Phase 5 follows the configuration conventions the collector already uses:
environment variables, a `WETECHINETMON_` prefix, no hardcoded hosts or
credentials, and a startup failure rather than a silent fallback when a
value is invalid.

## Storage

| Variable | Default | Notes |
|---|---|---|
| `INCIDENT_DB_URL` | — | **Required.** PostgreSQL connection URL |
| `INCIDENT_DB_MAX_CONNECTIONS` | `16` | Pool ceiling |
| `INCIDENT_DB_CONNECT_TIMEOUT_SECS` | `10` | |
| `INCIDENT_DB_STATEMENT_TIMEOUT_SECS` | `30` | Bounds a runaway query |
| `INCIDENT_DB_APPLICATION_NAME` | `wetechinetmon-incident` | Shows in `pg_stat_activity` |

The credential belongs in the URL, and the URL belongs in the
environment — never in a committed file, never on a command line where it
lands in shell history and process listings. Logs must show the host and
database name only, never the full URL.

## Correlation

| Variable | Default | Notes |
|---|---|---|
| `INCIDENT_REOPEN_WINDOW_SECS` | `900` | **BQ-9** |
| `INCIDENT_RECOVERY_CONFIRMATION_SECS` | `300` | `Recovering` hold before `Resolved` |
| `INCIDENT_DETECTOR_SILENCE_SECS` | `0` | `0` means derive: 3 × window, minimum 300 |
| `INCIDENT_AUTO_CLOSE_ENABLED` | `true` | |
| `INCIDENT_AUTO_CLOSE_AFTER_SECS` | `86400` | |
| `INCIDENT_AUTO_CLOSE_MIN_SEVERITY` | `critical` | At or above requires manual close — **BQ-8** |
| `INCIDENT_OBSERVE_MODE_INGEST` | `true` | Ingest `Observe` events for counting; they never open incidents |

`INCIDENT_DETECTOR_SILENCE_SECS = 0` deriving from the detection window
rather than taking a fixed default is deliberate: a policy with a
60-second window and one with a 5-second window need different silence
thresholds, and a single global number would be wrong for one of them.

## Ingestion

| Variable | Default | Notes |
|---|---|---|
| `INCIDENT_INGEST_BATCH_SIZE` | `500` | |
| `INCIDENT_INGEST_POLL_INTERVAL_MS` | `1000` | Fallback when `LISTEN`/`NOTIFY` is unavailable |
| `INCIDENT_INGEST_MAX_ATTEMPTS` | `5` | Then dead-letter |
| `INCIDENT_INGEST_BACKOFF_BASE_MS` | `500` | Exponential, with jitter |
| `INCIDENT_INGEST_BACKOFF_MAX_MS` | `60000` | |
| `INCIDENT_SUPPORTED_EVENT_SCHEMA_MAX` | `1` | Higher versions are quarantined, never guessed |

## Limits

| Variable | Default |
|---|---|
| `INCIDENT_MAX_OPEN_PER_TENANT` | `10000` |
| `INCIDENT_MAX_EVENTS_PER_INCIDENT` | `10000` |
| `INCIDENT_MAX_TIMELINE_ENTRIES` | `50000` |
| `INCIDENT_MAX_NOTES_PER_INCIDENT` | `500` |
| `INCIDENT_MAX_NOTE_BYTES` | `16000` |
| `INCIDENT_MAX_TAGS` | `32` |
| `INCIDENT_MAX_AFFECTED_TARGETS` | `256` |
| `INCIDENT_MAX_PAGE_SIZE` | `200` |
| `INCIDENT_MAX_EXPORT_ROWS` | `10000` |
| `INCIDENT_MAX_QUERY_RANGE_DAYS` | `90` |

`INCIDENT_MAX_TIMELINE_ENTRIES` is an **alert threshold, not a truncation
point**. The timeline is never truncated; reaching this number raises
`wetechinetmon_incident_timeline_pressure`.

## Retention

| Variable | Default |
|---|---|
| `INCIDENT_RETENTION_CLOSED_DAYS` | `730` |
| `INCIDENT_RETENTION_AUDIT_DAYS` | `730` |
| `INCIDENT_RETENTION_IDEMPOTENCY_HOURS` | `24` |
| `INCIDENT_RETENTION_OUTBOX_PUBLISHED_DAYS` | `7` |
| `INCIDENT_RETENTION_DEAD_LETTER_DAYS` | `90` |

Audit retention must be **greater than or equal to** incident retention.
A configuration where audit expires first should fail at startup, not be
discovered during an investigation.

Dead-letter rows are never purged while unreviewed, regardless of this
setting.

## API

| Variable | Default |
|---|---|
| `INCIDENT_API_BIND` | `127.0.0.1:8081` |
| `INCIDENT_API_RATE_MUTATIONS_PER_MIN` | `60` |
| `INCIDENT_API_RATE_QUERIES_PER_MIN` | `120` |
| `INCIDENT_API_RATE_EXPORTS_PER_HOUR` | `5` |
| `INCIDENT_API_REQUEST_TIMEOUT_SECS` | `30` |
| `INCIDENT_API_MAX_BODY_BYTES` | `65536` |

Binding to loopback by default is deliberate. An incident API reachable
on every interface the moment it is installed is a configuration mistake
waiting to happen; exposing it should be a decision an operator makes.

## Analytics export

| Variable | Default |
|---|---|
| `INCIDENT_ANALYTICS_ENABLED` | `false` |
| `INCIDENT_ANALYTICS_BATCH_SIZE` | `200` |
| `INCIDENT_ANALYTICS_FLUSH_INTERVAL_SECS` | `10` |

Reuses the collector's existing ClickHouse configuration rather than
introducing a second URL, so there is one place to change the endpoint.

## Validation at startup

Following the collector's existing pattern, an invalid value is a startup
error, never a silent fallback. Specifically: the database URL must be
present and parseable; `recovery_confirmation` must be greater than zero;
audit retention must be at least incident retention; every limit must be
greater than zero; `auto_close_after` must exceed
`recovery_confirmation`; and the bind address must parse.

## Not configurable, deliberately

Some things are invariants rather than policies, and exposing them as
settings would invite a deployment to turn off its own correctness:

- Whether mutations write audit records.
- Whether the timeline is append-only.
- Whether tenant isolation is enforced.
- Whether illegal transitions are refused.
- Whether `executed`, `mitigation_status`, and `notification_status` can
  be anything other than false or `none` in Phase 5.
