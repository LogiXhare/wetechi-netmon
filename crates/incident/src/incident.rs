//! The `Incident` aggregate.
//!
//! Every field here is either immutable after creation, mutable only
//! through [`crate::transition`]'s guarded functions, or derived. There
//! is no public setter that bypasses a guard, a version bump, a timeline
//! entry, and an audit entry together — the aggregate's own API does not
//! offer a way to do only one of those.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wetechinetmon_detector::{AddressFamily, MetricKind, ScopeId, ScopeType, TrafficDirection};

use crate::assignment::Assignment;
use crate::authorization::Actor;
use crate::category::IncidentCategory;
use crate::clock::Timestamp;
use crate::closure::ClosureReason;
use crate::correlation::{CorrelationKey, TenantId};
use crate::evidence::EvidenceLedger;
use crate::id::IncidentId;
use crate::limits::{
    DESCRIPTION_MAX_LEN, NOTES_PER_INCIDENT_MAX, NOTE_BODY_MAX_LEN, POLICY_REFS_MAX,
    TAGS_PER_INCIDENT_MAX, TITLE_MAX_LEN,
};
use crate::number::IncidentNumber;
use crate::severity::{Priority, Severity, SeveritySource};
use crate::state::IncidentState;
use crate::suppression::Suppression;

pub const INCIDENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteVisibility {
    Internal,
    /// Stored, never honoured in Phase 5A — there is no customer-facing
    /// surface and no tenant authorization model to decide who may
    /// publish to one. A command requesting this is refused.
    CustomerVisible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub index: u32,
    pub body: String,
    pub visibility: NoteVisibility,
    pub created_by: Actor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRef {
    pub policy_id: String,
    pub policy_version: u32,
    pub first_seen_sequence: u64,
    pub last_seen_sequence: u64,
}

/// The aggregate. `pub(crate)` mutation is the norm; only
/// [`crate::transition`] and [`crate::unit_of_work`] hold `&mut Incident`
/// with intent to change it, and every path they use also appends a
/// timeline entry, an audit entry, and bumps `version`.
#[derive(Debug, Clone, PartialEq)]
pub struct Incident {
    // Identity — immutable after creation.
    pub incident_id: IncidentId,
    pub incident_number: IncidentNumber,
    pub schema_version: u32,
    pub tenant_id: TenantId,
    pub correlation_key: CorrelationKey,
    pub address_family: AddressFamily,
    pub direction: TrafficDirection,
    pub target_type: ScopeType,
    pub target_identity: ScopeId,
    pub created_by: Actor,

    // Title/description — derived-then-mutable.
    pub title: String,
    pub description: Option<String>,

    // State and priority.
    pub state: IncidentState,
    pub severity: Severity,
    pub severity_source: SeveritySource,
    pub priority: Priority,
    pub closure_reason: Option<ClosureReason>,
    pub(crate) state_before_recovering: Option<IncidentState>,
    pub suppression: Option<Suppression>,
    pub version: u64,

    // Category — derived, recomputed on every linked event.
    pub category: IncidentCategory,
    pub(crate) matched_metrics: BTreeSet<MetricKind>,

    // Timestamps.
    pub first_detected_at: Timestamp,
    pub opened_at: Timestamp,
    pub last_detected_at: Timestamp,
    pub last_updated_at: Timestamp,
    pub acknowledged_at: Option<Timestamp>,
    pub recovering_since: Option<Timestamp>,
    pub resolved_at: Option<Timestamp>,
    pub closed_at: Option<Timestamp>,
    pub reopened_at: Option<Timestamp>,
    pub reopen_count: u32,

    // Ownership.
    pub assignment: Assignment,
    pub updated_by: Actor,

    // Evidence, notes, tags, policy references — all bounded.
    pub evidence: EvidenceLedger,
    pub notes: Vec<Note>,
    pub tags: BTreeMap<String, String>,
    pub policy_refs: Vec<PolicyRef>,
}

impl Incident {
    /// Whether this incident is "open" for correlation purposes — any
    /// state other than `Closed`.
    pub fn is_open_for_correlation(&self) -> bool {
        !self.state.is_terminal()
    }

    /// Whether this incident is eligible to be the target of a recurrence
    /// reopen — `Resolved` or `Closed`. Distinct from "terminal"
    /// (`Closed` only): a `Resolved` incident is not terminal but is
    /// removed from the active correlation index the moment it resolves,
    /// so the reopen-candidate search must use this predicate, not
    /// `!state.is_terminal()` and not `state.is_terminal()` — see the
    /// state machine plan's "a `Resolved` incident may be reopened, not
    /// only a `Closed` one".
    pub fn is_reopen_candidate(&self) -> bool {
        matches!(self.state, IncidentState::Resolved | IncidentState::Closed)
    }

    /// The timestamp a reopen decision measures elapsed time from:
    /// `resolved_at` when `Resolved`, `closed_at` when `Closed`. `None`
    /// for any other state, which callers treat as "not eligible".
    pub fn reopen_reference_timestamp(&self) -> Option<&Timestamp> {
        match self.state {
            IncidentState::Resolved => self.resolved_at.as_ref(),
            IncidentState::Closed => self.closed_at.as_ref(),
            _ => None,
        }
    }

    pub fn is_suppressed(&self, now: &Timestamp) -> bool {
        self.suppression.as_ref().is_some_and(|s| s.is_active(now))
    }

    pub fn notes_at_capacity(&self) -> bool {
        self.notes.len() >= NOTES_PER_INCIDENT_MAX
    }

    pub fn tags_at_capacity(&self) -> bool {
        self.tags.len() >= TAGS_PER_INCIDENT_MAX
    }

    pub fn policy_refs_at_capacity(&self) -> bool {
        self.policy_refs.len() >= POLICY_REFS_MAX
    }
}

/// Validates a title against its bound. Returns the owned, validated
/// string so a caller cannot construct an `Incident` with an oversized
/// one by accident.
pub fn validate_title(title: &str) -> Result<(), crate::error::IncidentError> {
    if title.chars().count() > TITLE_MAX_LEN {
        return Err(crate::error::IncidentError::ValidationError(format!(
            "title exceeds {TITLE_MAX_LEN} characters"
        )));
    }
    Ok(())
}

pub fn validate_description(description: &str) -> Result<(), crate::error::IncidentError> {
    if description.chars().count() > DESCRIPTION_MAX_LEN {
        return Err(crate::error::IncidentError::ValidationError(format!(
            "description exceeds {DESCRIPTION_MAX_LEN} characters"
        )));
    }
    Ok(())
}

pub fn validate_note_body(body: &str) -> Result<(), crate::error::IncidentError> {
    if body.chars().count() > NOTE_BODY_MAX_LEN {
        return Err(crate::error::IncidentError::ValidationError(format!(
            "note body exceeds {NOTE_BODY_MAX_LEN} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_at_exactly_the_bound_is_valid() {
        let title: String = "a".repeat(TITLE_MAX_LEN);
        assert!(validate_title(&title).is_ok());
    }

    #[test]
    fn a_title_one_over_the_bound_is_rejected() {
        let title: String = "a".repeat(TITLE_MAX_LEN + 1);
        assert!(validate_title(&title).is_err());
    }

    #[test]
    fn a_note_body_over_the_bound_is_rejected() {
        let body: String = "a".repeat(NOTE_BODY_MAX_LEN + 1);
        assert!(validate_note_body(&body).is_err());
    }
}
