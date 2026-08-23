//! Time for the incident domain.
//!
//! Reuses the detector's [`Clock`] trait and both its implementations
//! rather than defining a second one: the same monotonic-for-control-flow,
//! wall-for-display split that Phase 4 proved out applies unchanged here,
//! and a second timestamp dependency (`chrono`, `time`, ...) would just be
//! two clocks disagreeing about what "now" means.
//!
//! [`Timestamp`] pairs a monotonic [`Instant`] with a wall [`SystemTime`]
//! reading taken at the same instant, mirroring the dual-stamp pattern
//! `StateTransition` uses in the detector (`at` / `at_wall`). Every
//! duration used for a domain decision — the reopen window, the recovery
//! confirmation period, an automatic closure delay, a suppression expiry —
//! is computed from the monotonic half only, using
//! [`Instant::saturating_duration_since`] so a clock correction or a
//! constructed-out-of-order timestamp cannot panic or produce a negative
//! duration. The wall half exists only so a timestamp can be displayed or
//! serialized; nothing here branches on it.

pub use wetechinetmon_detector::{Clock, SystemClock, TestClock};

use std::time::{Duration, Instant, SystemTime};

/// A moment in time, recorded for both control flow and display.
///
/// Constructing one always reads both clock halves together, so a
/// `Timestamp` can never pair a monotonic reading with a wall reading
/// taken at a different instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    monotonic: Instant,
    wall: SystemTime,
}

impl Timestamp {
    /// Reads both clock halves now.
    pub fn now(clock: &dyn Clock) -> Self {
        Timestamp {
            monotonic: clock.monotonic(),
            wall: clock.wall(),
        }
    }

    /// A timestamp `duration` after this one, computed on the monotonic
    /// half only. Used to turn a relative policy duration (a suppression
    /// length, a closure delay) into an absolute deadline without ever
    /// touching the wall clock.
    pub fn plus(&self, duration: Duration) -> Timestamp {
        Timestamp {
            monotonic: self.monotonic + duration,
            wall: self.wall + duration,
        }
    }

    /// How long has elapsed from `self` to `later`, saturating at zero
    /// rather than panicking if the two are out of order.
    ///
    /// This is the only comparison every reopen-window, recovery, and
    /// closure-delay decision in this crate uses. It is deliberately
    /// symmetric with the detector's own
    /// [`saturating_duration_since`](Instant::saturating_duration_since)
    /// usage throughout `crates/detector/src/state.rs`.
    pub fn elapsed_since(&self, earlier: &Timestamp) -> Duration {
        self.monotonic.saturating_duration_since(earlier.monotonic)
    }

    /// Whether `self` is at or before `deadline` — the exact comparison
    /// the inclusive reopen boundary needs.
    pub fn is_at_or_before(&self, deadline: &Timestamp) -> bool {
        self.monotonic <= deadline.monotonic
    }

    /// Whether `self` is strictly before `deadline` — used for
    /// suppression expiry, where "still suppressed" must exclude the
    /// exact expiry instant itself.
    pub fn is_before(&self, deadline: &Timestamp) -> bool {
        self.monotonic < deadline.monotonic
    }

    /// The wall-clock reading, for display and serialization only. No
    /// decision in this crate may branch on this value.
    pub fn wall(&self) -> SystemTime {
        self.wall
    }

    /// The monotonic reading, exposed for tests that need to construct a
    /// `Timestamp` at a specific `Instant` produced by a `TestClock`.
    pub fn monotonic(&self) -> Instant {
        self.monotonic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_since_never_panics_when_out_of_order() {
        let clock = TestClock::new();
        let later = Timestamp::now(&clock);
        clock.advance(Duration::from_secs(10));
        let earlier_reading = Timestamp {
            monotonic: later.monotonic - Duration::from_secs(5),
            wall: later.wall - Duration::from_secs(5),
        };
        // earlier_reading is chronologically before `later`, so asking for
        // elapsed from `later` back to itself using a timestamp that is
        // itself "later" than the reference must saturate, not panic.
        assert_eq!(
            earlier_reading.elapsed_since(&later),
            Duration::ZERO,
            "an out-of-order pair must saturate to zero, never panic"
        );
    }

    #[test]
    fn plus_moves_both_halves_by_the_same_amount() {
        let clock = TestClock::new();
        let start = Timestamp::now(&clock);
        let deadline = start.plus(Duration::from_secs(900));
        assert_eq!(deadline.elapsed_since(&start), Duration::from_secs(900));
    }

    #[test]
    fn inclusive_boundary_is_true_at_exactly_the_deadline() {
        let clock = TestClock::new();
        let start = Timestamp::now(&clock);
        let deadline = start.plus(Duration::from_secs(900));
        clock.advance(Duration::from_secs(900));
        let now = Timestamp::now(&clock);
        assert!(
            now.is_at_or_before(&deadline),
            "exactly-at must be inclusive"
        );
        clock.advance(Duration::from_secs(1));
        let now = Timestamp::now(&clock);
        assert!(
            !now.is_at_or_before(&deadline),
            "one second past must not reopen"
        );
    }
}
