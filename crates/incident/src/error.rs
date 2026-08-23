//! Structured domain errors. No variant leaks internal detail a client
//! should not see; every variant carries only machine-readable fields.

use thiserror::Error;

use crate::correlation::CorrelationConflict;
use crate::id::IncidentId;
use crate::state::IncidentState;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IncidentError {
    #[error("incident not found")]
    NotFound,

    #[error("illegal transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: IncidentState,
        to: IncidentState,
    },

    #[error("version conflict: expected {expected}, current is {current} ({current_state:?})")]
    VersionConflict {
        expected: u64,
        current: u64,
        current_state: IncidentState,
    },

    #[error("idempotency key reused with a different request")]
    IdempotencyConflict,

    #[error("tenant mismatch")]
    TenantMismatch,

    #[error("unauthorized")]
    Unauthorized,

    #[error("validation failed: {0}")]
    ValidationError(String),

    #[error("capacity exceeded: {0}")]
    CapacityExceeded(&'static str),

    #[error("an open incident already exists for this correlation key: {0:?}")]
    DuplicateActiveIncident(IncidentId),

    #[error("reopen is not valid: {0}")]
    InvalidReopen(String),

    #[error("critical incidents require manual closure")]
    ManualClosureRequired,

    #[error("this operation is not valid while the incident is suppressed")]
    SuppressedOperation,

    #[error("the requested evidence is not available")]
    EvidenceUnavailable,

    #[error("internal invariant violated: {0}")]
    InternalInvariantViolation(&'static str),

    #[error("state unchanged: already {0:?}")]
    StateUnchanged(IncidentState),

    #[error("correlation conflict: {0}")]
    Correlation(#[from] CorrelationConflict),
}

impl IncidentError {
    /// The stable, machine-readable code a future API would return —
    /// matching the `incident.*` vocabulary the API and CLI plans use.
    pub fn code(&self) -> &'static str {
        match self {
            IncidentError::NotFound => "incident.not_found",
            IncidentError::InvalidTransition { .. } => "incident.illegal_transition",
            IncidentError::VersionConflict { .. } => "incident.version_conflict",
            IncidentError::IdempotencyConflict => "incident.idempotency_key_reuse",
            IncidentError::TenantMismatch => "incident.tenant_mismatch",
            IncidentError::Unauthorized => "incident.forbidden",
            IncidentError::ValidationError(_) => "incident.validation_failed",
            IncidentError::CapacityExceeded(_) => "incident.limit_reached",
            IncidentError::DuplicateActiveIncident(_) => "incident.duplicate_active",
            IncidentError::InvalidReopen(_) => "incident.invalid_reopen",
            IncidentError::ManualClosureRequired => "incident.manual_closure_required",
            IncidentError::SuppressedOperation => "incident.suppressed_operation",
            IncidentError::EvidenceUnavailable => "incident.evidence_unavailable",
            IncidentError::InternalInvariantViolation(_) => "incident.internal_invariant_violation",
            IncidentError::StateUnchanged(_) => "incident.state_unchanged",
            IncidentError::Correlation(_) => "incident.correlation_conflict",
        }
    }
}
