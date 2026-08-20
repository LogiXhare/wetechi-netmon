//! Bounded, multi-dimensional traffic aggregation and rate-window
//! calculation. See docs/architecture/aggregation.md and ADR 0003
//! (in-memory aggregation structure).

mod aggregator;
mod bounded_map;
mod counters;
mod rate_window;

pub use aggregator::{Aggregator, AggregatorConfig, IngestReport};
pub use bounded_map::{BoundedMap, BoundedMapConfig, UpsertOutcome};
pub use counters::TrafficCounters;
pub use rate_window::{RateSample, RateWindowSet, WINDOW_DURATIONS};
