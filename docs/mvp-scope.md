# MVP Scope

Status: Phase 0 draft
Last updated: 2026-08-20

## Definition

The MVP is the v1.0.0 milestone: a production-ready **single-tenant**
release, per the versioning plan in [roadmap.md](roadmap.md). Multi-tenancy
and enterprise auth (v1.1.0/v1.2.0) and distributed HA (v2.0.0) are
explicitly post-MVP.

## In Scope for MVP (v0.1.0 → v1.0.0)

- IPFIX collector (primary protocol), with NetFlow v9, NetFlow v5, and
  sFlow v5 following in that priority order
- Traffic aggregation across the core dimensions (host, network, /24,
  hostgroup, ASN, exporter, interface, protocol) at the required time
  windows
- Direction classification (Incoming/Outgoing/Internal/Other)
- ClickHouse storage for analytics; PostgreSQL for config/metadata;
  Prometheus for metrics
- Static threshold detection with hysteresis, cooldown, dry-run, and
  alert-only modes
- Incident lifecycle state machine, REST API, and CLI for incidents
- Grafana dashboards (ClickHouse + Prometheus datasources) and a native
  NOC web UI covering the core traffic/incident/exporter views
- Notification channels: email (SMTP), Teams, Slack, Telegram, generic
  webhook
- BGP mitigation **lab capability**: GoBGP integration, dry-run, RTBH,
  FlowSpec, withdrawal, reconciliation — with production BGP remaining
  disabled by default even at v1.0.0
- Single-tenant deployment via Docker Compose, Kubernetes/Helm, and
  bare-metal Ubuntu/systemd
- Backup/restore, upgrade/rollback tooling
- Core documentation set (installation, configuration, operations,
  security, API, CLI)
- CI/CD: formatting, linting, unit/integration/fuzz tests, container
  builds, vulnerability scanning, SBOM, signed images

## Explicitly Deferred Past MVP

See [out-of-scope.md](out-of-scope.md) for the full list and rationale.
Headline items: multi-tenancy enforcement, OIDC/Entra ID SSO, advanced
statistical/baseline anomaly detection, managed-service control plane,
distributed HA architecture, SMS notification plugin.

## MVP Acceptance Framing

MVP is "done" when the v1.0.0 exit criteria in
[acceptance-criteria.md](acceptance-criteria.md) are met — not when every
feature in the master prompt exists. The master prompt itself sequences
this via phases 0–10; MVP corresponds to completing phases 0–9 plus the
v1.0.0 release phase (10).

## Reference Lab Environment

Per master prompt section 4, the MVP is developed and tested against a
reference lab configuration only (Ubuntu 24.04 LTS, Cisco NCS540, IPFIX on
UDP/2055, ClickHouse/InfluxDB/Prometheus/Grafana, Nginx or Caddy). This is
a **lab reference for development/testing**, not a production commitment —
production BGP stays administratively disabled regardless of what the lab
demonstrates.
