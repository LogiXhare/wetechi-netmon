# Incident API

**Status:** Milestone 5D, foundation in place. `wetechinetmon-api` serves
liveness and readiness on axum
([ADR 0037](../../docs/architecture/decisions/0037-phase5d-http-framework-and-openapi.md)),
with the boundary rules of
[ADR 0038](../../docs/architecture/decisions/0038-phase5d-api-boundary.md).
The incident endpoints come next.

## What is here

| Module | Purpose |
|---|---|
| `problem` | RFC 9457 problem details. `ErrorCode` is the registry behind [docs/api/error-codes.md](../../docs/api/error-codes.md). Another tenant's incident is `incident.not_found`, and internal faults say only `api.internal`. |
| `request_id` | A UUIDv7 per request, in `X-Request-Id`, every problem body and the tracing span. A client-sent id is never trusted. |
| `rate_limit` | GCRA on `std` (no `unsafe`), bounded keys that fail closed when full, property-tested against the conformance bound. |
| `server` | Loopback may be plaintext (for a same-host proxy). Any other bind requires TLS with rustls, and handshakes run off the accept loop with a deadline. |
| `openapi` | The document generated from the handlers, checked against [docs/api/openapi.json](../../docs/api/openapi.json). |

## Running it

```sh
WETECHINETMON_API_DATABASE_URL="host=127.0.0.1 user=wetechinetmon dbname=incidents" \
wetechinetmon-api
```

| Variable | Default | Meaning |
|---|---|---|
| `WETECHINETMON_API_BIND` | `127.0.0.1:8080` | Listen address. Not loopback: TLS files are required. |
| `WETECHINETMON_API_TLS_CERT_FILE` / `..._TLS_KEY_FILE` | unset | PEM certificate chain and key. Set both. Never commit them. |
| `WETECHINETMON_API_DATABASE_URL` | required | libpq connection string. Never logged. |
| `WETECHINETMON_API_DATABASE_CA_FILE` and `..._CLIENT_CERT_FILE` / `..._CLIENT_KEY_FILE` | unset | Database TLS, the same rule as the incident manager. |
| `WETECHINETMON_API_POOL_MAX_SIZE` | 16 | Most database connections. |
| `RUST_LOG` | `info` | Log filter; logs are JSON. |

`GET /healthz` is liveness: it answers while the process runs.
`GET /readyz` is readiness: `503 api.unavailable` until the database
answers. Neither needs a token.

## Changing the API

Annotate every handler with `#[utoipa::path]` and list it in
`openapi::ApiDoc`, then regenerate the committed document:

```sh
WETECHINETMON_UPDATE_OPENAPI=1 cargo test -p wetechinetmon-api openapi
```

A new error code goes in `problem::ErrorCode` and in
[docs/api/error-codes.md](../../docs/api/error-codes.md). The tests fail
if either is missing.
