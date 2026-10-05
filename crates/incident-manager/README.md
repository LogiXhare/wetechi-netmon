# Incident Manager

**Status:** Milestone 5C. The background service for the incident
lifecycle ([ADR 0036](../../docs/architecture/decisions/0036-phase5c-incident-manager-process.md)).

`wetechinetmon-incident-manager` runs, against the PostgreSQL incident
database:

- **the correlation worker**, which claims detection events from the
  inbox the collector fills
  ([ADR 0035](../../docs/architecture/decisions/0035-phase5c-detection-event-inbox.md))
  and ingests each into an incident;
- **the incident timers**: silent incidents move to `Recovering`, recovery
  is confirmed, and resolved incidents close when the closure policy
  allows it (critical ones never close automatically);
- **retention**, from the retention table;
- **depth gauges** for the inbox, the outbox and dead letters, on
  `/metrics`.

It notifies nobody and mitigates nothing. The outbox is only filled, and
its consumers come in later phases.

## Running it

```sh
WETECHINETMON_INCIDENT_DATABASE_URL="host=db.example.net user=wetechinetmon dbname=incidents sslmode=require" \
WETECHINETMON_INCIDENT_DATABASE_CA_FILE=/etc/wetechinetmon/db-ca.pem \
wetechinetmon-incident-manager
```

Put the password in a libpq password file or the connection string. Keep
it out of shell history and Git. The connection string is never logged.

Startup applies pending migrations under an advisory lock, then starts
work. If the database cannot be reached or migrated, the process exits
non-zero for its supervisor to restart. After startup, database errors
are logged, counted and retried. Ctrl+C or SIGTERM stops it: the batch
being processed finishes, and nothing more is claimed.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `WETECHINETMON_INCIDENT_DATABASE_URL` | required | libpq-style connection string |
| `WETECHINETMON_INCIDENT_DATABASE_CA_FILE` | unset | PEM CA bundle for verifying the server. Without it, the connection string must reach only this host (plaintext loopback); anything else is refused |
| `WETECHINETMON_INCIDENT_DATABASE_CLIENT_CERT_FILE` / `..._CLIENT_KEY_FILE` | unset | mutual TLS; set both, and the CA file |
| `WETECHINETMON_INCIDENT_POOL_MAX_SIZE` | 16 | most database connections |
| `WETECHINETMON_INCIDENT_MIGRATE` | `true` | `false` if migrations are run separately |
| `WETECHINETMON_INCIDENT_METRICS_BIND` | `0.0.0.0:9091` | Prometheus `/metrics` |
| `WETECHINETMON_INCIDENT_WORKER_ID` | host, PID and start time | recorded on claimed inbox rows; must differ between running instances |
| `WETECHINETMON_INCIDENT_WORKER_IDLE_MS` | 1000 | wait after finding the inbox empty |
| `WETECHINETMON_INCIDENT_STATS_INTERVAL_SECS` | 15 | depth gauge refresh |
| `WETECHINETMON_INCIDENT_MAINTENANCE_INTERVAL_SECS` | 60 | incident timers |
| `WETECHINETMON_INCIDENT_RETENTION_INTERVAL_SECS` | 3600 | retention jobs |
| `RUST_LOG` | `info` | log filter; logs are JSON |

An unparseable value, or a zero interval, stops startup with the variable
named. CA bundles and keys are operator-managed files and never belong in
Git ([ADR 0023](../../docs/architecture/decisions/0023-phase5b-postgresql-tls.md)).

## Metrics

All `wetechinetmon_incident_*`; see
[incident-observability.md](../../docs/architecture/incident-observability.md).
No label carries a tenant, an incident, a scope or error text. The
allowlist in `src/metrics.rs` is enforced by a test.

## Tests

`cargo test -p wetechinetmon-incident-manager` runs the unit tests.
`tests/service_lifecycle.rs` runs the whole service against the opt-in,
ephemeral PostgreSQL database named by
`WETECHINETMON_INCIDENT_POSTGRES_TEST_URL`, as the
`crates/incident-postgres` tests do. It skips when that variable is unset
and fails CI if it skips there.
