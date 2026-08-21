//! WetechiNetMon's detection engine.
//!
//! Consumes protocol-independent traffic snapshots produced from the
//! Phase 3 pipeline, evaluates them against operator-written policies,
//! and emits explainable detection events. See
//! docs/architecture/detection-engine.md.
//!
//! **This crate cannot mitigate traffic.** It has no network client, no
//! command execution, and no dependency that could reach a router. That
//! is a property of the dependency graph, not a runtime check — see
//! docs/security/detection-safety.md and ADR 0007.

pub mod clock;
pub mod input;
pub mod policy;

pub use clock::{Clock, SystemClock, TestClock};
pub use input::{
    AddressFamily, CompletenessFlag, DataCompleteness, DetectionSnapshot, MetricKind, MetricRates,
    MetricUnit, SamplingStatus, ScopeId, ScopeKey, ScopeType, TrafficDirection, ALL_METRIC_KINDS,
};
pub use policy::{
    ClearPercent, DetectionPolicy, ExecutionMode, PolicyDraft, PolicyError, PolicySelector,
    Severity, TenantPrefixes, Thresholds,
};
