//! Shared types and utilities used across WetechiNetMon crates.
//!
//! This crate is intentionally small. It exists to avoid duplicating
//! logging setup and a couple of cross-cutting types across the
//! collector, aggregator, and future service crates — it is not a
//! dumping ground for unrelated helpers.

pub mod logging;

/// Errors that can cross crate boundaries within WetechiNetMon.
///
/// Individual crates should generally define their own, more specific
/// error enums (see `wetechinetmon_protocol_ipfix::DecodeError`, for
/// example) and only reach for this shared type at service boundaries
/// where a caller genuinely needs a crate-agnostic error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
