# WetechiNetMon

**See Every Flow. Defend Every Network.**

WetechiNetMon is an independently engineered open network telemetry, DDoS
detection, traffic analytics, and policy-controlled mitigation platform
for ISPs, enterprises, data centers, hosting providers, and managed
network service providers.

> **Project status: Phase 1 — repository and documentation foundation.**
> No software has shipped yet. See [Project Status](#project-status)
> below before evaluating this as a runnable product.

Built by **WeTechi Solutions**.

---

## Table of Contents

- [Project Status](#project-status)
- [What WetechiNetMon Is](#what-wetechinetmon-is)
- [What WetechiNetMon Is Not](#what-wetechinetmon-is-not)
- [Architecture](#architecture)
- [Core Capabilities (Planned)](#core-capabilities-planned)
- [Getting Started](#getting-started)
- [Repository Layout](#repository-layout)
- [Documentation](#documentation)
- [Roadmap](#roadmap)
- [Security](#security)
- [License](#license)
- [Contributing](#contributing)
- [Support](#support)

## Project Status

This repository is at **Phase 1** of an explicit, phased delivery model
(see [ROADMAP.md](ROADMAP.md)). Phase 0 (product foundation and
clean-room boundary) is complete. Phase 1 establishes the repository
skeleton, governance, and documentation/validation tooling — **it
contains no application code and nothing here is runnable yet.** The
first runnable component (the IPFIX telemetry collector) ships in Phase 2.

## What WetechiNetMon Is

WetechiNetMon is an independently engineered open network telemetry, DDoS
detection, traffic analytics, and policy-controlled mitigation platform.
Planned product categories:

- NetFlow / IPFIX / sFlow monitoring
- Network traffic analytics
- DDoS and network anomaly detection
- Incident management
- Grafana dashboards
- BGP RTBH and BGP FlowSpec automation
- Managed DDoS monitoring and policy-controlled mitigation

Full detail: [docs/product-charter.md](docs/product-charter.md).

## What WetechiNetMon Is Not

WetechiNetMon is not a clone, replica, copy, alternative build,
reverse-engineered edition, or replacement edition of any proprietary
product, named or unnamed. It is built exclusively from public RFCs,
public protocol specifications, vendor documentation, and independently
designed schemas and interfaces. See
[docs/clean-room-boundary.md](docs/clean-room-boundary.md) for the full,
binding clean-room policy.

## Architecture

WetechiNetMon is designed as a modular, event-driven platform of 15
logical services (Telemetry Collector, Traffic Aggregator, Direction
Classifier, Detection Engine, Incident Manager, Mitigation Controller,
Notification Service, Public REST API, Internal gRPC API, Web Application,
CLI, Configuration Service, Audit Service, Reporting Service, Backup and
Restore Service).

Architecture direction (not yet finalized — tracked via Architecture
Decision Records once decided):

- [docs/architecture-options.md](docs/architecture-options.md)
- [docs/technology-options.md](docs/technology-options.md)
- [docs/architecture/decisions/](docs/architecture/decisions/index.md)

*A visual architecture diagram will be added once the core service
boundaries are implemented (Phase 2–5) — a placeholder link only, no
diagram exists yet.*

## Core Capabilities (Planned)

| Area | Summary |
|---|---|
| Telemetry Collector | IPFIX, NetFlow v9/v5, sFlow v5 decoding with template caching and sampling correction |
| Aggregation | Traffic totals across host/network/hostgroup/ASN/exporter/interface dimensions |
| Detection | Static thresholds first, statistical/baseline anomaly detection later |
| Incident Management | Explicit incident state machine with full audit trail |
| Mitigation | GoBGP-integrated RTBH/FlowSpec, dry-run and disabled by default |
| Dashboards | Original Grafana dashboards and a native NOC web UI |
| Notifications | Email, Teams, Slack, Telegram, PagerDuty, generic webhook |
| Multi-Tenancy | Designed in from the schema level, enforced starting v1.1.0 |

Full requirement traceability:
[docs/functional-requirements.md](docs/functional-requirements.md) and
[docs/non-functional-requirements.md](docs/non-functional-requirements.md).

## Getting Started

There is nothing to install or run yet — see
[docs/development/local-setup.md](docs/development/local-setup.md) for
what you *can* do today (documentation preview, Markdown/YAML validation).
Installation guides for Docker Compose, Kubernetes/Helm, and bare-metal
Ubuntu will be published starting Phase 2.

*Screenshots and a quick-start walkthrough will be added once the web
application and NOC UI exist (Phase 6) — placeholder only, no
screenshots exist yet.*

## Repository Layout

```text
wetechi-netmon/
├── .github/          CI, issue/PR templates, CODEOWNERS, Dependabot
├── apps/             api, web, cli — reserved, Phase 2+
├── crates/           Rust service crates — reserved, Phase 2+
├── deployments/       docker-compose, kubernetes, helm, systemd — reserved
├── docs/             product, architecture, and operational documentation
├── grafana/          dashboards, provisioning — reserved, Phase 6
├── database/         ClickHouse, PostgreSQL schemas and migrations — reserved
├── tests/            integration, replay, performance, security, fixtures
├── tools/            flow-generator, flow-replay, diagnostics, migration
├── scripts/          developer/CI helper scripts
├── examples/         example configurations
├── branding/         logos and brand assets
├── mkdocs.yml        documentation site configuration
├── Makefile          developer task runner (make targets)
├── Taskfile.yml      developer task runner (Task targets)
└── prompts/          the governing master prompt for this project
```

Each currently-empty directory contains a `README.md` explaining what it
is reserved for and which phase populates it — see
[docs/roadmap.md](docs/roadmap.md).

## Documentation

Full documentation set (Phase 0 + Phase 1):

- [Product Charter](docs/product-charter.md)
- [Clean-Room Boundary](docs/clean-room-boundary.md)
- [Functional Requirements](docs/functional-requirements.md)
- [Non-Functional Requirements](docs/non-functional-requirements.md)
- [Architecture Options](docs/architecture-options.md)
- [Technology Options](docs/technology-options.md)
- [Dependency License Matrix](docs/dependency-license-matrix.md)
- [Commercial Boundaries](docs/commercial-boundaries.md)
- [Security Principles](docs/security-principles.md)
- [MVP Scope](docs/mvp-scope.md) / [Out of Scope](docs/out-of-scope.md)
- [Risk Register](docs/risk-register.md)
- [Detailed Roadmap](docs/roadmap.md)
- [Acceptance Criteria](docs/acceptance-criteria.md)
- [Blocking Questions](docs/blocking-questions.md)
- [Naming and Branding](docs/naming-and-branding.md)
- [License Recommendation Warning](docs/license-recommendation.md)

A browsable documentation site (MkDocs Material) will be published once
hosting is configured; until then, browse `docs/` directly or run
`mkdocs serve` locally per
[docs/development/local-setup.md](docs/development/local-setup.md).

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the milestone summary and
[docs/roadmap.md](docs/roadmap.md) for full per-phase detail.

## Security

See [SECURITY.md](SECURITY.md) for how to report a vulnerability, and
[docs/security-principles.md](docs/security-principles.md) for the
project's threat model and security design principles. **Automated BGP
mitigation is disabled and dry-run by default at every layer, permanently
— this is a hard safety rule, not a configuration default that will
change.**

## License

Licensed under the [Apache License 2.0](LICENSE).

**⚠️ This license selection is a Phase 1 recommendation pending explicit
confirmation by WeTechi Solutions — see
[docs/license-recommendation.md](docs/license-recommendation.md) and
[docs/blocking-questions.md](docs/blocking-questions.md) (BQ-1) before
relying on it for any legal or commercial decision.**

See also [NOTICE](NOTICE) and
[docs/dependency-license-matrix.md](docs/dependency-license-matrix.md).

## Contributing

Contributions are welcome once there is code to contribute to. Read
[CONTRIBUTING.md](CONTRIBUTING.md) first — in particular the clean-room
and dependency-licensing rules, which are non-negotiable. See also
[GOVERNANCE.md](GOVERNANCE.md) and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Support

See [SUPPORT.md](SUPPORT.md).

---

© 2026 WeTechi Solutions. WetechiNetMon, SentinelFlow Engine, and
`wetechinetmonctl` are original names — see
[docs/naming-and-branding.md](docs/naming-and-branding.md).
