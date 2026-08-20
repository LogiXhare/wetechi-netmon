//! Rate-window calculation (Phase 3 objective 7): 1s, 5s, 15s, 1m, 5m.
//!
//! **Design choice, documented rather than left implicit:** windows are
//! tumbling and keyed on *processing time* (when the aggregator received
//! the flow, `Instant::now()`), not on the flow's own `start_time`/
//! `end_time`. This one choice resolves all four concerns Phase 3
//! objective 7 calls out:
//!
//! - **Exporter clock skew**: irrelevant — we never trust the exporter's
//!   clock for windowing, only our own.
//! - **Missing timestamps**: irrelevant for the same reason —
//!   `NormalizedFlow::start_time`/`end_time` are never read here.
//! - **Late records**: a flow that describes traffic from the past is
//!   still counted in whichever window it *arrives* in — there is no
//!   out-of-order/watermarking logic to get wrong, at the cost of rate
//!   figures reflecting arrival pattern rather than true historical
//!   timing for delayed exporters.
//! - **Long-duration flows**: a flow spanning many seconds/minutes has
//!   its entire byte/packet count attributed to the single window it was
//!   received in, not spread proportionally across its duration. This is
//!   a known simplification — proportional attribution needs to slice
//!   one flow's counters across multiple windows, which is meaningfully
//!   more complex and not required by this phase's acceptance criteria.
//!
//! This trade-off must be revisited if later phases need
//! timing-accurate (not just arrival-accurate) rate figures — tracked in
//! docs/architecture/aggregation.md, not silently assumed sufficient
//! forever.

use std::time::{Duration, Instant};

/// The five window durations Phase 3 requires.
pub const WINDOW_DURATIONS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(300),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateSample {
    pub bps: f64,
    pub pps: f64,
    pub fps: f64,
}

struct Window {
    duration: Duration,
    window_start: Instant,
    bytes: u64,
    packets: u64,
    flows: u64,
    /// The most recently *finalized* (fully elapsed) window's rate. This
    /// is what callers read — the in-progress window's partial counts
    /// are not exposed, since a partial window's rate is misleadingly
    /// noisy (e.g. a 1s window read after 100ms looks like a 10x spike).
    last_finalized: Option<RateSample>,
}

impl Window {
    fn new(duration: Duration, now: Instant) -> Self {
        Window {
            duration,
            window_start: now,
            bytes: 0,
            packets: 0,
            flows: 0,
            last_finalized: None,
        }
    }

    fn record(&mut self, now: Instant, bytes: u64, packets: u64) {
        self.roll_if_elapsed(now);
        self.bytes = self.bytes.saturating_add(bytes);
        self.packets = self.packets.saturating_add(packets);
        self.flows = self.flows.saturating_add(1);
    }

    fn roll_if_elapsed(&mut self, now: Instant) {
        while now.duration_since(self.window_start) >= self.duration {
            let secs = self.duration.as_secs_f64();
            self.last_finalized = Some(RateSample {
                bps: (self.bytes as f64 * 8.0) / secs,
                pps: self.packets as f64 / secs,
                fps: self.flows as f64 / secs,
            });
            self.window_start += self.duration;
            self.bytes = 0;
            self.packets = 0;
            self.flows = 0;
        }
    }
}

/// The full set of rate windows (1s/5s/15s/1m/5m) for one aggregation
/// scope (e.g. total traffic).
pub struct RateWindowSet {
    windows: Vec<Window>,
}

impl RateWindowSet {
    pub fn new(now: Instant) -> Self {
        RateWindowSet {
            windows: WINDOW_DURATIONS
                .iter()
                .map(|&d| Window::new(d, now))
                .collect(),
        }
    }

    pub fn record(&mut self, now: Instant, bytes: u64, packets: u64) {
        for window in &mut self.windows {
            window.record(now, bytes, packets);
        }
    }

    /// Forces any windows that have elapsed to finalize, without
    /// recording new traffic — call periodically so rates go to zero
    /// during genuinely idle periods instead of showing stale non-zero
    /// values forever.
    pub fn tick(&mut self, now: Instant) {
        for window in &mut self.windows {
            window.roll_if_elapsed(now);
        }
    }

    /// Returns `(duration, last_finalized_rate)` for each of the five
    /// windows, in the same order as [`WINDOW_DURATIONS`].
    pub fn rates(&self) -> Vec<(Duration, Option<RateSample>)> {
        self.windows
            .iter()
            .map(|w| (w.duration, w.last_finalized))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rate_until_a_window_has_fully_elapsed() {
        let t0 = Instant::now();
        let mut set = RateWindowSet::new(t0);
        set.record(t0, 100, 1);
        let rates = set.rates();
        assert!(rates.iter().all(|(_, r)| r.is_none()));
    }

    #[test]
    fn finalizes_a_1s_window_and_reports_its_rate() {
        let t0 = Instant::now();
        let mut set = RateWindowSet::new(t0);
        set.record(t0, 1000, 10); // 1000 bytes, 10 packets in the 1s window
        set.record(t0 + Duration::from_millis(1100), 0, 0); // triggers roll-over

        let rates = set.rates();
        let (dur, sample) = rates[0]; // 1s window
        assert_eq!(dur, Duration::from_secs(1));
        let sample = sample.expect("1s window should have finalized");
        assert!((sample.bps - 8000.0).abs() < 0.01); // 1000 bytes * 8 bits / 1s
        assert!((sample.pps - 10.0).abs() < 0.01);
    }

    #[test]
    fn tick_finalizes_elapsed_windows_even_without_new_traffic() {
        let t0 = Instant::now();
        let mut set = RateWindowSet::new(t0);
        set.record(t0, 500, 5);
        set.tick(t0 + Duration::from_secs(2));
        let (_, sample) = set.rates()[0];
        assert!(sample.is_some());
    }

    #[test]
    fn idle_period_after_traffic_reports_zero_not_stale_value() {
        let t0 = Instant::now();
        let mut set = RateWindowSet::new(t0);
        set.record(t0, 1000, 10);
        set.tick(t0 + Duration::from_millis(1100)); // finalize window with traffic
        let (_, first) = set.rates()[0];
        assert!(first.unwrap().bps > 0.0);

        set.tick(t0 + Duration::from_millis(2200)); // finalize a second, idle window
        let (_, second) = set.rates()[0];
        assert_eq!(second.unwrap().bps, 0.0);
    }

    #[test]
    fn all_five_documented_durations_are_present() {
        let set = RateWindowSet::new(Instant::now());
        let durations: Vec<Duration> = set.rates().into_iter().map(|(d, _)| d).collect();
        assert_eq!(
            durations,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(5),
                Duration::from_secs(15),
                Duration::from_secs(60),
                Duration::from_secs(300),
            ]
        );
    }
}
