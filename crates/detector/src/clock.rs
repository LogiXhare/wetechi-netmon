//! Time for the detection engine.
//!
//! Every duration the detector cares about — trigger, clear, cooldown,
//! hold-down, event-update interval — is measured against this trait
//! rather than against `Instant::now()` directly. A unit test that had
//! to wait out a five-minute cooldown for real would either be skipped
//! or made meaninglessly short, so the durations that matter operationally
//! would never be the ones under test. With an injectable clock the test
//! suite exercises the real configured values in microseconds.
//!
//! Two readings, deliberately kept separate:
//!
//! - **Monotonic** ([`Clock::monotonic`]) drives every duration
//!   comparison. It cannot jump backwards when the host's wall clock is
//!   corrected by NTP, so a clock correction can never retroactively
//!   satisfy — or un-satisfy — a trigger duration.
//! - **Wall** ([`Clock::wall`]) is only ever stamped onto emitted events
//!   so an operator can correlate them with other systems. No control
//!   flow depends on it.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// The detector's only source of time.
pub trait Clock: Send + Sync {
    /// Monotonic reading, used for every duration comparison.
    fn monotonic(&self) -> Instant;

    /// Wall-clock reading, used only to stamp emitted events.
    fn wall(&self) -> SystemTime;
}

/// The production clock: the operating system's.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn monotonic(&self) -> Instant {
        Instant::now()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// A clock that only moves when a test tells it to.
///
/// Both readings advance together, by the same amount, so a test cannot
/// accidentally construct a state where monotonic and wall time disagree
/// about how much time passed — unless it deliberately calls
/// [`TestClock::advance_wall_only`], which exists precisely to test that
/// wall-clock movement does *not* affect detector control flow.
pub struct TestClock {
    inner: Mutex<(Instant, SystemTime)>,
}

impl TestClock {
    pub fn new() -> Self {
        TestClock {
            inner: Mutex::new((Instant::now(), SystemTime::UNIX_EPOCH)),
        }
    }

    /// Moves both readings forward. Time never moves backwards here —
    /// there is no `rewind`, because a detector that tolerated backwards
    /// monotonic time would be papering over a bug rather than handling
    /// a real condition.
    pub fn advance(&self, by: Duration) {
        let mut guard = self.inner.lock().expect("test clock poisoned");
        guard.0 += by;
        guard.1 += by;
    }

    /// Moves *only* the wall reading, leaving monotonic time untouched —
    /// simulating an NTP correction. Used to prove that no detector
    /// decision depends on wall time.
    pub fn advance_wall_only(&self, by: Duration) {
        let mut guard = self.inner.lock().expect("test clock poisoned");
        guard.1 += by;
    }
}

impl Default for TestClock {
    fn default() -> Self {
        TestClock::new()
    }
}

impl Clock for TestClock {
    fn monotonic(&self) -> Instant {
        self.inner.lock().expect("test clock poisoned").0
    }

    fn wall(&self) -> SystemTime {
        self.inner.lock().expect("test clock poisoned").1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_does_not_move_on_its_own() {
        let clock = TestClock::new();
        let first = clock.monotonic();
        let second = clock.monotonic();
        assert_eq!(first, second);
    }

    #[test]
    fn advance_moves_both_readings_by_the_same_amount() {
        let clock = TestClock::new();
        let mono_before = clock.monotonic();
        let wall_before = clock.wall();

        clock.advance(Duration::from_secs(30));

        assert_eq!(clock.monotonic() - mono_before, Duration::from_secs(30));
        assert_eq!(
            clock.wall().duration_since(wall_before).unwrap(),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn wall_only_advance_leaves_monotonic_untouched() {
        let clock = TestClock::new();
        let mono_before = clock.monotonic();

        clock.advance_wall_only(Duration::from_secs(3600));

        assert_eq!(clock.monotonic(), mono_before);
        assert_eq!(
            clock.wall().duration_since(SystemTime::UNIX_EPOCH).unwrap(),
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn system_clock_monotonic_never_goes_backwards() {
        let clock = SystemClock;
        let first = clock.monotonic();
        let second = clock.monotonic();
        assert!(second >= first);
    }
}
