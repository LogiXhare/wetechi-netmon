//! Reopen policy and the inclusive boundary — BQ-9.
//!
//! No environment parsing here either: [`ReopenPolicy::new`] validates a
//! typed `Duration` against the approved 0–24h range and returns a
//! structured error on violation; reading a setting from the process
//! environment is out of scope for this crate.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::clock::Timestamp;

const MAX_REOPEN_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_REOPEN_WINDOW: Duration = Duration::from_secs(15 * 60);

/// A validated reopen window. `Duration::ZERO` is legal and means
/// reopening is disabled — every recurrence creates a new incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReopenPolicy {
    window: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReopenPolicyError {
    #[error("reopen window {0:?} exceeds the maximum accepted value of 24 hours")]
    WindowTooLong(Duration),
}

impl ReopenPolicy {
    pub fn new(window: Duration) -> Result<Self, ReopenPolicyError> {
        if window > MAX_REOPEN_WINDOW {
            return Err(ReopenPolicyError::WindowTooLong(window));
        }
        Ok(ReopenPolicy { window })
    }

    /// The approved 15-minute technical default. Not a legal, regulatory,
    /// contractual, or SLA requirement — see BQ-9.
    pub const fn approved_default() -> Self {
        ReopenPolicy {
            window: DEFAULT_REOPEN_WINDOW,
        }
    }

    pub fn window(&self) -> Duration {
        self.window
    }

    /// Whether a recurrence at `now`, measured from `closed_or_resolved_at`,
    /// falls inside the reopen window. The boundary is **inclusive**:
    /// elapsed `<=` window reopens — except that a zero window always
    /// means "never reopen", which is a special case stated explicitly
    /// in the design (window `0` disables reopening) rather than a
    /// consequence of the inclusive-boundary comparison, since `0 <= 0`
    /// would otherwise reopen on a zero-elapsed recurrence.
    pub fn reopens(&self, closed_or_resolved_at: &Timestamp, now: &Timestamp) -> bool {
        if self.window.is_zero() {
            return false;
        }
        let elapsed = now.elapsed_since(closed_or_resolved_at);
        elapsed <= self.window
    }
}

impl Default for ReopenPolicy {
    fn default() -> Self {
        Self::approved_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;

    #[test]
    fn zero_window_never_reopens() {
        let policy = ReopenPolicy::new(Duration::ZERO).unwrap();
        let clock = TestClock::new();
        let closed = Timestamp::now(&clock);
        let now = Timestamp::now(&clock);
        assert!(
            !policy.reopens(&closed, &now),
            "even zero elapsed time must not reopen when window is zero"
        );
    }

    #[test]
    fn recurrence_at_fourteen_fifty_nine_reopens() {
        let policy = ReopenPolicy::approved_default();
        let clock = TestClock::new();
        let closed = Timestamp::now(&clock);
        clock.advance(Duration::from_secs(14 * 60 + 59));
        let now = Timestamp::now(&clock);
        assert!(policy.reopens(&closed, &now));
    }

    #[test]
    fn recurrence_at_exactly_fifteen_minutes_reopens_inclusive_boundary() {
        let policy = ReopenPolicy::approved_default();
        let clock = TestClock::new();
        let closed = Timestamp::now(&clock);
        clock.advance(Duration::from_secs(15 * 60));
        let now = Timestamp::now(&clock);
        assert!(
            policy.reopens(&closed, &now),
            "exactly 15 minutes must be inclusive"
        );
    }

    #[test]
    fn recurrence_at_fifteen_zero_one_creates_a_new_incident() {
        let policy = ReopenPolicy::approved_default();
        let clock = TestClock::new();
        let closed = Timestamp::now(&clock);
        clock.advance(Duration::from_secs(15 * 60 + 1));
        let now = Timestamp::now(&clock);
        assert!(!policy.reopens(&closed, &now));
    }

    #[test]
    fn the_maximum_accepted_window_is_twenty_four_hours() {
        assert!(ReopenPolicy::new(Duration::from_secs(24 * 60 * 60)).is_ok());
        assert_eq!(
            ReopenPolicy::new(Duration::from_secs(24 * 60 * 60 + 1)),
            Err(ReopenPolicyError::WindowTooLong(Duration::from_secs(
                24 * 60 * 60 + 1
            )))
        );
    }

    #[test]
    fn the_default_is_fifteen_minutes() {
        assert_eq!(
            ReopenPolicy::approved_default().window(),
            Duration::from_secs(900)
        );
    }
}
