//! Comparing one snapshot against one policy.
//!
//! Two rules govern everything here.
//!
//! **Every matching threshold is reported, not just the first.** An
//! operator looking at an event needs to know whether it fired because
//! bandwidth alone crossed a line, or because bandwidth *and* packet rate
//! *and* SYN rate all did at once — those are different attacks. Stopping
//! at the first match would discard exactly the information that
//! distinguishes them.
//!
//! **A metric whose source data was never present is skipped, not
//! treated as zero.** If an exporter sends no forwarding-status field,
//! the dropped-packet rate is unknown, not zero. Comparing unknown
//! against a threshold and concluding "below" is how a detector reports
//! all-clear on a link it cannot actually see.

use serde::{Deserialize, Serialize};

use crate::input::{DetectionSnapshot, MetricKind};
use crate::policy::DetectionPolicy;

/// One threshold that a snapshot crossed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedReason {
    pub metric: MetricKind,
    /// Observed rate in canonical units.
    pub observed: u64,
    /// The policy's trigger threshold in canonical units.
    pub threshold: u64,
    /// `observed - threshold`. Saturating, though it cannot underflow
    /// here because a reason is only built when `observed >= threshold`.
    pub excess: u64,
    /// `observed * 100 / threshold`, so 250 means two and a half times
    /// the threshold. Integer, for the same reason the comparison is:
    /// an operator should be able to reproduce this number by hand.
    pub ratio_percent: u64,
}

impl MatchedReason {
    fn new(metric: MetricKind, observed: u64, threshold: u64) -> Self {
        MatchedReason {
            metric,
            observed,
            threshold,
            excess: observed.saturating_sub(threshold),
            ratio_percent: ratio_percent(observed, threshold),
        }
    }
}

/// Why a configured threshold was not evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedMetric {
    pub metric: MetricKind,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// The upstream protocol never carried the field this metric derives
    /// from, so its value is unknown rather than zero.
    SourceFieldAbsent,
}

/// The result of comparing one snapshot against one policy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Evaluation {
    /// Thresholds crossed, ordered deterministically by metric.
    pub matched: Vec<MatchedReason>,
    /// Thresholds that could not be evaluated.
    pub skipped: Vec<SkippedMetric>,
    /// True when every evaluable threshold is at or below its clear
    /// level. Distinct from `matched.is_empty()`: with hysteresis there
    /// is a band between the clear and trigger thresholds where neither
    /// is true, and traffic sitting in that band must neither open a new
    /// detection nor begin clearing an existing one.
    pub all_below_clear: bool,
    /// How many thresholds were actually evaluated.
    pub evaluated: usize,
}

impl Evaluation {
    pub fn is_over_threshold(&self) -> bool {
        !self.matched.is_empty()
    }
}

/// `observed * 100 / threshold`, saturating rather than overflowing.
///
/// `u128` intermediate: an observed rate near `u64::MAX` multiplied by
/// 100 overflows `u64` easily, and a detector that panics on absurd
/// input is a detector an attacker can turn off.
fn ratio_percent(observed: u64, threshold: u64) -> u64 {
    if threshold == 0 {
        // Validation rejects zero thresholds, so this is unreachable via
        // a validated policy. Returning a saturated ratio rather than
        // dividing by zero keeps it unreachable *and* harmless.
        return u64::MAX;
    }
    let scaled = (observed as u128) * 100 / (threshold as u128);
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// Compares `snapshot` against `policy`.
///
/// The caller is responsible for having established that the policy
/// applies to the snapshot's scope — see [`crate::precedence`].
pub fn evaluate(policy: &DetectionPolicy, snapshot: &DetectionSnapshot) -> Evaluation {
    let mut matched = Vec::new();
    let mut skipped = Vec::new();
    let mut evaluated = 0usize;
    let mut all_below_clear = true;

    // `Thresholds` is a BTreeMap, so this iteration order is stable
    // across runs and processes — an event's reason list must be
    // diffable between restarts.
    for (metric, threshold) in policy.thresholds.iter() {
        if let Some(flag) = metric.required_completeness() {
            if !snapshot.completeness.has(flag) {
                skipped.push(SkippedMetric {
                    metric,
                    reason: SkipReason::SourceFieldAbsent,
                });
                continue;
            }
        }

        evaluated += 1;
        let observed = snapshot.rates.get(metric);

        if observed >= threshold {
            matched.push(MatchedReason::new(metric, observed, threshold));
        }

        let clear = policy.clear_percent.clear_threshold(threshold);
        if observed > clear {
            all_below_clear = false;
        }
    }

    // Nothing evaluable means nothing can be concluded. Reporting
    // "all below clear" here would let an active detection clear itself
    // the moment an exporter stopped sending the field it was detected
    // on, which is the opposite of safe.
    if evaluated == 0 {
        all_below_clear = false;
    }

    Evaluation {
        matched,
        skipped,
        all_below_clear,
        evaluated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{
        AddressFamily, DataCompleteness, MetricRates, SamplingStatus, ScopeId, ScopeKey, ScopeType,
        TrafficDirection,
    };
    use crate::policy::{
        DetectionPolicy, ExecutionMode, PolicyDraft, PolicySelector, Severity, TenantPrefixes,
        Thresholds, DEFAULT_CLEAR_PERCENT,
    };
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant, SystemTime};

    fn policy_with(thresholds: Thresholds) -> DetectionPolicy {
        PolicyDraft {
            id: "p1".to_string(),
            name: "P1".to_string(),
            description: None,
            enabled: true,
            tenant: "t".to_string(),
            scope_type: ScopeType::Host,
            selector: PolicySelector::Any,
            address_family: None,
            direction: TrafficDirection::Incoming,
            window: Duration::from_secs(15),
            thresholds,
            clear_percent: DEFAULT_CLEAR_PERCENT,
            trigger_for: Duration::from_secs(15),
            clear_for: Duration::from_secs(30),
            cooldown: Duration::from_secs(60),
            hold_down: Duration::from_secs(30),
            event_update_interval: Duration::from_secs(60),
            severity: Severity::Major,
            execution_mode: ExecutionMode::AlertOnly,
            priority: 0,
            labels: BTreeMap::new(),
            version: 1,
        }
        .validate(&TenantPrefixes::new())
        .unwrap()
    }

    fn snapshot(rates: MetricRates, completeness: DataCompleteness) -> DetectionSnapshot {
        DetectionSnapshot {
            key: ScopeKey {
                tenant: "t".to_string(),
                scope_type: ScopeType::Host,
                scope_id: ScopeId::Host {
                    addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                },
                direction: TrafficDirection::Incoming,
                address_family: AddressFamily::Ipv4,
            },
            window: Duration::from_secs(15),
            observed_at: Instant::now(),
            observed_wall: SystemTime::UNIX_EPOCH,
            rates,
            completeness,
            sampling: SamplingStatus::default(),
            flows_observed: 10,
            exporters_observed: 1,
        }
    }

    #[test]
    fn below_threshold_does_not_match() {
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1000));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 999,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert!(!e.is_over_threshold());
        assert_eq!(e.evaluated, 1);
    }

    #[test]
    fn exactly_at_the_threshold_matches() {
        // The comparison is inclusive, and this is the boundary an
        // operator will actually ask about.
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1000));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 1000,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert!(e.is_over_threshold());
        assert_eq!(e.matched[0].excess, 0);
        assert_eq!(e.matched[0].ratio_percent, 100);
    }

    #[test]
    fn above_the_threshold_records_excess_and_ratio() {
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1000));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 2500,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert_eq!(e.matched[0].observed, 2500);
        assert_eq!(e.matched[0].excess, 1500);
        assert_eq!(e.matched[0].ratio_percent, 250);
    }

    #[test]
    fn every_matching_threshold_is_reported_not_only_the_first() {
        let p = policy_with(
            Thresholds::new()
                .with(MetricKind::Bps, 1000)
                .with(MetricKind::Pps, 100)
                .with(MetricKind::Fps, 10),
        );
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 5000,
                    pps: 500,
                    fps: 50,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert_eq!(e.matched.len(), 3);
        let metrics: Vec<MetricKind> = e.matched.iter().map(|m| m.metric).collect();
        assert!(metrics.contains(&MetricKind::Bps));
        assert!(metrics.contains(&MetricKind::Pps));
        assert!(metrics.contains(&MetricKind::Fps));
    }

    #[test]
    fn matched_reason_order_is_stable_across_evaluations() {
        let p = policy_with(
            Thresholds::new()
                .with(MetricKind::Fps, 10)
                .with(MetricKind::Bps, 1000)
                .with(MetricKind::Pps, 100),
        );
        let snap = snapshot(
            MetricRates {
                bps: 5000,
                pps: 500,
                fps: 50,
                ..Default::default()
            },
            DataCompleteness::default(),
        );
        let first: Vec<MetricKind> = evaluate(&p, &snap)
            .matched
            .iter()
            .map(|m| m.metric)
            .collect();
        for _ in 0..20 {
            let again: Vec<MetricKind> = evaluate(&p, &snap)
                .matched
                .iter()
                .map(|m| m.metric)
                .collect();
            assert_eq!(first, again);
        }
    }

    #[test]
    fn only_one_of_several_thresholds_matching_still_triggers() {
        let p = policy_with(
            Thresholds::new()
                .with(MetricKind::Bps, 1_000_000)
                .with(MetricKind::Pps, 100),
        );
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 10,
                    pps: 5000,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert_eq!(e.matched.len(), 1);
        assert_eq!(e.matched[0].metric, MetricKind::Pps);
    }

    #[test]
    fn a_metric_whose_source_field_was_never_present_is_skipped_not_zeroed() {
        let p = policy_with(Thresholds::new().with(MetricKind::DroppedPps, 1));
        let e = evaluate(
            &p,
            &snapshot(MetricRates::default(), DataCompleteness::default()),
        );
        assert!(e.matched.is_empty());
        assert_eq!(e.evaluated, 0);
        assert_eq!(
            e.skipped,
            vec![SkippedMetric {
                metric: MetricKind::DroppedPps,
                reason: SkipReason::SourceFieldAbsent
            }]
        );
        // Crucially: not "all below clear". Nothing was measurable, so
        // nothing may clear on this evidence.
        assert!(!e.all_below_clear);
    }

    #[test]
    fn a_metric_becomes_evaluable_once_its_source_field_appears() {
        let p = policy_with(Thresholds::new().with(MetricKind::DroppedPps, 100));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    dropped_pps: 500,
                    ..Default::default()
                },
                DataCompleteness {
                    forwarding_status_seen: true,
                    ..Default::default()
                },
            ),
        );
        assert_eq!(e.evaluated, 1);
        assert_eq!(e.matched.len(), 1);
    }

    #[test]
    fn protocol_thresholds_need_the_protocol_field() {
        let p = policy_with(Thresholds::new().with(MetricKind::UdpPps, 100));
        let without = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    udp_pps: 9999,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert!(without.matched.is_empty());

        let with = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    udp_pps: 9999,
                    ..Default::default()
                },
                DataCompleteness {
                    protocol_seen: true,
                    ..Default::default()
                },
            ),
        );
        assert_eq!(with.matched.len(), 1);
    }

    #[test]
    fn hysteresis_band_is_neither_matching_nor_clear() {
        // Trigger 1000, clear at 80% = 800. An observation of 900 sits
        // in the band: it must not open a detection, and must not let an
        // open one start clearing.
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1000));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 900,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert!(!e.is_over_threshold());
        assert!(!e.all_below_clear);
    }

    #[test]
    fn exactly_at_the_clear_threshold_counts_as_clear() {
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1000));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 800,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert!(e.all_below_clear);
    }

    #[test]
    fn one_metric_still_above_clear_blocks_clearing_of_the_whole_policy() {
        let p = policy_with(
            Thresholds::new()
                .with(MetricKind::Bps, 1000)
                .with(MetricKind::Pps, 100),
        );
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: 10,
                    pps: 95,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert!(!e.is_over_threshold());
        assert!(!e.all_below_clear, "pps 95 is above its clear level of 80");
    }

    #[test]
    fn zero_observed_traffic_is_below_clear() {
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1000));
        let e = evaluate(
            &p,
            &snapshot(MetricRates::default(), DataCompleteness::default()),
        );
        assert!(e.all_below_clear);
    }

    #[test]
    fn integer_maximum_observed_does_not_overflow() {
        let p = policy_with(Thresholds::new().with(MetricKind::Bps, 1));
        let e = evaluate(
            &p,
            &snapshot(
                MetricRates {
                    bps: u64::MAX,
                    ..Default::default()
                },
                DataCompleteness::default(),
            ),
        );
        assert_eq!(e.matched[0].excess, u64::MAX - 1);
        assert_eq!(e.matched[0].ratio_percent, u64::MAX);
    }

    #[test]
    fn ratio_percent_never_divides_by_zero() {
        assert_eq!(ratio_percent(100, 0), u64::MAX);
        assert_eq!(ratio_percent(0, 100), 0);
    }
}
