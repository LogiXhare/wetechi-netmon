//! The outbox — populated on every mutation, published by nobody in
//! Phase 5A.
//!
//! Typed events, matching the same "no raw `serde_json::Value` as the
//! primary record" rule as [`crate::timeline`] and [`crate::audit`].
//! `IncidentReopened` and the other reserved-but-unconsumed event types
//! exist because leaving the seam is cheaper than retrofitting it later
//! — see [ADR 0011](../../../docs/architecture/decisions/0011-incident-domain-boundary.md)
//! — not because anything in this crate reads them.

use serde::{Deserialize, Serialize};

use crate::correlation::TenantId;
use crate::id::IncidentId;
use crate::severity::{Priority, Severity};
use crate::state::IncidentState;

pub const OUTBOX_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboxEvent {
    IncidentOpened,
    IncidentUpdated,
    IncidentAcknowledged,
    IncidentStateChanged {
        from: IncidentState,
        to: IncidentState,
    },
    IncidentRecovering,
    IncidentResolved,
    IncidentClosed,
    IncidentReopened {
        reopen_count: u32,
    },
    IncidentAssignmentChanged,
    IncidentSeverityChanged {
        from: Severity,
        to: Severity,
    },
    IncidentPriorityChanged {
        from: Priority,
        to: Priority,
    },
    IncidentSuppressionChanged {
        suppressed: bool,
    },
}

impl OutboxEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            OutboxEvent::IncidentOpened => "incident.opened",
            OutboxEvent::IncidentUpdated => "incident.updated",
            OutboxEvent::IncidentAcknowledged => "incident.acknowledged",
            OutboxEvent::IncidentStateChanged { .. } => "incident.state_changed",
            OutboxEvent::IncidentRecovering => "incident.recovering",
            OutboxEvent::IncidentResolved => "incident.resolved",
            OutboxEvent::IncidentClosed => "incident.closed",
            OutboxEvent::IncidentReopened { .. } => "incident.reopened",
            OutboxEvent::IncidentAssignmentChanged => "incident.assignment_changed",
            OutboxEvent::IncidentSeverityChanged { .. } => "incident.severity_changed",
            OutboxEvent::IncidentPriorityChanged { .. } => "incident.priority_changed",
            OutboxEvent::IncidentSuppressionChanged { .. } => "incident.suppression_changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxMessage {
    pub schema_version: u32,
    pub sequence: u64,
    pub tenant: TenantId,
    pub incident_id: IncidentId,
    pub event: OutboxEvent,
}

impl OutboxMessage {
    pub fn new(
        sequence: u64,
        tenant: TenantId,
        incident_id: IncidentId,
        event: OutboxEvent,
    ) -> Self {
        OutboxMessage {
            schema_version: OUTBOX_SCHEMA_VERSION,
            sequence,
            tenant,
            incident_id,
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_is_stable_and_namespaced() {
        assert_eq!(
            OutboxEvent::IncidentReopened { reopen_count: 1 }.event_type(),
            "incident.reopened"
        );
    }
}
