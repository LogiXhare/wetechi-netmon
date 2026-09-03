//! Durable, cross-process time for the incident domain.
//!
//! [`crate::clock::Timestamp`] pairs a monotonic [`std::time::Instant`]
//! with a wall [`SystemTime`], and every 5A decision compares the
//! *monotonic* half deliberately, because a monotonic reading cannot go
//! backward under a clock correction. That choice is correct for a single
//! process that never restarts, and only for that.
//!
//! `Instant` is defined by the standard library as opaque and
//! process-local: it has no fixed epoch and no cross-process meaning. It
//! cannot be written to a database, and a value read back after a restart
//! is not comparable to one taken before it. There is no mapping from a
//! persisted wall time back to a new process's `Instant` epoch, so a
//! persistence layer claiming to restore monotonic ordering would be
//! silently falling back to wall-clock comparison while advertising a
//! guarantee it cannot provide.
//!
//! [`DurableTimestamp`] is therefore the representation every *persisted*
//! decision compares against: a UTC instant as microseconds since the Unix
//! epoch, which is exactly PostgreSQL's `timestamptz` resolution, so a
//! round trip through the database is lossless. `Instant` keeps its
//! original job — process-local latency and timeout measurement — and
//! loses the one it was never suited for.
//!
//! Because a wall clock *can* go backward (an NTP correction, a
//! misconfigured host, a deliberate skew), the comparison 5A could
//! perform infallibly becomes fallible here.
//! [`DurableTimestamp::checked_elapsed_since`] returns a structured
//! [`ClockSkew`] rather than saturating to zero: clamping silently would
//! turn "I cannot trust this comparison" into "no time has passed", which
//! is a decision the domain is not entitled to make on the caller's
//! behalf. See ADR 0031.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::clock::{Clock, Timestamp};
use crate::error::IncidentError;

/// Microseconds per second, the resolution this type stores.
const MICROS_PER_SEC: i64 = 1_000_000;

/// Nanoseconds per microsecond, for detecting sub-microsecond remainders
/// that must round *down* (toward negative infinity) for pre-epoch times.
const NANOS_PER_MICRO: u32 = 1_000;

/// A UTC instant, durable across processes and restarts.
///
/// Stored as microseconds since the Unix epoch, matching PostgreSQL's
/// `timestamptz` resolution exactly so that a value written to the
/// database and read back is bit-identical. Sub-microsecond precision
/// from a host clock is truncated toward negative infinity on
/// construction, never on comparison, so two timestamps that compare
/// equal here also compare equal after a round trip.
///
/// Serialized as a plain integer, not a formatted string: this crate has
/// no date-formatting dependency and must not acquire one (ADR 0011), and
/// an integer cannot be misparsed by a reader that disagrees about time
/// zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DurableTimestamp {
    micros_since_epoch: i64,
}

/// A comparison the domain refused to make because the two timestamps
/// were out of order — the decision time was earlier than the persisted
/// reference it was being measured from.
///
/// Carries both readings so a caller can log, alert, or retry with enough
/// context to tell a one-off correction from a systematically wrong
/// clock. It deliberately does **not** carry an incident's contents: this
/// is an operational fault, not a domain event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSkew {
    /// The persisted timestamp the comparison was measured from.
    pub reference_micros: i64,
    /// The decision timestamp that turned out to be earlier than it.
    pub decision_micros: i64,
}

impl ClockSkew {
    /// How far backward the decision time ran relative to the reference.
    pub fn backward_by(&self) -> Duration {
        let delta = self.reference_micros.saturating_sub(self.decision_micros);
        micros_to_duration(delta.max(0))
    }
}

impl From<ClockSkew> for IncidentError {
    fn from(skew: ClockSkew) -> Self {
        IncidentError::ClockSkew {
            reference_micros: skew.reference_micros,
            decision_micros: skew.decision_micros,
        }
    }
}

impl DurableTimestamp {
    /// Builds a timestamp from raw microseconds since the Unix epoch.
    ///
    /// This is the constructor a persistence adapter uses when mapping a
    /// `timestamptz` column, and the one a test uses to pin an exact
    /// instant. It performs no validation, because "is this instant
    /// plausible for this field" is an aggregate-level invariant that
    /// depends on the other fields — see `Incident::reconstitute`.
    pub const fn from_micros(micros_since_epoch: i64) -> Self {
        DurableTimestamp { micros_since_epoch }
    }

    /// The raw microseconds since the Unix epoch, for a persistence
    /// adapter writing a `timestamptz` column.
    pub const fn as_micros(&self) -> i64 {
        self.micros_since_epoch
    }

    /// Reads the wall half of `clock` as a durable timestamp.
    ///
    /// This is how the *in-memory* adapter sources a decision time. The
    /// PostgreSQL adapter will source it from `transaction_timestamp()`
    /// instead, so that one database — rather than several application
    /// hosts with independently drifting clocks — is the single authority
    /// on when a transition was decided (ADR 0031).
    pub fn now(clock: &dyn Clock) -> Result<Self, IncidentError> {
        Self::from_system_time(clock.wall())
    }

    /// Converts a host wall-clock reading, truncating sub-microsecond
    /// precision toward negative infinity.
    ///
    /// Errors only if the instant lies outside the range microseconds in
    /// an `i64` can address — about 292,000 years either side of the
    /// epoch, which no real host clock reaches and PostgreSQL could not
    /// store either.
    pub fn from_system_time(wall: SystemTime) -> Result<Self, IncidentError> {
        let micros = match wall.duration_since(UNIX_EPOCH) {
            Ok(after_epoch) => {
                i64::try_from(after_epoch.as_micros()).map_err(|_| out_of_range())?
            }
            Err(before_epoch) => {
                let magnitude = before_epoch.duration();
                let whole = i64::try_from(magnitude.as_micros()).map_err(|_| out_of_range())?;
                // `as_micros` truncates toward zero, which for a pre-epoch
                // instant rounds *up*. Push it back down so truncation is
                // uniformly toward negative infinity on both sides of the
                // epoch and ordering is never inverted by rounding.
                let rounds_up = magnitude.subsec_nanos() % NANOS_PER_MICRO != 0;
                whole
                    .checked_add(i64::from(rounds_up))
                    .and_then(i64::checked_neg)
                    .ok_or_else(out_of_range)?
            }
        };
        Ok(DurableTimestamp {
            micros_since_epoch: micros,
        })
    }

    /// The equivalent [`SystemTime`], for display and for interoperating
    /// with code that still speaks the standard library's clock types.
    ///
    /// Returns `None` only if the instant is unrepresentable on this
    /// platform's `SystemTime`.
    pub fn to_system_time(&self) -> Option<SystemTime> {
        let magnitude = micros_to_duration(self.micros_since_epoch.saturating_abs());
        if self.micros_since_epoch >= 0 {
            UNIX_EPOCH.checked_add(magnitude)
        } else {
            UNIX_EPOCH.checked_sub(magnitude)
        }
    }

    /// A timestamp `duration` after this one, or `None` on overflow.
    ///
    /// There is no unchecked `plus` counterpart on purpose: every
    /// duration this domain adds to a timestamp — a suppression length, a
    /// reopen window, a closure delay — originates from operator input or
    /// configuration, so overflow must be a rejected operation rather
    /// than a panic.
    pub fn checked_plus(&self, duration: Duration) -> Option<Self> {
        let added = i64::try_from(duration.as_micros()).ok()?;
        Some(DurableTimestamp {
            micros_since_epoch: self.micros_since_epoch.checked_add(added)?,
        })
    }

    /// How long elapsed from `earlier` to `self`, or [`ClockSkew`] if
    /// `earlier` is actually later.
    ///
    /// This replaces 5A's saturating `Timestamp::elapsed_since`. The
    /// saturating form was safe when the reference was a monotonic
    /// reading that could not run backward; against a wall clock that
    /// can, saturating to zero would silently answer "no time has passed"
    /// to a question the domain has no reliable answer for — which for a
    /// reopen decision is the difference between correlating an event to
    /// an existing incident and creating a duplicate. ADR 0031 requires
    /// the structured error instead: neither clamp, nor reopen, nor
    /// duplicate.
    pub fn checked_elapsed_since(&self, earlier: &DurableTimestamp) -> Result<Duration, ClockSkew> {
        if self.micros_since_epoch < earlier.micros_since_epoch {
            return Err(ClockSkew {
                reference_micros: earlier.micros_since_epoch,
                decision_micros: self.micros_since_epoch,
            });
        }
        Ok(micros_to_duration(
            self.micros_since_epoch - earlier.micros_since_epoch,
        ))
    }

    /// Whether `self` is at or before `deadline` — the exact inclusive
    /// comparison the reopen boundary needs (BQ-9). The boundary
    /// semantics are unchanged from 5A; only the representation being
    /// compared is durable rather than process-local.
    pub fn is_at_or_before(&self, deadline: &DurableTimestamp) -> bool {
        self.micros_since_epoch <= deadline.micros_since_epoch
    }

    /// Whether `self` is strictly before `deadline` — suppression expiry,
    /// where "still suppressed" must exclude the expiry instant itself.
    /// Unchanged from 5A in semantics.
    pub fn is_before(&self, deadline: &DurableTimestamp) -> bool {
        self.micros_since_epoch < deadline.micros_since_epoch
    }
}

impl Timestamp {
    /// This timestamp's durable half.
    ///
    /// The bridge used while lifecycle fields migrate off [`Timestamp`].
    /// It reads the *wall* half, which is the only half that has
    /// cross-process meaning; the monotonic half is deliberately dropped
    /// rather than approximated.
    pub fn to_durable(&self) -> Result<DurableTimestamp, IncidentError> {
        DurableTimestamp::from_system_time(self.wall())
    }
}

/// The single validation error this module can raise, kept in one place
/// so every out-of-range path reports identically.
fn out_of_range() -> IncidentError {
    IncidentError::ValidationError("timestamp is out of range".into())
}

/// Converts a non-negative microsecond count to a `Duration`, saturating
/// at a magnitude no real clock produces.
fn micros_to_duration(micros: i64) -> Duration {
    debug_assert!(micros >= 0, "callers must pass a non-negative magnitude");
    let micros = micros.max(0);
    Duration::new(
        (micros / MICROS_PER_SEC) as u64,
        (micros % MICROS_PER_SEC) as u32 * NANOS_PER_MICRO,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;

    #[test]
    fn epoch_is_zero_micros() {
        assert_eq!(
            DurableTimestamp::from_system_time(UNIX_EPOCH).unwrap(),
            DurableTimestamp::from_micros(0)
        );
    }

    #[test]
    fn a_post_epoch_instant_round_trips_through_system_time() {
        let original = DurableTimestamp::from_micros(1_756_000_000_123_456);
        let restored =
            DurableTimestamp::from_system_time(original.to_system_time().unwrap()).unwrap();
        assert_eq!(restored, original, "a round trip must be lossless");
    }

    #[test]
    fn a_pre_epoch_instant_round_trips_through_system_time() {
        let original = DurableTimestamp::from_micros(-1_756_000_000_123_456);
        let restored =
            DurableTimestamp::from_system_time(original.to_system_time().unwrap()).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn sub_microsecond_precision_truncates_toward_negative_infinity() {
        // 1s + 1500ns after the epoch truncates down to 1_000_001 micros.
        let after = UNIX_EPOCH + Duration::new(1, 1_500);
        assert_eq!(
            DurableTimestamp::from_system_time(after)
                .unwrap()
                .as_micros(),
            1_000_001
        );
        // The same magnitude *before* the epoch must also round down,
        // i.e. further from zero, so ordering is never inverted.
        let before = UNIX_EPOCH - Duration::new(1, 1_500);
        assert_eq!(
            DurableTimestamp::from_system_time(before)
                .unwrap()
                .as_micros(),
            -1_000_002
        );
    }

    #[test]
    fn truncation_preserves_ordering_across_the_epoch() {
        let before =
            DurableTimestamp::from_system_time(UNIX_EPOCH - Duration::new(1, 1_500)).unwrap();
        let at = DurableTimestamp::from_system_time(UNIX_EPOCH).unwrap();
        let after =
            DurableTimestamp::from_system_time(UNIX_EPOCH + Duration::new(1, 1_500)).unwrap();
        assert!(before < at && at < after);
    }

    #[test]
    fn elapsed_since_measures_forward_progress() {
        let start = DurableTimestamp::from_micros(1_000_000);
        let later = DurableTimestamp::from_micros(1_900_500);
        assert_eq!(
            later.checked_elapsed_since(&start).unwrap(),
            Duration::from_micros(900_500)
        );
    }

    #[test]
    fn elapsed_since_is_zero_for_the_same_instant() {
        let at = DurableTimestamp::from_micros(42);
        assert_eq!(at.checked_elapsed_since(&at).unwrap(), Duration::ZERO);
    }

    #[test]
    fn elapsed_since_reports_skew_instead_of_clamping_to_zero() {
        let reference = DurableTimestamp::from_micros(2_000_000);
        let decision = DurableTimestamp::from_micros(1_500_000);
        let skew = decision
            .checked_elapsed_since(&reference)
            .expect_err("a backward comparison must not silently succeed");
        assert_eq!(skew.reference_micros, 2_000_000);
        assert_eq!(skew.decision_micros, 1_500_000);
        assert_eq!(skew.backward_by(), Duration::from_micros(500_000));
    }

    #[test]
    fn a_clock_skew_converts_to_a_structured_domain_error() {
        let skew = ClockSkew {
            reference_micros: 10,
            decision_micros: 4,
        };
        let error: IncidentError = skew.into();
        assert_eq!(error.code(), "incident.clock_skew");
    }

    #[test]
    fn the_inclusive_boundary_is_true_at_exactly_the_deadline() {
        let start = DurableTimestamp::from_micros(0);
        let deadline = start.checked_plus(Duration::from_secs(900)).unwrap();
        assert!(
            deadline.is_at_or_before(&deadline),
            "exactly-at is inclusive"
        );
        let past = deadline.checked_plus(Duration::from_micros(1)).unwrap();
        assert!(!past.is_at_or_before(&deadline));
    }

    #[test]
    fn the_strict_boundary_is_false_at_exactly_the_deadline() {
        let deadline = DurableTimestamp::from_micros(900_000_000);
        assert!(
            !deadline.is_before(&deadline),
            "suppression must have expired at exactly its deadline"
        );
        assert!(DurableTimestamp::from_micros(899_999_999).is_before(&deadline));
    }

    #[test]
    fn checked_plus_rejects_an_overflowing_duration() {
        let at = DurableTimestamp::from_micros(i64::MAX - 5);
        assert_eq!(at.checked_plus(Duration::from_secs(1)), None);
        assert_eq!(at.checked_plus(Duration::MAX), None);
    }

    #[test]
    fn a_timestamp_exposes_its_durable_wall_half() {
        let clock = TestClock::new();
        let stamp = Timestamp::now(&clock);
        let durable = stamp.to_durable().unwrap();
        assert_eq!(
            durable,
            DurableTimestamp::from_system_time(stamp.wall()).unwrap()
        );
    }

    #[test]
    fn advancing_a_test_clock_advances_the_durable_reading() {
        let clock = TestClock::new();
        let start = DurableTimestamp::now(&clock).unwrap();
        clock.advance(Duration::from_secs(900));
        let later = DurableTimestamp::now(&clock).unwrap();
        assert_eq!(
            later.checked_elapsed_since(&start).unwrap(),
            Duration::from_secs(900),
            "the wall half must advance in step with the monotonic half"
        );
    }

    #[test]
    fn serialization_is_a_bare_integer() {
        let at = DurableTimestamp::from_micros(-1_234_567);
        let json = serde_json::to_string(&at).unwrap();
        assert_eq!(json, "-1234567");
        assert_eq!(
            serde_json::from_str::<DurableTimestamp>(&json).unwrap(),
            at,
            "a serialized timestamp must survive a round trip exactly"
        );
    }
}
