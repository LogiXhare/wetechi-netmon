//! WetechiNetMon incident REST API (Milestone 5D).
//!
//! At this commit, only the gate-8 dependency probe: the crates
//! [ADR 0037](../../docs/architecture/decisions/0037-phase5d-http-framework-and-openapi.md)
//! and [ADR 0038](../../docs/architecture/decisions/0038-phase5d-api-boundary.md)
//! select are added so their closure, advisories, `unsafe` and builds can
//! be measured. No endpoint exists yet.

/// Proves every probed dependency links on this platform. Never called.
fn _probe_every_dependency_links() {
    let _ = std::any::type_name::<axum::Router>();
    let _ = std::any::type_name::<utoipa::openapi::OpenApi>();
    let _: fn(&mut [u8]) -> Result<(), getrandom::Error> = getrandom::fill;
}
