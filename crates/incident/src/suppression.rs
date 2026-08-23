//! Suppression — an attribute, not a state.
//!
//! Three fields, none of them the lifecycle state: a deadline, a reason,
//! and the actor who suppressed it. Whether an incident is currently
//! suppressed is **derived** from comparing the deadline against now,
//! never stored as a separate boolean, so a suppression cannot outlive
//! its own expiry through a missed sweep.
//!
//! The deadline is computed as `suppressed_at.plus(duration)` on the
//! **monotonic** half of a [`crate::clock::Timestamp`] — a relative
//! duration from an operator command, turned into an absolute deadline
//! immediately, rather than an absolute wall-clock instant accepted from
//! the caller. This keeps expiry evaluation immune to a wall-clock
//! correction, matching the detector's own monotonic-only philosophy for
//! every timer that drives a decision (see `crates/detector/src/clock.rs`).
//! An indefinite suppression is how a real attack gets missed, so the
//! expiry is mandatory — there is no constructor that omits it.
//!
//! [`Suppression`] itself is not `Serialize` — it carries a
//! process-local [`Timestamp`], which (like the detector's own
//! `Instant`-bearing types) has no meaningful cross-process wire form.
//! [`Suppression::to_display`] produces the boundary-safe DTO, per the
//! "typed domain, JSON only at the boundary" rule this crate follows
//! throughout for timeline and audit payloads too.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::authorization::Actor;
use crate::clock::Timestamp;

pub const SUPPRESSION_REASON_MAX_LEN: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    deadline: Timestamp,
    pub reason: String,
    pub by: Actor,
}

/// The boundary-safe, serializable rendering of a [`Suppression`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuppressionDisplay {
    #[serde(with = "wall_millis")]
    pub until_wall: SystemTime,
    pub reason: String,
    pub by: Actor,
}

mod wall_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(value: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        let millis = value
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        millis.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_millis(millis))
    }
}

impl Suppression {
    pub fn new(reason: impl Into<String>, by: Actor, deadline: Timestamp) -> Self {
        Suppression {
            deadline,
            reason: reason.into(),
            by,
        }
    }

    /// Whether this suppression is still active at `now`. The exact
    /// expiry instant itself is not active — `is_before` is strict.
    pub fn is_active(&self, now: &Timestamp) -> bool {
        now.is_before(&self.deadline)
    }

    pub fn deadline(&self) -> Timestamp {
        self.deadline
    }

    pub fn to_display(&self) -> SuppressionDisplay {
        SuppressionDisplay {
            until_wall: self.deadline.wall(),
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
        let now0 = Timestamp::now(&clock);
        let deadline = now0.plus(Duration::from_secs(3600));
        let suppression =
            Suppression::new("noisy known scanner", Actor::system_correlator(), deadline);

        clock.advance(Duration::from_secs(1800));
        let midway = Timestamp::now(&clock);
        assert!(suppression.is_active(&midway));

        clock.advance(Duration::from_secs(1800));
        let at_deadline = Timestamp::now(&clock);
        assert!(
            !suppression.is_active(&at_deadline),
            "expiry instant itself must not be active"
        );

        clock.advance(Duration::from_secs(1));
        let after = Timestamp::now(&clock);
        assert!(!suppression.is_active(&after));
    }

    #[test]
    fn to_display_carries_the_wall_clock_deadline() {
        let clock = TestClock::new();
        let now0 = Timestamp::now(&clock);
        let deadline = now0.plus(Duration::from_secs(60));
        let suppression = Suppression::new("test", Actor::system_correlator(), deadline);
        let display = suppression.to_display();
        assert_eq!(display.until_wall, deadline.wall());
        assert_eq!(display.reason, "test");
    }
}
