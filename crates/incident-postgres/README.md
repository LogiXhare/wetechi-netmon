# Incident PostgreSQL Adapter

**Status:** Phase 5B-1 dependency probe only. No schema, no migration, no
`IncidentStore` implementation, no database connection — see
[FU-42](../../docs/development/follow-ups.md) for the acceptance gate
this crate exists to satisfy, and [ADR 0029](../../docs/architecture/decisions/0029-phase5b-repository-and-unit-of-work-seam.md)
for why this is the crate's real, final placement rather than a
throwaway.

The six conditionally-accepted dependencies (`uuid`, `tokio-postgres`,
`deadpool-postgres`, `rustls`, `tokio-postgres-rustls`, `refinery`) are
declared here at the exact versions their respective ADRs pinned, and
referenced from a function nothing calls so a normal `cargo build`
actually links every one of them. That makes the measured `cargo tree` /
`cargo audit` / `unsafe` inventory / build result the real numbers for
where these crates will actually ship, not an estimate from a scratch
project.

Milestone 5B-2 onward implements the actual PostgreSQL-backed
`wetechinetmon_incident::store::IncidentStore` here — schema, migrations,
the connection pool, and the transaction-atomicity obligation FU-44
records.
