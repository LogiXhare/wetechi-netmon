//! Deciding which policy governs a scope when several could.
//!
//! Exactly one policy wins per scope key, and the same input always
//! produces the same winner. Both halves matter. If two policies could
//! both fire on one host, an operator silencing the wrong one would be
//! left wondering why the pages continued; and if the winner depended on
//! hash-map iteration order, the answer to "which policy fired?" would
//! change between restarts of the same binary on the same config.
//!
//! The ladder, most specific first, is documented in ADR 0009:
//!
//! 1. an explicit host policy
//! 2. the longest matching prefix policy
//! 3. a hostgroup policy
//! 4. the tenant's default (`Any` selector, named tenant)
//! 5. the global default (`Any` selector, wildcard tenant)
//!
//! Ties are broken by explicit priority, then by policy id. Policy id is
//! the last resort precisely because it is arbitrary but stable — an
//! arbitrary-but-stable answer is far better than a non-deterministic
//! one, and the diagnostics say when it was used so an operator can add
//! a priority instead.

use std::collections::BTreeMap;

use crate::input::ScopeKey;
use crate::policy::{DetectionPolicy, PolicyError};

/// Why one policy beat the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecedenceReason {
    /// Nothing else applied.
    OnlyCandidate,
    /// It targeted the scope more specifically.
    MoreSpecific,
    /// Equally specific, but carried a higher explicit priority.
    HigherPriority,
    /// Equally specific and equally prioritised — decided by policy id.
    /// Worth surfacing: it means the configuration is ambiguous and the
    /// operator probably meant to set a priority.
    LowestId,
}

impl PrecedenceReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrecedenceReason::OnlyCandidate => "onlyCandidate",
            PrecedenceReason::MoreSpecific => "moreSpecific",
            PrecedenceReason::HigherPriority => "higherPriority",
            PrecedenceReason::LowestId => "lowestId",
        }
    }
}

/// One policy that could have governed a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub policy_id: String,
    pub specificity: u32,
    pub priority: i32,
    pub selected: bool,
}

/// The outcome of choosing a policy for one scope, including the losers.
///
/// Keeping the candidates is what makes "why did this host page me under
/// that policy?" answerable without re-deriving the whole config by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub winner: Option<String>,
    pub reason: Option<PrecedenceReason>,
    pub candidates: Vec<Candidate>,
}

impl Selection {
    pub fn none() -> Self {
        Selection {
            winner: None,
            reason: None,
            candidates: Vec::new(),
        }
    }
}

/// A validated, duplicate-free set of policies.
///
/// A `BTreeMap` keyed by policy id: iteration is ordered, so selection
/// cannot depend on insertion or hash order.
#[derive(Debug, Clone, Default)]
pub struct PolicySet {
    policies: BTreeMap<String, DetectionPolicy>,
}

impl PolicySet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a set, rejecting duplicate ids.
    pub fn from_policies(
        policies: impl IntoIterator<Item = DetectionPolicy>,
    ) -> Result<Self, PolicyError> {
        let mut set = PolicySet::new();
        for policy in policies {
            if set.policies.contains_key(&policy.id) {
                return Err(PolicyError::DuplicateId { id: policy.id });
            }
            set.policies.insert(policy.id.clone(), policy);
        }
        Ok(set)
    }

    pub fn len(&self) -> usize {
        self.policies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&DetectionPolicy> {
        self.policies.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &DetectionPolicy> {
        self.policies.values()
    }

    /// Chooses the governing policy for `key`, with full diagnostics.
    pub fn select(&self, key: &ScopeKey) -> Selection {
        let mut candidates: Vec<&DetectionPolicy> = self
            .policies
            .values()
            .filter(|p| p.matches_scope(key))
            .collect();

        if candidates.is_empty() {
            return Selection::none();
        }

        // Sort best-first. Every comparison is total and every field is
        // deterministic, so this ordering is reproducible.
        candidates.sort_by(|a, b| {
            b.specificity()
                .cmp(&a.specificity())
                .then_with(|| b.priority.cmp(&a.priority))
                .then_with(|| a.id.cmp(&b.id))
        });

        let winner = candidates[0];
        let reason = if candidates.len() == 1 {
            PrecedenceReason::OnlyCandidate
        } else {
            let runner_up = candidates[1];
            if winner.specificity() != runner_up.specificity() {
                PrecedenceReason::MoreSpecific
            } else if winner.priority != runner_up.priority {
                PrecedenceReason::HigherPriority
            } else {
                PrecedenceReason::LowestId
            }
        };

        let winner_id = winner.id.clone();
        Selection {
            winner: Some(winner_id.clone()),
            reason: Some(reason),
            candidates: candidates
                .iter()
                .map(|p| Candidate {
                    policy_id: p.id.clone(),
                    specificity: p.specificity(),
                    priority: p.priority,
                    selected: p.id == winner_id,
                })
                .collect(),
        }
    }

    /// The winning policy for `key`, if any.
    pub fn winner_for(&self, key: &ScopeKey) -> Option<&DetectionPolicy> {
        self.select(key)
            .winner
            .and_then(|id| self.policies.get(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{AddressFamily, ScopeId, ScopeType, TrafficDirection};
    use crate::policy::{
        ExecutionMode, PolicyDraft, PolicySelector, Severity, TenantPrefixes, Thresholds,
        DEFAULT_CLEAR_PERCENT, WILDCARD_TENANT,
    };
    use crate::MetricKind;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    fn policy(
        id: &str,
        tenant: &str,
        scope_type: ScopeType,
        selector: PolicySelector,
        priority: i32,
    ) -> DetectionPolicy {
        PolicyDraft {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            enabled: true,
            tenant: tenant.to_string(),
            scope_type,
            selector,
            address_family: None,
            direction: TrafficDirection::Incoming,
            window: Duration::from_secs(15),
            thresholds: Thresholds::new().with(MetricKind::Bps, 1000),
            clear_percent: DEFAULT_CLEAR_PERCENT,
            trigger_for: Duration::from_secs(15),
            clear_for: Duration::from_secs(30),
            cooldown: Duration::from_secs(60),
            hold_down: Duration::from_secs(30),
            event_update_interval: Duration::from_secs(60),
            severity: Severity::Major,
            execution_mode: ExecutionMode::AlertOnly,
            priority,
            labels: Default::default(),
            version: 1,
        }
        .validate(&TenantPrefixes::new())
        .unwrap()
    }

    fn host_key() -> ScopeKey {
        ScopeKey {
            tenant: "t1".to_string(),
            scope_type: ScopeType::Host,
            scope_id: ScopeId::Host {
                addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            },
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        }
    }

    fn network_key(addr: [u8; 4], len: u8) -> ScopeKey {
        ScopeKey {
            tenant: "t1".to_string(),
            scope_type: ScopeType::Prefix,
            scope_id: ScopeId::Network {
                addr: IpAddr::V4(Ipv4Addr::from(addr)),
                prefix_len: len,
            },
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        }
    }

    #[test]
    fn duplicate_policy_ids_are_rejected() {
        let err = PolicySet::from_policies(vec![
            policy("dup", "t1", ScopeType::Host, PolicySelector::Any, 0),
            policy("dup", "t1", ScopeType::Host, PolicySelector::Any, 0),
        ])
        .unwrap_err();
        assert!(matches!(err, PolicyError::DuplicateId { .. }));
    }

    #[test]
    fn no_matching_policy_selects_nothing() {
        let set = PolicySet::from_policies(vec![policy(
            "other-tenant",
            "t2",
            ScopeType::Host,
            PolicySelector::Any,
            0,
        )])
        .unwrap();
        let selection = set.select(&host_key());
        assert_eq!(selection.winner, None);
        assert!(selection.candidates.is_empty());
    }

    #[test]
    fn an_explicit_host_policy_beats_the_tenant_default() {
        let set = PolicySet::from_policies(vec![
            policy(
                "tenant-default",
                "t1",
                ScopeType::Host,
                PolicySelector::Any,
                0,
            ),
            policy(
                "explicit-host",
                "t1",
                ScopeType::Host,
                PolicySelector::Host {
                    addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
                },
                0,
            ),
        ])
        .unwrap();
        let selection = set.select(&host_key());
        assert_eq!(selection.winner.as_deref(), Some("explicit-host"));
        assert_eq!(selection.reason, Some(PrecedenceReason::MoreSpecific));
        assert_eq!(selection.candidates.len(), 2);
    }

    #[test]
    fn the_tenant_default_beats_the_global_default() {
        let set = PolicySet::from_policies(vec![
            policy(
                "global-default",
                WILDCARD_TENANT,
                ScopeType::Host,
                PolicySelector::Any,
                0,
            ),
            policy(
                "tenant-default",
                "t1",
                ScopeType::Host,
                PolicySelector::Any,
                0,
            ),
        ])
        .unwrap();
        let selection = set.select(&host_key());
        assert_eq!(selection.winner.as_deref(), Some("tenant-default"));
        assert_eq!(selection.reason, Some(PrecedenceReason::MoreSpecific));
    }

    #[test]
    fn the_global_default_applies_when_the_tenant_has_none() {
        let set = PolicySet::from_policies(vec![policy(
            "global-default",
            WILDCARD_TENANT,
            ScopeType::Host,
            PolicySelector::Any,
            0,
        )])
        .unwrap();
        let selection = set.select(&host_key());
        assert_eq!(selection.winner.as_deref(), Some("global-default"));
        assert_eq!(selection.reason, Some(PrecedenceReason::OnlyCandidate));
    }

    #[test]
    fn the_longest_matching_prefix_wins() {
        let set = PolicySet::from_policies(vec![
            policy(
                "short",
                "t1",
                ScopeType::Prefix,
                PolicySelector::Network {
                    addr: IpAddr::V4(Ipv4Addr::new(198, 51, 0, 0)),
                    prefix_len: 16,
                },
                0,
            ),
            policy(
                "long",
                "t1",
                ScopeType::Prefix,
                PolicySelector::Network {
                    addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
                    prefix_len: 24,
                },
                0,
            ),
        ])
        .unwrap();
        // Only the /24 policy matches the /24 scope exactly; the /16 one
        // does not, because a prefix policy targets a prefix scope by
        // identity, not by containment.
        let selection = set.select(&network_key([198, 51, 100, 0], 24));
        assert_eq!(selection.winner.as_deref(), Some("long"));
    }

    #[test]
    fn explicit_priority_breaks_a_specificity_tie() {
        let set = PolicySet::from_policies(vec![
            policy("aaa-low", "t1", ScopeType::Host, PolicySelector::Any, 1),
            policy("zzz-high", "t1", ScopeType::Host, PolicySelector::Any, 99),
        ])
        .unwrap();
        let selection = set.select(&host_key());
        assert_eq!(selection.winner.as_deref(), Some("zzz-high"));
        assert_eq!(selection.reason, Some(PrecedenceReason::HigherPriority));
    }

    #[test]
    fn a_full_tie_falls_back_to_the_lowest_id_and_says_so() {
        let set = PolicySet::from_policies(vec![
            policy("bbb", "t1", ScopeType::Host, PolicySelector::Any, 0),
            policy("aaa", "t1", ScopeType::Host, PolicySelector::Any, 0),
        ])
        .unwrap();
        let selection = set.select(&host_key());
        assert_eq!(selection.winner.as_deref(), Some("aaa"));
        assert_eq!(selection.reason, Some(PrecedenceReason::LowestId));
    }

    #[test]
    fn selection_is_identical_across_many_runs() {
        // Built fresh each time so any hash-order dependence would show.
        let expected = {
            let set = PolicySet::from_policies(vec![
                policy("p-b", "t1", ScopeType::Host, PolicySelector::Any, 5),
                policy("p-a", "t1", ScopeType::Host, PolicySelector::Any, 5),
                policy("p-c", "t1", ScopeType::Host, PolicySelector::Any, 5),
            ])
            .unwrap();
            set.select(&host_key())
        };
        for _ in 0..50 {
            let set = PolicySet::from_policies(vec![
                policy("p-c", "t1", ScopeType::Host, PolicySelector::Any, 5),
                policy("p-a", "t1", ScopeType::Host, PolicySelector::Any, 5),
                policy("p-b", "t1", ScopeType::Host, PolicySelector::Any, 5),
            ])
            .unwrap();
            assert_eq!(set.select(&host_key()), expected);
        }
    }

    #[test]
    fn candidates_record_which_one_was_selected() {
        let set = PolicySet::from_policies(vec![
            policy(
                "tenant-default",
                "t1",
                ScopeType::Host,
                PolicySelector::Any,
                0,
            ),
            policy(
                "explicit-host",
                "t1",
                ScopeType::Host,
                PolicySelector::Host {
                    addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
                },
                0,
            ),
        ])
        .unwrap();
        let selection = set.select(&host_key());
        let selected: Vec<&Candidate> =
            selection.candidates.iter().filter(|c| c.selected).collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].policy_id, "explicit-host");
    }

    #[test]
    fn a_disabled_policy_is_not_even_a_candidate() {
        let mut disabled = policy("disabled", "t1", ScopeType::Host, PolicySelector::Any, 0);
        disabled.enabled = false;
        let set = PolicySet::from_policies(vec![disabled]).unwrap();
        assert!(set.select(&host_key()).candidates.is_empty());
    }

    #[test]
    fn policies_for_a_different_direction_do_not_compete() {
        let mut outgoing = policy("outgoing", "t1", ScopeType::Host, PolicySelector::Any, 100);
        outgoing.direction = TrafficDirection::Outgoing;
        let set = PolicySet::from_policies(vec![
            outgoing,
            policy("incoming", "t1", ScopeType::Host, PolicySelector::Any, 0),
        ])
        .unwrap();
        assert_eq!(
            set.select(&host_key()).winner.as_deref(),
            Some("incoming"),
            "an outgoing policy must not govern incoming traffic however high its priority"
        );
    }

    #[test]
    fn a_family_specific_policy_beats_a_family_agnostic_one() {
        let mut v4_only = policy("v4-only", "t1", ScopeType::Host, PolicySelector::Any, 0);
        v4_only.address_family = Some(AddressFamily::Ipv4);
        let set = PolicySet::from_policies(vec![
            v4_only,
            policy("any-family", "t1", ScopeType::Host, PolicySelector::Any, 0),
        ])
        .unwrap();
        assert_eq!(set.select(&host_key()).winner.as_deref(), Some("v4-only"));
    }

    #[test]
    fn scope_types_never_compete_with_each_other() {
        let set = PolicySet::from_policies(vec![
            policy(
                "hostgroup",
                "t1",
                ScopeType::HostgroupTotal,
                PolicySelector::Any,
                1000,
            ),
            policy("host", "t1", ScopeType::Host, PolicySelector::Any, 0),
        ])
        .unwrap();
        // A hostgroup policy governs the hostgroup scope; it does not
        // outrank a host policy on the host scope. They produce separate
        // events on separate scopes, which is the documented Phase 4
        // behaviour (ADR 0009).
        assert_eq!(set.select(&host_key()).winner.as_deref(), Some("host"));
    }
}
