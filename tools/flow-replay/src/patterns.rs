//! Traffic shapes for exercising the detection engine.
//!
//! These describe *volume over time* and nothing else. Every record
//! flow-replay sends is the same synthetic, well-formed IPFIX record it
//! has always sent — no spoofed sources, no amplification payloads, no
//! reflection patterns, nothing resembling real attack traffic. What
//! changes is how many bits per second the synthetic stream carries, so
//! a detection policy can be seen to fire, hold, and clear. See
//! docs/security-principles.md.
//!
//! Each pattern exists to prove one property of the engine:
//!
//! | Pattern  | What it should prove                                     |
//! |----------|----------------------------------------------------------|
//! | `steady` | A policy at a sane threshold stays silent.               |
//! | `flood`  | A sustained crossing opens exactly one detection.        |
//! | `spike`  | A crossing shorter than `triggerFor` opens nothing.      |
//! | `flap`   | Hysteresis and `cooldown` stop one attack becoming ten events. |
//! | `ramp`   | A detection opens when the rate crosses, not when it starts rising. |

/// How the synthetic stream's volume varies over a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Steady,
    Flood,
    Spike,
    Flap,
    Ramp,
}

/// A tenth of peak. Chosen so `steady` sits well under a threshold set
/// at peak, with enough headroom that a `clearPercent` of 80 does not
/// accidentally put it in the hysteresis band.
const QUIET_DIVISOR: u64 = 10;

/// How long `spike` stays above the threshold. Deliberately short: the
/// point is that it is under any sensible `triggerFor`.
const SPIKE_SECONDS: u64 = 2;

/// When `spike` starts, so there is quiet traffic on either side of it.
const SPIKE_START: u64 = 5;

/// Half-period of `flap`, in seconds.
const FLAP_PERIOD: u64 = 6;

impl Pattern {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "steady" => Some(Pattern::Steady),
            "flood" => Some(Pattern::Flood),
            "spike" => Some(Pattern::Spike),
            "flap" => Some(Pattern::Flap),
            "ramp" => Some(Pattern::Ramp),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Pattern::Steady => "steady",
            Pattern::Flood => "flood",
            Pattern::Spike => "spike",
            Pattern::Flap => "flap",
            Pattern::Ramp => "ramp",
        }
    }

    /// What an operator should expect to see, printed before the run so
    /// a failed expectation is obvious without reading this file.
    pub fn expectation(&self) -> &'static str {
        match self {
            Pattern::Steady => "no detection: the rate never crosses the threshold",
            Pattern::Flood => {
                "exactly one detection, opening after triggerFor and closing after clearFor"
            }
            Pattern::Spike => "no detection: the crossing is shorter than any sane triggerFor",
            Pattern::Flap => {
                "one detection per crossing at most, with cooldown suppressing the rest"
            }
            Pattern::Ramp => {
                "one detection, opening when the rate crosses — not when it starts rising"
            }
        }
    }

    /// Bits per second this pattern wants at `second` into a run of
    /// `duration` seconds, given a peak of `peak_bps`.
    ///
    /// Saturating throughout: a caller passing an absurd peak gets an
    /// absurd but finite rate rather than a panic.
    pub fn bps_at(&self, second: u64, peak_bps: u64, duration: u64) -> u64 {
        let quiet = peak_bps / QUIET_DIVISOR;
        match self {
            Pattern::Steady => quiet,
            Pattern::Flood => peak_bps,
            Pattern::Spike => {
                if (SPIKE_START..SPIKE_START + SPIKE_SECONDS).contains(&second) {
                    peak_bps
                } else {
                    quiet
                }
            }
            Pattern::Flap => {
                if (second / FLAP_PERIOD).is_multiple_of(2) {
                    peak_bps
                } else {
                    quiet
                }
            }
            Pattern::Ramp => {
                if duration <= 1 {
                    return peak_bps;
                }
                let span = peak_bps.saturating_sub(quiet);
                let step = (span as u128 * second.min(duration - 1) as u128)
                    / (duration - 1).max(1) as u128;
                quiet.saturating_add(step.min(u64::MAX as u128) as u64)
            }
        }
    }
}

/// One record to send: how many bytes and packets it should claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPlan {
    pub bytes: u64,
    pub packets: u64,
}

/// Splits one second's bit budget across `records` records.
///
/// Returns at least one record with at least one byte, so a pattern
/// never produces a record the collector would reject as empty.
pub fn plan_second(bps: u64, records: u32) -> Vec<RecordPlan> {
    let records = records.max(1);
    let total_bytes = (bps / 8).max(1);
    let per_record = (total_bytes / records as u64).max(1);
    // A packet every 1500 bytes is a plausible full-MTU stream. One
    // packet minimum, for the same reason as one byte minimum.
    let per_packets = (per_record / 1500).max(1);
    (0..records)
        .map(|_| RecordPlan {
            bytes: per_record,
            packets: per_packets,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEAK: u64 = 10_000_000;
    const DURATION: u64 = 30;

    #[test]
    fn every_name_round_trips() {
        for pattern in [
            Pattern::Steady,
            Pattern::Flood,
            Pattern::Spike,
            Pattern::Flap,
            Pattern::Ramp,
        ] {
            assert_eq!(Pattern::parse(pattern.as_str()), Some(pattern));
            assert!(!pattern.expectation().is_empty());
        }
        assert_eq!(Pattern::parse("nonsense"), None);
    }

    #[test]
    fn steady_never_reaches_the_peak() {
        for second in 0..DURATION {
            assert_eq!(Pattern::Steady.bps_at(second, PEAK, DURATION), PEAK / 10);
        }
    }

    #[test]
    fn flood_holds_the_peak_for_the_whole_run() {
        for second in 0..DURATION {
            assert_eq!(Pattern::Flood.bps_at(second, PEAK, DURATION), PEAK);
        }
    }

    #[test]
    fn spike_is_above_the_threshold_for_only_two_seconds() {
        let above: Vec<u64> = (0..DURATION)
            .filter(|s| Pattern::Spike.bps_at(*s, PEAK, DURATION) == PEAK)
            .collect();
        assert_eq!(above, vec![5, 6]);
    }

    #[test]
    fn flap_alternates_on_a_fixed_period() {
        assert_eq!(Pattern::Flap.bps_at(0, PEAK, DURATION), PEAK);
        assert_eq!(Pattern::Flap.bps_at(5, PEAK, DURATION), PEAK);
        assert_eq!(Pattern::Flap.bps_at(6, PEAK, DURATION), PEAK / 10);
        assert_eq!(Pattern::Flap.bps_at(11, PEAK, DURATION), PEAK / 10);
        assert_eq!(Pattern::Flap.bps_at(12, PEAK, DURATION), PEAK);
    }

    #[test]
    fn ramp_rises_monotonically_from_quiet_to_peak() {
        let mut previous = 0;
        for second in 0..DURATION {
            let bps = Pattern::Ramp.bps_at(second, PEAK, DURATION);
            assert!(bps >= previous, "ramp must never fall, at second {second}");
            previous = bps;
        }
        assert_eq!(Pattern::Ramp.bps_at(0, PEAK, DURATION), PEAK / 10);
        assert_eq!(Pattern::Ramp.bps_at(DURATION - 1, PEAK, DURATION), PEAK);
    }

    #[test]
    fn ramp_handles_a_one_second_run_without_dividing_by_zero() {
        assert_eq!(Pattern::Ramp.bps_at(0, PEAK, 1), PEAK);
        assert_eq!(Pattern::Ramp.bps_at(0, PEAK, 0), PEAK);
    }

    #[test]
    fn an_absurd_peak_saturates_rather_than_panicking() {
        for pattern in [
            Pattern::Steady,
            Pattern::Flood,
            Pattern::Spike,
            Pattern::Flap,
            Pattern::Ramp,
        ] {
            let bps = pattern.bps_at(0, u64::MAX, DURATION);
            assert!(bps > 0, "{} produced nothing", pattern.as_str());
        }
    }

    #[test]
    fn a_second_is_split_across_records() {
        let plans = plan_second(80_000_000, 10);
        assert_eq!(plans.len(), 10);
        assert_eq!(plans[0].bytes, 1_000_000);
        assert!(plans.iter().all(|p| p.packets > 0));
        let total: u64 = plans.iter().map(|p| p.bytes).sum();
        assert_eq!(total, 10_000_000);
    }

    #[test]
    fn a_plan_never_produces_an_empty_record() {
        let plans = plan_second(0, 10);
        assert_eq!(plans.len(), 10);
        assert!(plans.iter().all(|p| p.bytes >= 1 && p.packets >= 1));
        let plans = plan_second(8, 0);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].bytes >= 1);
    }
}
