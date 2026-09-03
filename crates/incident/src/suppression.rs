//! Suppression — an attribute, not a state.
//!
//! Three fields, none of them the lifecycle state: a deadline, a reason,
//! and the actor who suppressed it. Whether an incident is currently
//! suppressed is **derived** from comparing the deadline against now,
//! never stored as a separate boolean, so a suppression cannot outlive
//! its own expiry through a missed sweep.
//!
//! The deadline is computed as `suppressed_at.checked_plus(duration)` on
//! a [`DurableTimestamp`] — a relative duration from an operator command,
//! turned into an absolute deadline immediately, rather than an absolute
//! wall-clock instant accepted from the caller. The checked form is
//! required because `duration` is operator-supplied and unbounded:
//! `IncidentUnitOfWork::suppress` rejects with a validation error before
//! any mutation if it would overflow, rather than panicking. An
//! indefinite suppression is how a real attack gets missed, so the expiry
//! is mandatory — there is no constructor that omits it.
//!
//! 5A evaluated expiry against the **monotonic** half of a
//! [`crate::clock::Timestamp`], which kept it immune to a wall-clock
//! correction. That immunity was only ever available to a process that
//! never restarts: a monotonic reading cannot be written to a database
//! and cannot be compared across processes, so a suppression that
//! survives a restart has to be evaluated against durable UTC. Per ADR
//! 0031 the comparison *semantics* are unchanged — expiry is still
//! strict, so the deadline instant itself is not suppressed — and only
//! the representation being compared changed.
//!
//! [`Suppression`] itself is still not `Serialize`: it carries an
//! [`Actor`], and the crate keeps the "typed domain, JSON only at the
//! boundary" rule it follows for timeline and audit payloads too.
//! [`Suppression::to_display`] produces the boundary-safe DTO.

use serde::{Deserialize, Serialize};

use crate::authorization::Actor;
use crate::durable_time::DurableTimestamp;

pub const SUPPRESSION_REASON_MAX_LEN: usize = 500;

/// Validates a suppression reason against its bound, matching
/// [`crate::incident::validate_note_body`] and
/// [`crate::incident::validate_title`]'s pattern (FU-32): checked before
/// any mutation, so an oversized reason refuses the whole command instead
/// of being stored truncated or unbounded.
pub fn validate_reason(reason: &str) -> Result<(), crate::error::IncidentError> {
    if reason.chars().count() > SUPPRESSION_REASON_MAX_LEN {
        return Err(crate::error::IncidentError::ValidationError(format!(
            "suppression reason exceeds {SUPPRESSION_REASON_MAX_LEN} characters"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    deadline: DurableTimestamp,
    pub reason: String,
    pub by: Actor,
}

/// The boundary-safe, serializable rendering of a [`Suppression`].
///
/// `until` is the full-precision durable deadline. 5A rendered it as
/// milliseconds since the epoch through a hand-written serde module that
/// truncated sub-millisecond precision and silently mapped any pre-epoch
/// instant to the epoch itself; carrying the [`DurableTimestamp`]
/// directly removes both, and removes the hand-written module with them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuppressionDisplay {
    pub until: DurableTimestamp,
    pub reason: String,
    pub by: Actor,
}

impl Suppression {
    pub fn new(reason: impl Into<String>, by: Actor, deadline: DurableTimestamp) -> Self {
        Suppression {
            deadline,
            reason: reason.into(),
            by,
        }
    }

    /// Whether this suppression is still active at `now`. The exact
    /// expiry instant itself is not active — `is_before` is strict.
    ///
    /// This is an ordering question, not an elapsed-duration one, so it
    /// stays infallible: two instants always compare, whichever way round
    /// they are. Only [`DurableTimestamp::checked_elapsed_since`], which
    /// has to answer *how long*, can report clock skew.
    pub fn is_active(&self, now: &DurableTimestamp) -> bool {
        now.is_before(&self.deadline)
    }

    pub fn deadline(&self) -> DurableTimestamp {
        self.deadline
    }

    pub fn to_display(&self) -> SuppressionDisplay {
        SuppressionDisplay {
            until: self.deadline,
            reason: self.reason.clone(),
            by: self.by.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::Actor;
    use crate::clock::TestClock;
    use std::time::Duration;

    #[test]
    fn a_suppression_is_active_until_its_deadline_and_not_after() {
        let clock = TestClock::new();
        let now0 = DurableTimestamp::now(&clock).unwrap();
        let deadline = now0.checked_plus(Duration::from_secs(3600)).unwrap();
        let suppression =
            Suppression::new("noisy known scanner", Actor::system_correlator(), deadline);

        clock.advance(Duration::from_secs(1800));
        let midway = DurableTimestamp::now(&clock).unwrap();
        assert!(suppression.is_active(&midway));

        clock.advance(Duration::from_secs(1800));
        let at_deadline = DurableTimestamp::now(&clock).unwrap();
        assert!(
            !suppression.is_active(&at_deadline),
            "expiry instant itself must not be active"
        );

        clock.advance(Duration::from_secs(1));
        let after = DurableTimestamp::now(&clock).unwrap();
        assert!(!suppression.is_active(&after));
    }

    #[test]
    fn to_display_carries_the_durable_deadline_without_truncation() {
        let deadline = DurableTimestamp::from_micros(1_756_000_000_123_456);
        let suppression = Suppression::new("test", Actor::system_correlator(), deadline);
        let display = suppression.to_display();
        assert_eq!(display.until, deadline);
        assert_eq!(display.reason, "test");
        let json = serde_json::to_string(&display).unwrap();
        assert_eq!(
            serde_json::from_str::<SuppressionDisplay>(&json).unwrap(),
            display,
            "the display DTO must survive a serialization round trip exactly"
        );
    }

    #[test]
    fn a_reason_at_exactly_the_bound_is_valid() {
        let reason: String = "a".repeat(SUPPRESSION_REASON_MAX_LEN);
        assert!(validate_reason(&reason).is_ok());
    }

    #[test]
    fn a_reason_one_over_the_bound_is_rejected() {
        let reason: String = "a".repeat(SUPPRESSION_REASON_MAX_LEN + 1);
        assert!(validate_reason(&reason).is_err());
    }
}
