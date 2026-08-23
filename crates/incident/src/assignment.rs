//! Assignment.
//!
//! No user database in 5A — `Assignee` is an opaque directory reference,
//! never an email, ready to integrate with a real identity provider
//! later without this type changing shape.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Assignee {
    User { id: String },
    Team { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Assignment {
    pub assignee: Option<Assignee>,
}

impl Assignment {
    pub fn unassigned() -> Self {
        Assignment { assignee: None }
    }

    pub fn is_assigned(&self) -> bool {
        self.assignee.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_assignment_is_unassigned() {
        assert!(!Assignment::unassigned().is_assigned());
    }

    #[test]
    fn assigning_a_team_is_assigned() {
        let assignment = Assignment {
            assignee: Some(Assignee::Team {
                id: "noc-a".to_string(),
            }),
        };
        assert!(assignment.is_assigned());
    }
}
