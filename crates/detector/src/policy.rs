//! Detection policies: what an operator writes, and what the engine
//! refuses to accept.
//!
//! Validation here is deliberately unforgiving. A detection policy is
//! the thing standing between real traffic and an operator's pager, and
//! a policy that is subtly wrong — a clear threshold above its trigger
//! threshold, a trigger duration shorter than the window that feeds it —
//! does not fail loudly. It quietly produces either silence or a storm.
//! Every such shape is rejected at load time with a message naming the
//! policy and the field, rather than discovered at three in the morning.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::input::{AddressFamily, MetricKind, ScopeId, ScopeType, TrafficDirection};

/// Hard bounds on policy content. These exist so a hostile or careless
/// policy file cannot become a memory-exhaustion vector — see
/// docs/security/detection-safety.md.
pub const MAX_POLICY_ID_LEN: usize = 128;
pub const MAX_POLICY_NAME_LEN: usize = 256;
pub const MAX_DESCRIPTION_LEN: usize = 2048;
pub const MAX_LABELS: usize = 16;
pub const MAX_LABEL_KEY_LEN: usize = 64;
pub const MAX_LABEL_VALUE_LEN: usize = 256;
/// Longest window the engine will evaluate against. Phase 3 produces
/// 1s/5s/15s/1m/5m windows; anything longer has no source.
pub const MAX_WINDOW: Duration = Duration::from_secs(300);
/// Longest any single timer may be set to. A year-long cooldown is
/// almost certainly a units mistake, not an intention.
pub const MAX_TIMER: Duration = Duration::from_secs(7 * 24 * 3600);

/// The tenant value that means "every tenant".
///
/// This is how a global default policy is expressed. It is deliberately
/// a character no real tenant identifier would contain, and a policy
/// carrying it is always less specific than one naming a tenant — so a
/// global default can never outrank a tenant's own policy.
pub const WILDCARD_TENANT: &str = "*";

/// What the engine is allowed to do when a policy matches.
///
/// There is deliberately no mitigation-capable variant. Adding one is a
/// later phase's decision, and leaving a placeholder here would invite
/// code that treats "not yet implemented" as "temporarily disabled".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionMode {
    /// Not evaluated at all.
    Disabled,
    /// Evaluated, state advanced, metrics updated — but no event leaves
    /// the engine. For tuning a threshold against live traffic without
    /// waking anyone.
    Observe,
    /// Evaluated and events emitted. Never requests mitigation.
    AlertOnly,
    /// Evaluated and events emitted, each carrying the action that
    /// *would* have been proposed, explicitly marked as not executed.
    DryRun,
}

impl ExecutionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionMode::Disabled => "disabled",
            ExecutionMode::Observe => "observe",
            ExecutionMode::AlertOnly => "alertOnly",
            ExecutionMode::DryRun => "dryRun",
        }
    }

    /// Whether this mode evaluates traffic at all.
    pub fn evaluates(&self) -> bool {
        !matches!(self, ExecutionMode::Disabled)
    }

    /// Whether this mode lets events reach a sink.
    pub fn emits_events(&self) -> bool {
        matches!(self, ExecutionMode::AlertOnly | ExecutionMode::DryRun)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Minor,
    Major,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Minor => "minor",
            Severity::Major => "major",
            Severity::Critical => "critical",
        }
    }
}

/// What a policy is written against.
///
/// `Any` is how a tenant-wide or global default is expressed: it matches
/// every scope of its declared type within the policy's tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PolicySelector {
    Host {
        addr: IpAddr,
    },
    Network {
        addr: IpAddr,
        #[serde(rename = "prefixLen")]
        prefix_len: u8,
    },
    Hostgroup {
        name: String,
    },
    Any,
}

impl std::fmt::Display for PolicySelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicySelector::Host { addr } => write!(f, "{addr}"),
            PolicySelector::Network { addr, prefix_len } => write!(f, "{addr}/{prefix_len}"),
            PolicySelector::Hostgroup { name } => write!(f, "{name}"),
            PolicySelector::Any => write!(f, "*"),
        }
    }
}

/// Threshold values in canonical units: bits/sec, packets/sec, flows/sec.
///
/// A `BTreeMap` rather than a `HashMap` so that iterating the matched
/// reasons of an event produces the same order every run. An event whose
/// reason list reshuffles between restarts is one an operator cannot
/// diff.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Thresholds(pub BTreeMap<MetricKind, u64>);

impl Thresholds {
    pub fn new() -> Self {
        Thresholds(BTreeMap::new())
    }

    pub fn with(mut self, kind: MetricKind, canonical_value: u64) -> Self {
        self.0.insert(kind, canonical_value);
        self
    }

    pub fn get(&self, kind: MetricKind) -> Option<u64> {
        self.0.get(&kind).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (MetricKind, u64)> + '_ {
        self.0.iter().map(|(k, v)| (*k, *v))
    }
}

/// How far traffic must fall before an active detection is allowed to
/// start clearing, expressed as a percentage of the trigger threshold.
///
/// Integer percent, not a float ratio, so the clear threshold is computed
/// with exact integer arithmetic — see ADR 0008 for why hysteresis is a
/// ratio of the trigger rather than a second independent threshold.
/// `100` means no hysteresis at all: clearing begins as soon as traffic
/// drops below the trigger threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClearPercent(u8);

pub const DEFAULT_CLEAR_PERCENT: u8 = 80;

impl ClearPercent {
    pub fn new(percent: u8) -> Option<Self> {
        if (1..=100).contains(&percent) {
            Some(ClearPercent(percent))
        } else {
            None
        }
    }

    pub fn get(&self) -> u8 {
        self.0
    }

    /// The clear threshold for a given trigger threshold.
    ///
    /// Computed in `u128` so a trigger threshold near `u64::MAX` cannot
    /// overflow the multiplication — at 100 Gbps a `u64` of bits per
    /// second is nowhere near its limit, but a policy file is operator
    /// input and must not be trusted to be sensible.
    pub fn clear_threshold(&self, trigger: u64) -> u64 {
        let scaled = (trigger as u128) * (self.0 as u128) / 100;
        u64::try_from(scaled).unwrap_or(u64::MAX)
    }
}

impl Default for ClearPercent {
    fn default() -> Self {
        ClearPercent(DEFAULT_CLEAR_PERCENT)
    }
}

/// A validated detection policy.
///
/// Construct one only through [`PolicyDraft::validate`]. There is no
/// public constructor that skips validation, so an invalid policy cannot
/// reach the engine by any route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPolicy {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub tenant: String,
    pub scope_type: ScopeType,
    pub selector: PolicySelector,
    /// `None` matches both families.
    pub address_family: Option<AddressFamily>,
    pub direction: TrafficDirection,
    pub window: Duration,
    pub thresholds: Thresholds,
    pub clear_percent: ClearPercent,
    pub trigger_for: Duration,
    pub clear_for: Duration,
    pub cooldown: Duration,
    pub hold_down: Duration,
    pub event_update_interval: Duration,
    pub severity: Severity,
    pub execution_mode: ExecutionMode,
    /// Higher wins when two policies are otherwise equally specific.
    pub priority: i32,
    pub labels: BTreeMap<String, String>,
    /// Bumped by the operator when the policy's meaning changes. Stamped
    /// onto every event so an alert can be traced to the exact policy
    /// text that produced it.
    pub version: u32,
}

impl DetectionPolicy {
    /// The clear threshold for one metric, or `None` if this policy has
    /// no trigger threshold for it.
    pub fn clear_threshold(&self, kind: MetricKind) -> Option<u64> {
        self.thresholds
            .get(kind)
            .map(|t| self.clear_percent.clear_threshold(t))
    }

    /// Whether this policy could ever apply to the given scope key.
    pub fn matches_scope(&self, key: &crate::input::ScopeKey) -> bool {
        if !self.enabled {
            return false;
        }
        if self.tenant != WILDCARD_TENANT && self.tenant != key.tenant {
            return false;
        }
        if self.scope_type != key.scope_type {
            return false;
        }
        if self.direction != key.direction {
            return false;
        }
        if let Some(family) = self.address_family {
            if family != key.address_family {
                return false;
            }
        }
        match (&self.selector, &key.scope_id) {
            (PolicySelector::Any, _) => true,
            (PolicySelector::Host { addr }, ScopeId::Host { addr: other }) => addr == other,
            (
                PolicySelector::Network { addr, prefix_len },
                ScopeId::Network {
                    addr: other,
                    prefix_len: other_len,
                },
            ) => addr == other && prefix_len == other_len,
            (PolicySelector::Hostgroup { name }, ScopeId::Hostgroup { name: other }) => {
                name == other
            }
            _ => false,
        }
    }

    /// How specific this policy is, for precedence. See ADR 0009.
    pub fn specificity(&self) -> u32 {
        let scope = self.scope_type.specificity() as u32 * 1000;
        let selector = match &self.selector {
            PolicySelector::Host { .. } => 900,
            PolicySelector::Network { prefix_len, .. } => 100 + *prefix_len as u32,
            PolicySelector::Hostgroup { .. } => 50,
            PolicySelector::Any => 0,
        };
        let family = if self.address_family.is_some() { 1 } else { 0 };
        // A policy naming its tenant always outranks the global default,
        // whatever their selectors — hence a term larger than any
        // scope/selector combination can reach.
        let tenant = if self.tenant == WILDCARD_TENANT {
            0
        } else {
            1_000_000
        };
        tenant + scope + selector + family
    }
}

/// Everything that can be wrong with a policy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("policy id must not be empty")]
    EmptyId,
    #[error("policy id {id:?} is longer than {MAX_POLICY_ID_LEN} characters")]
    IdTooLong { id: String },
    #[error(
        "policy id {id:?} contains {ch:?}; ids may use only ASCII letters, digits, '-', '_' and '.'"
    )]
    IdCharacter { id: String, ch: char },
    #[error("policy {id:?} has an empty name")]
    EmptyName { id: String },
    #[error("policy {id:?} has a name longer than {MAX_POLICY_NAME_LEN} characters")]
    NameTooLong { id: String },
    #[error("policy {id:?} has a description longer than {MAX_DESCRIPTION_LEN} characters")]
    DescriptionTooLong { id: String },
    #[error("policy {id:?} has an empty tenant")]
    EmptyTenant { id: String },
    #[error("policy {id:?} has {count} labels; at most {MAX_LABELS} are allowed")]
    TooManyLabels { id: String, count: usize },
    #[error("policy {id:?} has an oversized label {key:?}")]
    LabelTooLong { id: String, key: String },
    #[error("policy {id:?} has a zero-length rate window")]
    ZeroWindow { id: String },
    #[error(
        "policy {id:?} has a rate window of {window:?}, longer than the maximum {MAX_WINDOW:?}"
    )]
    WindowTooLong { id: String, window: Duration },
    #[error("policy {id:?} declares no thresholds, so it can never trigger")]
    NoThresholds { id: String },
    #[error(
        "policy {id:?} sets {metric} to zero; the comparison is >=, so a zero threshold would match \
         idle traffic — use 1 to mean 'any at all'"
    )]
    ZeroThreshold { id: String, metric: &'static str },
    #[error(
        "policy {id:?} sets {metric}, which is not meaningful for scope {scope}: \
         flow counts are per-scope but this metric is not"
    )]
    MetricScopeMismatch {
        id: String,
        metric: &'static str,
        scope: &'static str,
    },
    #[error("policy {id:?} has a clear percentage of {percent}; it must be between 1 and 100")]
    InvalidClearPercent { id: String, percent: u8 },
    #[error(
        "policy {id:?} has triggerFor {trigger_for:?}, shorter than its {window:?} window; \
         a duration shorter than the evaluation resolution can never be measured"
    )]
    TriggerShorterThanWindow {
        id: String,
        trigger_for: Duration,
        window: Duration,
    },
    #[error(
        "policy {id:?} has clearFor {clear_for:?}, shorter than its {window:?} window; \
         a duration shorter than the evaluation resolution can never be measured"
    )]
    ClearShorterThanWindow {
        id: String,
        clear_for: Duration,
        window: Duration,
    },
    #[error("policy {id:?} has {field} of {value:?}, longer than the maximum {MAX_TIMER:?}")]
    TimerTooLong {
        id: String,
        field: &'static str,
        value: Duration,
    },
    #[error(
        "policy {id:?} has a zero event update interval, which would emit an update on every \
         evaluation"
    )]
    ZeroEventUpdateInterval { id: String },
    #[error(
        "policy {id:?} targets scope type {scope} with a {selector} selector, which cannot match"
    )]
    SelectorScopeMismatch {
        id: String,
        scope: &'static str,
        selector: &'static str,
    },
    #[error("policy {id:?} has an invalid prefix length /{prefix_len} for {addr}")]
    InvalidPrefixLength {
        id: String,
        addr: IpAddr,
        prefix_len: u8,
    },
    #[error(
        "policy {id:?} targets {addr}/{prefix_len}, which is not covered by any prefix tenant \
         {tenant:?} owns"
    )]
    PrefixNotOwnedByTenant {
        id: String,
        tenant: String,
        addr: IpAddr,
        prefix_len: u8,
    },
    #[error("policy {id:?} targets direction {direction}, which cannot be detected on")]
    UnsupportedDirection { id: String, direction: &'static str },
    #[error("duplicate policy id {id:?}")]
    DuplicateId { id: String },
}

/// An unvalidated policy, as read from configuration.
///
/// Every field is plain data. The only way to get a [`DetectionPolicy`]
/// out of one is [`PolicyDraft::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDraft {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub tenant: String,
    pub scope_type: ScopeType,
    pub selector: PolicySelector,
    pub address_family: Option<AddressFamily>,
    pub direction: TrafficDirection,
    pub window: Duration,
    pub thresholds: Thresholds,
    pub clear_percent: u8,
    pub trigger_for: Duration,
    pub clear_for: Duration,
    pub cooldown: Duration,
    pub hold_down: Duration,
    pub event_update_interval: Duration,
    pub severity: Severity,
    pub execution_mode: ExecutionMode,
    pub priority: i32,
    pub labels: BTreeMap<String, String>,
    pub version: u32,
}

impl PolicyDraft {
    /// Checks every rule, returning the first violation found.
    ///
    /// `tenant_prefixes` is the set of prefixes each tenant owns, used to
    /// reject a policy that targets an address range its tenant has no
    /// claim to. Pass an empty map to skip that check — appropriate when
    /// prefix ownership is not configured, and documented as such in
    /// docs/configuration/detection-policies.md.
    pub fn validate(
        self,
        tenant_prefixes: &TenantPrefixes,
    ) -> Result<DetectionPolicy, PolicyError> {
        validate_id(&self.id)?;
        let id = self.id.clone();

        if self.name.trim().is_empty() {
            return Err(PolicyError::EmptyName { id });
        }
        if self.name.chars().count() > MAX_POLICY_NAME_LEN {
            return Err(PolicyError::NameTooLong { id });
        }
        if let Some(d) = &self.description {
            if d.chars().count() > MAX_DESCRIPTION_LEN {
                return Err(PolicyError::DescriptionTooLong { id });
            }
        }
        if self.tenant.trim().is_empty() {
            return Err(PolicyError::EmptyTenant { id });
        }
        if self.labels.len() > MAX_LABELS {
            return Err(PolicyError::TooManyLabels {
                id,
                count: self.labels.len(),
            });
        }
        for (k, v) in &self.labels {
            if k.chars().count() > MAX_LABEL_KEY_LEN || v.chars().count() > MAX_LABEL_VALUE_LEN {
                return Err(PolicyError::LabelTooLong { id, key: k.clone() });
            }
        }

        if self.window.is_zero() {
            return Err(PolicyError::ZeroWindow { id });
        }
        if self.window > MAX_WINDOW {
            return Err(PolicyError::WindowTooLong {
                id,
                window: self.window,
            });
        }

        if self.thresholds.is_empty() {
            return Err(PolicyError::NoThresholds { id });
        }
        for (metric, value) in self.thresholds.iter() {
            if value == 0 {
                return Err(PolicyError::ZeroThreshold {
                    id,
                    metric: metric.as_str(),
                });
            }
        }

        let clear_percent =
            ClearPercent::new(self.clear_percent).ok_or(PolicyError::InvalidClearPercent {
                id: id.clone(),
                percent: self.clear_percent,
            })?;

        if self.trigger_for < self.window {
            return Err(PolicyError::TriggerShorterThanWindow {
                id,
                trigger_for: self.trigger_for,
                window: self.window,
            });
        }
        if self.clear_for < self.window {
            return Err(PolicyError::ClearShorterThanWindow {
                id,
                clear_for: self.clear_for,
                window: self.window,
            });
        }
        for (field, value) in [
            ("triggerFor", self.trigger_for),
            ("clearFor", self.clear_for),
            ("cooldown", self.cooldown),
            ("holdDown", self.hold_down),
            ("eventUpdateInterval", self.event_update_interval),
        ] {
            if value > MAX_TIMER {
                return Err(PolicyError::TimerTooLong { id, field, value });
            }
        }
        if self.event_update_interval.is_zero() {
            return Err(PolicyError::ZeroEventUpdateInterval { id });
        }

        validate_selector(&id, self.scope_type, &self.selector)?;
        validate_direction(&id, self.direction)?;
        validate_prefix_ownership(&id, &self.tenant, &self.selector, tenant_prefixes)?;

        Ok(DetectionPolicy {
            id: self.id,
            name: self.name,
            description: self.description,
            enabled: self.enabled,
            tenant: self.tenant,
            scope_type: self.scope_type,
            selector: self.selector,
            address_family: self.address_family,
            direction: self.direction,
            window: self.window,
            thresholds: self.thresholds,
            clear_percent,
            trigger_for: self.trigger_for,
            clear_for: self.clear_for,
            cooldown: self.cooldown,
            hold_down: self.hold_down,
            event_update_interval: self.event_update_interval,
            severity: self.severity,
            execution_mode: self.execution_mode,
            priority: self.priority,
            labels: self.labels,
            version: self.version,
        })
    }
}

fn validate_id(id: &str) -> Result<(), PolicyError> {
    if id.is_empty() {
        return Err(PolicyError::EmptyId);
    }
    if id.chars().count() > MAX_POLICY_ID_LEN {
        return Err(PolicyError::IdTooLong { id: id.to_string() });
    }
    if let Some(ch) = id
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    {
        return Err(PolicyError::IdCharacter {
            id: id.to_string(),
            ch,
        });
    }
    Ok(())
}

fn validate_selector(
    id: &str,
    scope_type: ScopeType,
    selector: &PolicySelector,
) -> Result<(), PolicyError> {
    let compatible = matches!(
        (scope_type, selector),
        (_, PolicySelector::Any)
            | (ScopeType::Host, PolicySelector::Host { .. })
            | (ScopeType::Prefix, PolicySelector::Network { .. })
            | (
                ScopeType::Slash24,
                PolicySelector::Network { prefix_len: 24, .. }
            )
            | (ScopeType::HostgroupTotal, PolicySelector::Hostgroup { .. })
    );
    if !compatible {
        return Err(PolicyError::SelectorScopeMismatch {
            id: id.to_string(),
            scope: scope_type.as_str(),
            selector: match selector {
                PolicySelector::Host { .. } => "host",
                PolicySelector::Network { .. } => "network",
                PolicySelector::Hostgroup { .. } => "hostgroup",
                PolicySelector::Any => "any",
            },
        });
    }
    if let PolicySelector::Network { addr, prefix_len } = selector {
        let max = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if *prefix_len > max {
            return Err(PolicyError::InvalidPrefixLength {
                id: id.to_string(),
                addr: *addr,
                prefix_len: *prefix_len,
            });
        }
    }
    Ok(())
}

fn validate_direction(id: &str, direction: TrafficDirection) -> Result<(), PolicyError> {
    // Unknown means the classifier could not decide, which happens when
    // no local prefixes are configured at all. Alerting on it would fire
    // on every flow in a misconfigured deployment, which is a
    // configuration problem to fix rather than an attack to page about.
    if direction == TrafficDirection::Unknown {
        return Err(PolicyError::UnsupportedDirection {
            id: id.to_string(),
            direction: direction.as_str(),
        });
    }
    Ok(())
}

/// The prefixes each tenant owns, used to reject cross-tenant targeting.
#[derive(Debug, Clone, Default)]
pub struct TenantPrefixes {
    entries: BTreeMap<String, Vec<(IpAddr, u8)>>,
}

impl TenantPrefixes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, tenant: impl Into<String>, addr: IpAddr, prefix_len: u8) {
        self.entries
            .entry(tenant.into())
            .or_default()
            .push((addr, prefix_len));
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether `tenant` owns a prefix that fully contains `addr/len`.
    pub fn covers(&self, tenant: &str, addr: IpAddr, prefix_len: u8) -> bool {
        let Some(owned) = self.entries.get(tenant) else {
            return false;
        };
        owned.iter().any(|(owned_addr, owned_len)| {
            *owned_len <= prefix_len && prefix_contains(*owned_addr, *owned_len, addr)
        })
    }
}

fn validate_prefix_ownership(
    id: &str,
    tenant: &str,
    selector: &PolicySelector,
    tenant_prefixes: &TenantPrefixes,
) -> Result<(), PolicyError> {
    if tenant_prefixes.is_empty() || tenant == WILDCARD_TENANT {
        return Ok(());
    }
    let (addr, prefix_len) = match selector {
        PolicySelector::Network { addr, prefix_len } => (*addr, *prefix_len),
        PolicySelector::Host { addr } => (
            *addr,
            match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            },
        ),
        _ => return Ok(()),
    };
    if tenant_prefixes.covers(tenant, addr, prefix_len) {
        Ok(())
    } else {
        Err(PolicyError::PrefixNotOwnedByTenant {
            id: id.to_string(),
            tenant: tenant.to_string(),
            addr,
            prefix_len,
        })
    }
}

/// Whether `network/prefix_len` contains `addr`.
pub(crate) fn prefix_contains(network: IpAddr, prefix_len: u8, addr: IpAddr) -> bool {
    match (network, addr) {
        (IpAddr::V4(n), IpAddr::V4(a)) => {
            if prefix_len > 32 {
                return false;
            }
            if prefix_len == 0 {
                return true;
            }
            let mask = u32::MAX << (32 - prefix_len as u32);
            (u32::from(n) & mask) == (u32::from(a) & mask)
        }
        (IpAddr::V6(n), IpAddr::V6(a)) => {
            if prefix_len > 128 {
                return false;
            }
            if prefix_len == 0 {
                return true;
            }
            let mask = u128::MAX << (128 - prefix_len as u32);
            (u128::from(n) & mask) == (u128::from(a) & mask)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn draft() -> PolicyDraft {
        PolicyDraft {
            id: "inbound-host".to_string(),
            name: "Inbound host protection".to_string(),
            description: None,
            enabled: true,
            tenant: "tenant-a".to_string(),
            scope_type: ScopeType::Host,
            selector: PolicySelector::Host {
                addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            },
            address_family: None,
            direction: TrafficDirection::Incoming,
            window: Duration::from_secs(15),
            thresholds: Thresholds::new().with(MetricKind::Bps, 1_500_000_000),
            clear_percent: DEFAULT_CLEAR_PERCENT,
            trigger_for: Duration::from_secs(15),
            clear_for: Duration::from_secs(30),
            cooldown: Duration::from_secs(300),
            hold_down: Duration::from_secs(60),
            event_update_interval: Duration::from_secs(60),
            severity: Severity::Critical,
            execution_mode: ExecutionMode::AlertOnly,
            priority: 0,
            labels: BTreeMap::new(),
            version: 1,
        }
    }

    #[test]
    fn a_well_formed_policy_validates() {
        let policy = draft().validate(&TenantPrefixes::new()).unwrap();
        assert_eq!(policy.id, "inbound-host");
        assert_eq!(policy.clear_percent.get(), 80);
    }

    #[test]
    fn empty_id_is_rejected() {
        let mut d = draft();
        d.id = String::new();
        assert_eq!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::EmptyId)
        );
    }

    #[test]
    fn id_with_a_space_is_rejected() {
        let mut d = draft();
        d.id = "bad id".to_string();
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::IdCharacter { ch: ' ', .. })
        ));
    }

    #[test]
    fn overlong_id_is_rejected() {
        let mut d = draft();
        d.id = "a".repeat(MAX_POLICY_ID_LEN + 1);
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::IdTooLong { .. })
        ));
    }

    #[test]
    fn empty_name_and_tenant_are_rejected() {
        let mut d = draft();
        d.name = "   ".to_string();
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::EmptyName { .. })
        ));

        let mut d = draft();
        d.tenant = String::new();
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::EmptyTenant { .. })
        ));
    }

    #[test]
    fn zero_window_is_rejected() {
        let mut d = draft();
        d.window = Duration::ZERO;
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::ZeroWindow { .. })
        ));
    }

    #[test]
    fn window_longer_than_the_longest_source_window_is_rejected() {
        let mut d = draft();
        d.window = MAX_WINDOW + Duration::from_secs(1);
        d.trigger_for = d.window;
        d.clear_for = d.window;
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::WindowTooLong { .. })
        ));
    }

    #[test]
    fn a_policy_with_no_thresholds_is_rejected() {
        let mut d = draft();
        d.thresholds = Thresholds::new();
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::NoThresholds { .. })
        ));
    }

    #[test]
    fn a_zero_threshold_is_rejected_because_the_comparison_is_inclusive() {
        let mut d = draft();
        d.thresholds = Thresholds::new().with(MetricKind::Pps, 0);
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::ZeroThreshold { metric: "pps", .. })
        ));
    }

    #[test]
    fn clear_percent_outside_one_to_hundred_is_rejected() {
        for percent in [0u8, 101, 255] {
            let mut d = draft();
            d.clear_percent = percent;
            assert!(
                matches!(
                    d.validate(&TenantPrefixes::new()),
                    Err(PolicyError::InvalidClearPercent { .. })
                ),
                "percent {percent} should be rejected"
            );
        }
    }

    #[test]
    fn trigger_shorter_than_the_window_is_rejected() {
        let mut d = draft();
        d.trigger_for = Duration::from_secs(5);
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::TriggerShorterThanWindow { .. })
        ));
    }

    #[test]
    fn clear_shorter_than_the_window_is_rejected() {
        let mut d = draft();
        d.clear_for = Duration::from_secs(1);
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::ClearShorterThanWindow { .. })
        ));
    }

    #[test]
    fn an_absurdly_long_timer_is_rejected() {
        let mut d = draft();
        d.cooldown = MAX_TIMER + Duration::from_secs(1);
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::TimerTooLong {
                field: "cooldown",
                ..
            })
        ));
    }

    #[test]
    fn zero_event_update_interval_is_rejected() {
        let mut d = draft();
        d.event_update_interval = Duration::ZERO;
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::ZeroEventUpdateInterval { .. })
        ));
    }

    #[test]
    fn too_many_labels_are_rejected() {
        let mut d = draft();
        for i in 0..(MAX_LABELS + 1) {
            d.labels.insert(format!("k{i}"), "v".to_string());
        }
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::TooManyLabels { .. })
        ));
    }

    #[test]
    fn an_oversized_label_value_is_rejected() {
        let mut d = draft();
        d.labels
            .insert("k".to_string(), "v".repeat(MAX_LABEL_VALUE_LEN + 1));
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::LabelTooLong { .. })
        ));
    }

    #[test]
    fn a_selector_that_cannot_match_its_scope_is_rejected() {
        let mut d = draft();
        d.scope_type = ScopeType::HostgroupTotal;
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::SelectorScopeMismatch { .. })
        ));
    }

    #[test]
    fn a_slash24_scope_rejects_a_non_24_selector() {
        let mut d = draft();
        d.scope_type = ScopeType::Slash24;
        d.selector = PolicySelector::Network {
            addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
            prefix_len: 16,
        };
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::SelectorScopeMismatch { .. })
        ));
    }

    #[test]
    fn an_impossible_prefix_length_is_rejected() {
        let mut d = draft();
        d.scope_type = ScopeType::Prefix;
        d.selector = PolicySelector::Network {
            addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
            prefix_len: 33,
        };
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::InvalidPrefixLength { .. })
        ));
    }

    #[test]
    fn direction_unknown_is_rejected() {
        let mut d = draft();
        d.direction = TrafficDirection::Unknown;
        assert!(matches!(
            d.validate(&TenantPrefixes::new()),
            Err(PolicyError::UnsupportedDirection { .. })
        ));
    }

    #[test]
    fn a_prefix_outside_the_tenants_ownership_is_rejected() {
        let mut owned = TenantPrefixes::new();
        owned.insert("tenant-a", IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)), 24);

        let mut d = draft();
        d.scope_type = ScopeType::Prefix;
        d.selector = PolicySelector::Network {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 0)),
            prefix_len: 24,
        };
        assert!(matches!(
            d.validate(&owned),
            Err(PolicyError::PrefixNotOwnedByTenant { .. })
        ));
    }

    #[test]
    fn a_prefix_inside_the_tenants_ownership_is_accepted() {
        let mut owned = TenantPrefixes::new();
        owned.insert("tenant-a", IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)), 16);

        let mut d = draft();
        d.scope_type = ScopeType::Prefix;
        d.selector = PolicySelector::Network {
            addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
            prefix_len: 24,
        };
        assert!(d.validate(&owned).is_ok());
    }

    #[test]
    fn ownership_is_skipped_entirely_when_no_prefixes_are_configured() {
        let mut d = draft();
        d.scope_type = ScopeType::Prefix;
        d.selector = PolicySelector::Network {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 0)),
            prefix_len: 24,
        };
        assert!(d.validate(&TenantPrefixes::new()).is_ok());
    }

    #[test]
    fn clear_threshold_is_a_percentage_of_the_trigger() {
        let p = ClearPercent::new(80).unwrap();
        assert_eq!(p.clear_threshold(1000), 800);
        assert_eq!(ClearPercent::new(100).unwrap().clear_threshold(1000), 1000);
        assert_eq!(ClearPercent::new(1).unwrap().clear_threshold(1000), 10);
    }

    #[test]
    fn clear_threshold_does_not_overflow_at_the_top_of_u64() {
        let p = ClearPercent::new(100).unwrap();
        assert_eq!(p.clear_threshold(u64::MAX), u64::MAX);
        let p = ClearPercent::new(50).unwrap();
        assert_eq!(p.clear_threshold(u64::MAX), u64::MAX / 2);
    }

    #[test]
    fn matches_scope_respects_tenant_direction_and_family() {
        let policy = draft().validate(&TenantPrefixes::new()).unwrap();
        let key = crate::input::ScopeKey {
            tenant: "tenant-a".to_string(),
            scope_type: ScopeType::Host,
            scope_id: ScopeId::Host {
                addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            },
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        };
        assert!(policy.matches_scope(&key));

        let mut other_tenant = key.clone();
        other_tenant.tenant = "tenant-b".to_string();
        assert!(!policy.matches_scope(&other_tenant));

        let mut other_direction = key.clone();
        other_direction.direction = TrafficDirection::Outgoing;
        assert!(!policy.matches_scope(&other_direction));
    }

    #[test]
    fn a_disabled_policy_matches_nothing() {
        let mut d = draft();
        d.enabled = false;
        let policy = d.validate(&TenantPrefixes::new()).unwrap();
        let key = crate::input::ScopeKey {
            tenant: "tenant-a".to_string(),
            scope_type: ScopeType::Host,
            scope_id: ScopeId::Host {
                addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            },
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        };
        assert!(!policy.matches_scope(&key));
    }

    #[test]
    fn an_any_selector_matches_every_scope_of_its_type() {
        let mut d = draft();
        d.selector = PolicySelector::Any;
        let policy = d.validate(&TenantPrefixes::new()).unwrap();
        for last in [1u8, 2, 250] {
            let key = crate::input::ScopeKey {
                tenant: "tenant-a".to_string(),
                scope_type: ScopeType::Host,
                scope_id: ScopeId::Host {
                    addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, last)),
                },
                direction: TrafficDirection::Incoming,
                address_family: AddressFamily::Ipv4,
            };
            assert!(policy.matches_scope(&key));
        }
    }

    #[test]
    fn specificity_ranks_host_above_prefix_above_any() {
        let host = draft().validate(&TenantPrefixes::new()).unwrap();

        let mut d = draft();
        d.scope_type = ScopeType::Prefix;
        d.selector = PolicySelector::Network {
            addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
            prefix_len: 24,
        };
        let prefix = d.validate(&TenantPrefixes::new()).unwrap();

        let mut d = draft();
        d.scope_type = ScopeType::HostgroupTotal;
        d.selector = PolicySelector::Any;
        let any = d.validate(&TenantPrefixes::new()).unwrap();

        assert!(host.specificity() > prefix.specificity());
        assert!(prefix.specificity() > any.specificity());
    }

    #[test]
    fn a_longer_prefix_is_more_specific_than_a_shorter_one() {
        let make = |len: u8| {
            let mut d = draft();
            d.scope_type = ScopeType::Prefix;
            d.selector = PolicySelector::Network {
                addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
                prefix_len: len,
            };
            d.validate(&TenantPrefixes::new()).unwrap()
        };
        assert!(make(24).specificity() > make(16).specificity());
    }

    #[test]
    fn prefix_contains_handles_boundaries_and_family_mismatch() {
        let net = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0));
        assert!(prefix_contains(
            net,
            24,
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 255))
        ));
        assert!(!prefix_contains(
            net,
            24,
            IpAddr::V4(Ipv4Addr::new(198, 51, 101, 0))
        ));
        assert!(prefix_contains(
            net,
            0,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))
        ));
        assert!(!prefix_contains(
            net,
            24,
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
        ));
        assert!(!prefix_contains(
            net,
            33,
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1))
        ));
    }
}
