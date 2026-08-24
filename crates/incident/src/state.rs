//! The incident lifecycle.
//!
//! Seven states, one terminal (`Closed`). Deliberately does not mirror
//! the detector's five-state machine — see
//! [ADR 0014](../../../docs/architecture/decisions/0014-incident-state-machine.md).
//! `Reopened` is a transition, not a state; `Suppressed` is an attribute
//! (see [`crate::suppression`]), not a state.
//!
//! Automatic and operator transitions are validated by **two separate**
//! guards, following the design's own split between "traffic-driven" and
//! "person-driven" edges. This is not cosmetic: `Recovering -> Investigating`
//! is legal when the *cause* is a recovery abort restoring
//! `state_before_recovering` (an automatic transition), and illegal when
//! an operator tries to hand-steer a recovering incident sideways (the
//! table in the state-machine plan is explicit that these must be told
//! apart). A single combined guard cannot express that distinction; two
//! guards can.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncidentState {
    Open,
    Acknowledged,
    Investigating,
    Monitoring,
    Recovering,
    Resolved,
    Closed,
}

impl IncidentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            IncidentState::Open => "open",
            IncidentState::Acknowledged => "acknowledged",
            IncidentState::Investigating => "investigating",
            IncidentState::Monitoring => "monitoring",
            IncidentState::Recovering => "recovering",
            IncidentState::Resolved => "resolved",
            IncidentState::Closed => "closed",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, IncidentState::Closed)
    }

    /// Legal edges when the actor is `system:correlator` or the clock —
    /// detection-driven and staleness-driven transitions, plus the
    /// recovery-abort restoration to whichever state preceded
    /// `Recovering`.
    pub fn can_automatic_transition_to(&self, next: IncidentState) -> bool {
        use IncidentState::{
            Acknowledged, Closed, Investigating, Monitoring, Open, Recovering, Resolved,
        };
        matches!(
            (self, next),
            (Open, Recovering)
                | (Acknowledged, Recovering)
                | (Investigating, Recovering)
                | (Monitoring, Recovering)
                | (Recovering, Resolved)
                // Recovery-abort: restores whichever state preceded
                // Recovering. All four are legal destinations here and
                // nowhere else for an automatic actor.
                | (Recovering, Open)
                | (Recovering, Acknowledged)
                | (Recovering, Investigating)
                | (Recovering, Monitoring)
                // Non-critical auto-close. Critical is refused by
                // crate::closure's guard before this is ever consulted.
                | (Resolved, Closed)
                // Reopen on recurrence within the window.
                | (Resolved, Open)
                | (Closed, Open)
        )
    }

    /// Legal edges when the actor is an operator issuing a command.
    /// Notably: an operator may never move `Recovering` anywhere except
    /// to `Resolved` — recovery either completes or is aborted
    /// automatically, and an operator cannot hand-steer it sideways.
    pub fn can_operator_transition_to(&self, next: IncidentState) -> bool {
        use IncidentState::{
            Acknowledged, Closed, Investigating, Monitoring, Open, Recovering, Resolved,
        };
        matches!(
            (self, next),
            (Open, Acknowledged)
                | (Open, Investigating)
                | (Acknowledged, Investigating)
                | (Monitoring, Investigating)
                | (Acknowledged, Monitoring)
                | (Investigating, Monitoring)
                | (Open, Resolved)
                | (Acknowledged, Resolved)
                | (Investigating, Resolved)
                | (Monitoring, Resolved)
                | (Recovering, Resolved)
                | (Resolved, Closed)
                | (Resolved, Open)
                | (Closed, Open)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use IncidentState::*;

    #[test]
    fn every_legal_automatic_edge_is_accepted() {
        for (from, to) in [
            (Open, Recovering),
            (Acknowledged, Recovering),
            (Investigating, Recovering),
            (Monitoring, Recovering),
            (Recovering, Resolved),
            (Recovering, Open),
            (Recovering, Acknowledged),
            (Recovering, Investigating),
            (Recovering, Monitoring),
            (Resolved, Closed),
            (Resolved, Open),
            (Closed, Open),
        ] {
            assert!(
                from.can_automatic_transition_to(to),
                "{from:?} -> {to:?} should be legal automatically"
            );
        }
    }

    #[test]
    fn open_can_never_automatically_reach_monitoring() {
        assert!(!Open.can_automatic_transition_to(Monitoring));
    }

    #[test]
    fn operator_cannot_hand_steer_recovering_sideways() {
        for to in [Acknowledged, Investigating, Monitoring, Open] {
            assert!(
                !Recovering.can_operator_transition_to(to),
                "an operator must not move Recovering to {to:?} directly"
            );
        }
        assert!(Recovering.can_operator_transition_to(Resolved));
    }

    #[test]
    fn open_to_monitoring_is_illegal_for_an_operator_too() {
        assert!(!Open.can_operator_transition_to(Monitoring));
    }

    #[test]
    fn closed_only_exits_via_reopen() {
        for to in [
            Open,
            Acknowledged,
            Investigating,
            Monitoring,
            Recovering,
            Resolved,
        ] {
            if to == Open {
                assert!(
                    Closed.can_operator_transition_to(to) || Closed.can_automatic_transition_to(to)
                );
            } else {
                assert!(!Closed.can_operator_transition_to(to));
                assert!(!Closed.can_automatic_transition_to(to));
            }
        }
    }

    #[test]
    fn closing_is_only_legal_from_resolved() {
        for state in [Open, Acknowledged, Investigating, Monitoring, Recovering] {
            assert!(!state.can_operator_transition_to(Closed));
            assert!(!state.can_automatic_transition_to(Closed));
        }
        assert!(Resolved.can_operator_transition_to(Closed));
    }

    #[test]
    fn no_state_transitions_to_itself() {
        for state in [
            Open,
            Acknowledged,
            Investigating,
            Monitoring,
            Recovering,
            Resolved,
            Closed,
        ] {
            assert!(
                !state.can_automatic_transition_to(state),
                "{state:?} self-edge automatic"
            );
            assert!(
                !state.can_operator_transition_to(state),
                "{state:?} self-edge operator"
            );
        }
    }

    #[test]
    fn closed_is_the_only_terminal_state() {
        for state in [
            Open,
            Acknowledged,
            Investigating,
            Monitoring,
            Recovering,
            Resolved,
        ] {
            assert!(!state.is_terminal());
        }
        assert!(Closed.is_terminal());
    }
}
