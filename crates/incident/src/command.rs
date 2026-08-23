//! Typed domain commands.
//!
//! Every command that changes state or a safety-relevant field carries
//! `expected_version` per ADR 0016; notes and tags do not, since they are
//! append-only or set-semantic and cannot meaningfully conflict.
//! Validated payload construction (bounded strings, etc.) happens where
//! the command is handled, in [`crate::unit_of_work`], not in this enum
//! — the enum's job is only to name the operation and carry its data.

use std::time::Duration;

use serde::Serialize;

use crate::assignment::Assignee;
use crate::authorization::Permission;
use crate::closure::ClosureReason;
use crate::incident::NoteVisibility;
use crate::severity::{Priority, Severity};
use crate::timeline::OperatorCommandKind;

/// `Serialize` (not `Deserialize`) is enough: a command is only ever
/// turned into canonical bytes for [`crate::idempotency::RequestFingerprint`],
/// never parsed back out of them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Command {
    AcknowledgeIncident {
        expected_version: u64,
    },
    BeginInvestigation {
        expected_version: u64,
    },
    MarkMonitoring {
        expected_version: u64,
    },
    ResolveIncident {
        expected_version: u64,
        resolution_note: Option<String>,
    },
    CloseIncident {
        expected_version: u64,
        reason: ClosureReason,
        detail: Option<String>,
    },
    ReopenIncident {
        expected_version: u64,
        reason: String,
    },
    SuppressIncident {
        expected_version: u64,
        reason: String,
        duration: Duration,
    },
    UnsuppressIncident {
        expected_version: u64,
    },
    AssignIncident {
        expected_version: u64,
        assignee: Assignee,
    },
    UnassignIncident {
        expected_version: u64,
    },
    ChangeSeverity {
        expected_version: u64,
        new_severity: Severity,
        reason: Option<String>,
    },
    ChangePriority {
        expected_version: u64,
        new_priority: Priority,
    },
    AddNote {
        body: String,
        visibility: NoteVisibility,
    },
    AddTag {
        key: String,
        value: String,
    },
    RemoveTag {
        key: String,
    },
}

impl Command {
    pub fn kind(&self) -> OperatorCommandKind {
        match self {
            Command::AcknowledgeIncident { .. } => OperatorCommandKind::Acknowledge,
            Command::BeginInvestigation { .. } => OperatorCommandKind::BeginInvestigation,
            Command::MarkMonitoring { .. } => OperatorCommandKind::MarkMonitoring,
            Command::ResolveIncident { .. } => OperatorCommandKind::Resolve,
            Command::CloseIncident { .. } => OperatorCommandKind::Close,
            Command::ReopenIncident { .. } => OperatorCommandKind::Reopen,
            Command::SuppressIncident { .. } => OperatorCommandKind::Suppress,
            Command::UnsuppressIncident { .. } => OperatorCommandKind::Unsuppress,
            Command::AssignIncident { .. } => OperatorCommandKind::Assign,
            Command::UnassignIncident { .. } => OperatorCommandKind::Unassign,
            Command::ChangeSeverity { .. } => OperatorCommandKind::ChangeSeverity,
            Command::ChangePriority { .. } => OperatorCommandKind::ChangePriority,
            Command::AddNote { .. } => OperatorCommandKind::AddNote,
            Command::AddTag { .. } => OperatorCommandKind::AddTag,
            Command::RemoveTag { .. } => OperatorCommandKind::RemoveTag,
        }
    }

    pub fn required_permission(&self) -> Permission {
        match self {
            Command::AcknowledgeIncident { .. } => Permission::IncidentAcknowledge,
            Command::BeginInvestigation { .. } | Command::MarkMonitoring { .. } => {
                Permission::IncidentInvestigate
            }
            Command::ResolveIncident { .. } => Permission::IncidentResolve,
            Command::CloseIncident { .. } => Permission::IncidentClose,
            Command::ReopenIncident { .. } => Permission::IncidentReopen,
            Command::SuppressIncident { .. } | Command::UnsuppressIncident { .. } => {
                Permission::IncidentSuppress
            }
            Command::AssignIncident { .. } | Command::UnassignIncident { .. } => {
                Permission::IncidentAssign
            }
            Command::ChangeSeverity { .. } => Permission::IncidentSeverityChange,
            Command::ChangePriority { .. } => Permission::IncidentPriorityChange,
            Command::AddNote { .. } => Permission::IncidentNoteCreate,
            Command::AddTag { .. } | Command::RemoveTag { .. } => Permission::IncidentUpdate,
        }
    }

    /// Whether this command requires `expected_version` per ADR 0016 —
    /// every state transition and every safety-relevant field change.
    /// Notes and tags do not: notes are append-only, tags are
    /// set-semantic, and neither can meaningfully conflict.
    pub fn requires_expected_version(&self) -> bool {
        !matches!(
            self,
            Command::AddNote { .. } | Command::AddTag { .. } | Command::RemoveTag { .. }
        )
    }

    pub fn expected_version(&self) -> Option<u64> {
        match self {
            Command::AcknowledgeIncident { expected_version }
            | Command::BeginInvestigation { expected_version }
            | Command::MarkMonitoring { expected_version }
            | Command::ResolveIncident {
                expected_version, ..
            }
            | Command::CloseIncident {
                expected_version, ..
            }
            | Command::ReopenIncident {
                expected_version, ..
            }
            | Command::SuppressIncident {
                expected_version, ..
            }
            | Command::UnsuppressIncident { expected_version }
            | Command::AssignIncident {
                expected_version, ..
            }
            | Command::UnassignIncident { expected_version }
            | Command::ChangeSeverity {
                expected_version, ..
            }
            | Command::ChangePriority {
                expected_version, ..
            } => Some(*expected_version),
            Command::AddNote { .. } | Command::AddTag { .. } | Command::RemoveTag { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_and_tags_do_not_require_expected_version() {
        assert!(!Command::AddNote {
            body: "x".into(),
            visibility: NoteVisibility::Internal
        }
        .requires_expected_version());
        assert!(!Command::AddTag {
            key: "k".into(),
            value: "v".into()
        }
        .requires_expected_version());
        assert!(!Command::RemoveTag { key: "k".into() }.requires_expected_version());
    }

    #[test]
    fn state_transitions_require_expected_version() {
        assert!(Command::AcknowledgeIncident {
            expected_version: 1
        }
        .requires_expected_version());
        assert!(Command::CloseIncident {
            expected_version: 1,
            reason: ClosureReason::Resolved,
            detail: None
        }
        .requires_expected_version());
    }

    #[test]
    fn closure_policy_override_is_never_a_command_permission() {
        // The override permission is a policy-level concern (crate::closure),
        // never something a per-incident command requires — it must not be
        // reachable through any Command variant's required_permission().
        let commands = [
            Command::AcknowledgeIncident {
                expected_version: 1,
            },
            Command::CloseIncident {
                expected_version: 1,
                reason: ClosureReason::Resolved,
                detail: None,
            },
        ];
        for command in commands {
            assert_ne!(
                command.required_permission(),
                Permission::IncidentClosurePolicyOverride
            );
        }
    }
}
