//! GCRA rate limiting in memory (ADR 0038, gate 6).
//!
//! The Generic Cell Rate Algorithm, the token-bucket equivalent ATM and
//! most API gateways use. Its state is one *theoretical arrival time*
//! (TAT) per key:
//!
//! - `T`, the emission interval, is `window / limit`.
//! - `tau`, the tolerance, is `T × (burst − 1)`, so up to `burst` requests
//!   may arrive at once.
//! - A request at `now` is allowed when `now ≥ TAT − tau`; then
//!   `TAT = max(TAT, now) + T`. Otherwise it is refused, and may retry at
//!   `TAT − tau`.
//!
//! Implemented here on `std` alone: the gate-8 probe rejected `governor`
//! for adding ~920 `unsafe` occurrences for this (see the licence matrix,
//! row 40).
//!
//! **Bounded memory.** At most `max_keys` keys are held. When full, keys
//! whose TAT has passed (indistinguishable from a fresh key) are swept; if
//! none can be, a new key is refused rather than any state evicted, which
//! fails closed under a flood of distinct keys.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// `limit` requests per `window`, with bursts of up to `limit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub limit: u32,
    pub window: Duration,
}

impl Quota {
    pub const fn per_minute(limit: u32) -> Self {
        Quota {
            limit,
            window: Duration::from_secs(60),
        }
    }

    pub const fn per_hour(limit: u32) -> Self {
        Quota {
            limit,
            window: Duration::from_secs(3_600),
        }
    }

    fn emission_interval(&self) -> Duration {
        self.window / self.limit.max(1)
    }

    fn tolerance(&self) -> Duration {
        self.emission_interval() * self.limit.max(1).saturating_sub(1)
    }
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Over the quota; may retry after this long.
    RetryAfter(Duration),
    /// The key table is full of live keys.
    Saturated,
}

impl Refusal {
    /// Whole seconds for `Retry-After`, rounded up, at least 1.
    pub fn retry_after_secs(&self) -> u64 {
        match self {
            Refusal::RetryAfter(wait) => {
                let secs = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
                secs.max(1)
            }
            Refusal::Saturated => 1,
        }
    }
}

#[derive(Debug)]
pub struct RateLimiter<K> {
    quota: Quota,
    max_keys: usize,
    tats: Mutex<HashMap<K, Instant>>,
}

impl<K: Eq + Hash + Clone> RateLimiter<K> {
    pub fn new(quota: Quota, max_keys: usize) -> Self {
        RateLimiter {
            quota,
            max_keys: max_keys.max(1),
            tats: Mutex::new(HashMap::new()),
        }
    }

    pub fn quota(&self) -> Quota {
        self.quota
    }

    /// Counts one request for `key` at `now`, or refuses it.
    pub fn check(&self, key: &K, now: Instant) -> Result<(), Refusal> {
        let interval = self.quota.emission_interval();
        let tolerance = self.quota.tolerance();
        let mut tats = self.locked();
        if !tats.contains_key(key) && tats.len() >= self.max_keys {
            tats.retain(|_, tat| *tat > now);
            if tats.len() >= self.max_keys {
                return Err(Refusal::Saturated);
            }
        }
        let tat = tats.get(key).copied().unwrap_or(now).max(now);
        if let Some(allow_at) = tat.checked_sub(tolerance) {
            if now < allow_at {
                return Err(Refusal::RetryAfter(allow_at - now));
            }
        }
        tats.insert(key.clone(), tat + interval);
        Ok(())
    }

    /// Keys currently held, for tests and the sweep.
    pub fn tracked(&self) -> usize {
        self.locked().len()
    }

    /// A poisoned lock means a panic mid-update; the map holds only
    /// instants, so recovering is safe.
    fn locked(&self) -> MutexGuard<'_, HashMap<K, Instant>> {
        self.tats.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn limiter(limit: u32, window_secs: u64) -> RateLimiter<&'static str> {
        RateLimiter::new(
            Quota {
                limit,
                window: Duration::from_secs(window_secs),
            },
            100,
        )
    }

    #[test]
    fn a_full_burst_is_allowed_then_the_next_waits_one_interval() {
        let limiter = limiter(60, 60);
        let start = Instant::now();
        for _ in 0..60 {
            assert!(limiter.check(&"a", start).is_ok());
        }
        assert_eq!(
            limiter.check(&"a", start),
            Err(Refusal::RetryAfter(Duration::from_secs(1)))
        );
        assert!(limiter.check(&"a", start + Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn keys_are_independent() {
        let limiter = limiter(1, 60);
        let now = Instant::now();
        assert!(limiter.check(&"a", now).is_ok());
        assert!(limiter.check(&"a", now).is_err());
        assert!(limiter.check(&"b", now).is_ok());
    }

    #[test]
    fn a_full_table_sweeps_spent_keys_and_refuses_when_all_are_live() {
        let limiter = RateLimiter::new(Quota::per_minute(1), 2);
        let now = Instant::now();
        assert!(limiter.check(&"a", now).is_ok());
        assert!(limiter.check(&"b", now).is_ok());
        assert_eq!(limiter.check(&"c", now), Err(Refusal::Saturated));
        // A minute later both keys are spent and swept for the newcomer.
        let later = now + Duration::from_secs(61);
        assert!(limiter.check(&"c", later).is_ok());
        assert_eq!(limiter.tracked(), 1);
    }

    #[test]
    fn retry_after_rounds_up_to_whole_seconds() {
        assert_eq!(
            Refusal::RetryAfter(Duration::from_millis(1_200)).retry_after_secs(),
            2
        );
        assert_eq!(
            Refusal::RetryAfter(Duration::from_millis(10)).retry_after_secs(),
            1
        );
        assert_eq!(Refusal::Saturated.retry_after_secs(), 1);
    }

    proptest! {
        /// Over any sequence of arrivals, the requests allowed never exceed
        /// the burst plus one per emission interval elapsed — the GCRA
        /// conformance bound.
        #[test]
        fn never_allows_more_than_burst_plus_elapsed_intervals(
            limit in 1u32..20,
            gaps in proptest::collection::vec(0u64..3_000, 1..300),
        ) {
            let quota = Quota { limit, window: Duration::from_secs(10) };
            let limiter = RateLimiter::new(quota, 10);
            let start = Instant::now();
            let mut now = start;
            let mut allowed = 0u64;
            for gap in gaps {
                now += Duration::from_millis(gap);
                if limiter.check(&"k", now).is_ok() {
                    allowed += 1;
                }
                let interval = quota.emission_interval().as_nanos();
                let elapsed = (now - start).as_nanos();
                let bound = u64::from(limit) + (elapsed / interval) as u64;
                prop_assert!(allowed <= bound, "allowed {allowed} > bound {bound}");
            }
        }

        /// A refused request told to wait is allowed after waiting exactly
        /// that long.
        #[test]
        fn waiting_the_advised_time_is_enough(limit in 1u32..20, extra in 0u32..5) {
            let limiter = RateLimiter::new(Quota::per_minute(limit), 10);
            let now = Instant::now();
            for _ in 0..(limit + extra) {
                let _ = limiter.check(&"k", now);
            }
            if let Err(Refusal::RetryAfter(wait)) = limiter.check(&"k", now) {
                prop_assert!(limiter.check(&"k", now + wait).is_ok());
            }
        }
    }
}
