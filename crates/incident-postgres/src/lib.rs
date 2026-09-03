//! **Phase 5B-1 dependency probe only.** This crate exists so the six
//! conditionally-accepted dependencies (ADR 0019, 0020, 0022, 0023,
//! 0024 — see `docs/development/follow-ups.md` FU-42) can be measured
//! in their real, final placement: `cargo tree`, `cargo audit`, an
//! `unsafe` inventory, and a Windows-GNU build, against the actual
//! crate `wetechinetmon-incident-postgres` that will host the
//! PostgreSQL-backed [`wetechinetmon_incident::store::IncidentStore`]
//! implementation — not a throwaway scratch crate whose numbers might
//! not match what actually ships.
//!
//! **Nothing here implements anything.** No schema, no migration, no
//! `IncidentStore` implementation, no connection to a database. That is
//! explicitly out of scope for the probe (Milestone 5B-2 onward) — see
//! [ADR 0029](../../../docs/architecture/decisions/0029-phase5b-repository-and-unit-of-work-seam.md)
//! for the crate-placement decision this stub fulfills, and FU-42's
//! acceptance gate for what "probe passes" means before any of that
//! begins.
//!
//! Each dependency is referenced by name only, in a function nothing
//! calls, so `cargo build` actually links every one of them — proving
//! the measured dependency closure is the real one a linker would
//! produce, not merely what `Cargo.lock` resolved for an unused
//! `[dependencies]` entry.
#![allow(dead_code, unused_imports)]

/// Never called. Exists only so every probed dependency's crate root is
/// referenced by name, forcing the compiler to actually resolve and link
/// each one rather than merely list it in `Cargo.toml`.
fn _probe_every_dependency_links() {
    let _: fn(uuid::Timestamp) -> uuid::Uuid = uuid::Uuid::new_v7;
    let _ = std::any::type_name::<tokio_postgres::Client>();
    let _ = std::any::type_name::<deadpool_postgres::Pool>();
    let _ = std::any::type_name::<rustls::ClientConfig>();
    let _ = std::any::type_name::<tokio_postgres_rustls::MakeRustlsConnect>();
    let _ = std::any::type_name::<refinery::Report>();
    let _ = std::any::type_name::<dyn wetechinetmon_incident::store::IncidentStore>();
}
