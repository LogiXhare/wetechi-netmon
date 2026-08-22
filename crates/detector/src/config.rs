//! Reading detection policies from a document.
//!
//! Two properties matter more than convenience here.
//!
//! **A typo is an error, not a silent default.** Every structure is
//! `deny_unknown_fields`. An operator who writes `trigger_for` where the
//! schema says `triggerFor` gets a parse failure naming the field, not a
//! policy that silently uses the default and never fires.
//!
//! **A number is never ambiguous about its units.** Every duration
//! carries a unit suffix (`"30s"`, `"250ms"`, `"5m"`), so `triggerFor`
//! can never be read as 30 milliseconds when 30 seconds was meant. Every
//! threshold may be written with a decimal magnitude suffix (`"10G"` for
//! ten billion bits per second), because an operator asked to type
//! `10000000000` will eventually type it with one zero too few.
//!
//! # Why JSON
//!
//! The obvious choice for an operator-facing config file is YAML, and
//! this deliberately is not it. Every YAML crate available to this
//! project is either deprecated by its own author (`serde_yaml`,
//! published as `0.9.34+deprecated`), a fork with a handful of releases,
//! or below `0.1.0`. Taking a semi-abandoned parser as a permanent
//! dependency — for files this project already fully controls the schema
//! of — is a worse trade than asking operators to write JSON, which
//! `serde_json` already in the tree handles.
//!
//! [`PolicyDocument`] is a plain data structure with no format
//! knowledge, so if a maintained YAML crate appears, adding
//! `from_yaml` beside [`PolicyDocument::from_json`] is the whole change.
//! See docs/architecture/decisions/0008-detection-policy-configuration.md.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::input::{AddressFamily, MetricKind, ScopeType, TrafficDirection};
use crate::policy::{
    DetectionPolicy, ExecutionMode, PolicyDraft, PolicyError, PolicySelector, Severity,
    TenantPrefixes, Thresholds, DEFAULT_CLEAR_PERCENT,
};
use crate::precedence::PolicySet;

/// The document schema this build understands. A document declaring
/// anything else is refused rather than guessed at.
pub const POLICY_SCHEMA_VERSION: u32 = 1;

/// Largest document accepted, before parsing.
///
/// Checked on the raw text so a hostile file cannot force an allocation
/// the size of itself. Four mebibytes is roughly forty thousand
/// policies — far past any plausible deployment.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

/// Largest number of policies accepted from one document.
pub const MAX_POLICIES: usize = 10_000;

/// Largest number of tenant prefixes accepted from one document.
pub const MAX_TENANT_PREFIXES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("policy document is {size} bytes, above the {MAX_DOCUMENT_BYTES}-byte limit")]
    TooLarge { size: usize },
    #[error("policy document is not valid JSON: {0}")]
    Malformed(String),
    #[error(
        "policy document declares schemaVersion {found}, but this build understands \
         {POLICY_SCHEMA_VERSION}"
    )]
    UnsupportedSchema { found: u32 },
    #[error("policy document declares {count} policies, above the {MAX_POLICIES} limit")]
    TooManyPolicies { count: usize },
    #[error(
        "policy document declares {count} tenant prefixes, above the {MAX_TENANT_PREFIXES} limit"
    )]
    TooManyPrefixes { count: usize },
    #[error("policy {id:?}: {source}")]
    Invalid {
        id: String,
        #[source]
        source: PolicyError,
    },
    #[error("policy {id:?} field {field}: {detail}")]
    Field {
        id: String,
        field: &'static str,
        detail: String,
    },
    #[error("tenant {tenant:?} prefix {prefix:?}: {detail}")]
    Prefix {
        tenant: String,
        prefix: String,
        detail: String,
    },
    #[error("duplicate policy id {id:?}")]
    DuplicateId { id: String },
}

/// A whole policy document, as written.
///
/// Deliberately dumb: it holds text and numbers, validates nothing, and
/// knows nothing about JSON beyond the derives. Turning it into
/// something the engine can use is [`PolicyDocument::compile`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub defaults: PolicyDefaults,
    #[serde(default)]
    pub tenants: Vec<TenantEntry>,
    pub policies: Vec<PolicyEntry>,
}

/// Values every policy inherits unless it says otherwise.
///
/// Exists because the alternative is repeating `cooldown` on ninety
/// policies and getting it wrong on the ninety-first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyDefaults {
    #[serde(default = "default_clear_percent")]
    pub clear_percent: u8,
    #[serde(default)]
    pub cooldown: Option<String>,
    #[serde(default)]
    pub hold_down: Option<String>,
    #[serde(default)]
    pub event_update_interval: Option<String>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub execution_mode: Option<ExecutionMode>,
}

fn default_clear_percent() -> u8 {
    DEFAULT_CLEAR_PERCENT
}

impl Default for PolicyDefaults {
    fn default() -> Self {
        PolicyDefaults {
            clear_percent: DEFAULT_CLEAR_PERCENT,
            cooldown: None,
            hold_down: None,
            event_update_interval: None,
            severity: None,
            execution_mode: None,
        }
    }
}

/// One tenant's address space, used to reject a policy aimed at a range
/// its tenant has no claim to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantEntry {
    pub tenant: String,
    /// CIDR strings, for example `"203.0.113.0/24"`.
    pub prefixes: Vec<String>,
}

/// One policy, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub tenant: String,
    pub scope_type: ScopeType,
    #[serde(default = "any_selector")]
    pub selector: PolicySelector,
    #[serde(default)]
    pub address_family: Option<AddressFamily>,
    pub direction: TrafficDirection,
    /// A duration with a unit suffix, for example `"5s"`.
    pub window: String,
    /// Metric name to threshold, in canonical units. A value may be a
    /// number or a string with a decimal magnitude suffix.
    pub thresholds: BTreeMap<MetricKind, Magnitude>,
    #[serde(default)]
    pub clear_percent: Option<u8>,
    pub trigger_for: String,
    pub clear_for: String,
    #[serde(default)]
    pub cooldown: Option<String>,
    #[serde(default)]
    pub hold_down: Option<String>,
    #[serde(default)]
    pub event_update_interval: Option<String>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub execution_mode: Option<ExecutionMode>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default = "first_version")]
    pub version: u32,
}

fn enabled_by_default() -> bool {
    true
}

fn any_selector() -> PolicySelector {
    PolicySelector::Any
}

fn first_version() -> u32 {
    1
}

/// A threshold, written either as a plain number or as a string with a
/// decimal magnitude suffix.
///
/// `"10G"` is ten billion — decimal, not binary, because the units these
/// multiply are bits and packets per second, which are decimal
/// everywhere else in networking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Magnitude {
    Number(u64),
    Text(String),
}

impl Magnitude {
    /// The value in canonical units.
    pub fn resolve(&self) -> Result<u64, String> {
        match self {
            Magnitude::Number(value) => Ok(*value),
            Magnitude::Text(text) => parse_magnitude(text),
        }
    }
}

/// What a document compiles to.
#[derive(Debug, Clone)]
pub struct CompiledPolicies {
    pub policies: PolicySet,
    pub tenant_prefixes: TenantPrefixes,
}

impl PolicyDocument {
    /// Parses a document, refusing an oversized or unknown-schema one
    /// before it can allocate.
    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        if text.len() > MAX_DOCUMENT_BYTES {
            return Err(ConfigError::TooLarge { size: text.len() });
        }
        let document: PolicyDocument =
            serde_json::from_str(text).map_err(|e| ConfigError::Malformed(e.to_string()))?;
        if document.schema_version != POLICY_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema {
                found: document.schema_version,
            });
        }
        if document.policies.len() > MAX_POLICIES {
            return Err(ConfigError::TooManyPolicies {
                count: document.policies.len(),
            });
        }
        Ok(document)
    }

    /// Validates every policy and builds the set the engine runs on.
    ///
    /// Stops at the first invalid policy rather than collecting every
    /// error. A partially valid policy file must never be half-loaded:
    /// the thresholds an operator ends up running would be neither what
    /// they wrote nor what was there before.
    pub fn compile(&self) -> Result<CompiledPolicies, ConfigError> {
        let tenant_prefixes = self.tenant_prefixes()?;
        let mut policies = Vec::with_capacity(self.policies.len());
        let mut seen: BTreeMap<&str, ()> = BTreeMap::new();

        for entry in &self.policies {
            if seen.insert(entry.id.as_str(), ()).is_some() {
                return Err(ConfigError::DuplicateId {
                    id: entry.id.clone(),
                });
            }
            let draft = entry.to_draft(&self.defaults)?;
            let policy =
                draft
                    .validate(&tenant_prefixes)
                    .map_err(|source| ConfigError::Invalid {
                        id: entry.id.clone(),
                        source,
                    })?;
            policies.push(policy);
        }

        let policies = PolicySet::from_policies(policies).map_err(|source| match &source {
            PolicyError::DuplicateId { id } => ConfigError::DuplicateId { id: id.clone() },
            _ => ConfigError::Invalid {
                id: String::new(),
                source,
            },
        })?;

        Ok(CompiledPolicies {
            policies,
            tenant_prefixes,
        })
    }

    fn tenant_prefixes(&self) -> Result<TenantPrefixes, ConfigError> {
        let total: usize = self.tenants.iter().map(|t| t.prefixes.len()).sum();
        if total > MAX_TENANT_PREFIXES {
            return Err(ConfigError::TooManyPrefixes { count: total });
        }
        let mut prefixes = TenantPrefixes::new();
        for tenant in &self.tenants {
            for raw in &tenant.prefixes {
                let (addr, len) = parse_cidr(raw).map_err(|detail| ConfigError::Prefix {
                    tenant: tenant.tenant.clone(),
                    prefix: raw.clone(),
                    detail,
                })?;
                prefixes.insert(tenant.tenant.clone(), addr, len);
            }
        }
        Ok(prefixes)
    }
}

/// Parses and compiles in one step.
pub fn load_policies(text: &str) -> Result<CompiledPolicies, ConfigError> {
    PolicyDocument::from_json(text)?.compile()
}

impl PolicyEntry {
    fn to_draft(&self, defaults: &PolicyDefaults) -> Result<PolicyDraft, ConfigError> {
        let duration = |field: &'static str, text: &str| {
            parse_duration(text).map_err(|detail| ConfigError::Field {
                id: self.id.clone(),
                field,
                detail,
            })
        };
        let optional = |field: &'static str,
                        own: &Option<String>,
                        fallback: &Option<String>|
         -> Result<Duration, ConfigError> {
            match own.as_ref().or(fallback.as_ref()) {
                Some(text) => parse_duration(text).map_err(|detail| ConfigError::Field {
                    id: self.id.clone(),
                    field,
                    detail,
                }),
                None => Ok(Duration::ZERO),
            }
        };

        let mut thresholds = Thresholds::new();
        for (metric, magnitude) in &self.thresholds {
            let value = magnitude.resolve().map_err(|detail| ConfigError::Field {
                id: self.id.clone(),
                field: "thresholds",
                detail: format!("{}: {detail}", metric.as_str()),
            })?;
            thresholds = thresholds.with(*metric, value);
        }

        Ok(PolicyDraft {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            enabled: self.enabled,
            tenant: self.tenant.clone(),
            scope_type: self.scope_type,
            selector: self.selector.clone(),
            address_family: self.address_family,
            direction: self.direction,
            window: duration("window", &self.window)?,
            thresholds,
            clear_percent: self.clear_percent.unwrap_or(defaults.clear_percent),
            trigger_for: duration("triggerFor", &self.trigger_for)?,
            clear_for: duration("clearFor", &self.clear_for)?,
            cooldown: optional("cooldown", &self.cooldown, &defaults.cooldown)?,
            hold_down: optional("holdDown", &self.hold_down, &defaults.hold_down)?,
            event_update_interval: match self
                .event_update_interval
                .as_ref()
                .or(defaults.event_update_interval.as_ref())
            {
                Some(text) => duration("eventUpdateInterval", text)?,
                // Zero is rejected by policy validation, so a document
                // that sets neither the policy's nor the defaults' value
                // gets a minute rather than a validation failure about a
                // field it never mentioned.
                None => Duration::from_secs(60),
            },
            severity: self
                .severity
                .or(defaults.severity)
                .unwrap_or(Severity::Major),
            execution_mode: self
                .execution_mode
                .or(defaults.execution_mode)
                // A policy that does not say what it may do gets the
                // most restrictive mode that still produces an event.
                .unwrap_or(ExecutionMode::AlertOnly),
            priority: self.priority,
            labels: self.labels.clone(),
            version: self.version,
        })
    }
}

/// Turns `"250ms"`, `"30s"`, `"5m"`, `"2h"`, or `"1d"` into a
/// [`Duration`].
///
/// A bare number is refused. `triggerFor: 300` reads as five minutes to
/// one operator and three hundred milliseconds to another, and both are
/// plausible values — so neither is guessed at.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("is empty".to_string());
    }
    let split = trimmed.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| {
        format!("{trimmed:?} has no unit; write \"{trimmed}s\" or \"{trimmed}ms\"")
    })?;
    if split == 0 {
        return Err(format!("{trimmed:?} does not start with a number"));
    }
    let (digits, unit) = trimmed.split_at(split);
    let value: u64 = digits
        .parse()
        .map_err(|_| format!("{digits:?} is not a whole number"))?;
    let millis = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        other => return Err(format!("unit {other:?} is not one of ms, s, m, h, d")),
    };
    value
        .checked_mul(millis)
        .map(Duration::from_millis)
        .ok_or_else(|| format!("{trimmed:?} overflows"))
}

/// Turns `"10G"`, `"500M"`, `"1k"`, or `"250"` into a plain number.
///
/// Decimal multipliers: `k` is a thousand, not 1024. The units these
/// scale are bits and packets per second, which are decimal everywhere
/// else in networking, and an operator who writes `"10G"` for a ten
/// gigabit link means ten billion.
pub fn parse_magnitude(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("is empty".to_string());
    }
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if split == 0 {
        return Err(format!("{trimmed:?} does not start with a number"));
    }
    let (digits, unit) = trimmed.split_at(split);
    let value: u64 = digits
        .parse()
        .map_err(|_| format!("{digits:?} is not a whole number"))?;
    let multiplier: u64 = match unit {
        "" => 1,
        "k" | "K" => 1_000,
        "M" => 1_000_000,
        "G" => 1_000_000_000,
        "T" => 1_000_000_000_000,
        other => return Err(format!("suffix {other:?} is not one of k, M, G, T")),
    };
    value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("{trimmed:?} overflows"))
}

/// Splits `"203.0.113.0/24"` into an address and a prefix length.
fn parse_cidr(text: &str) -> Result<(IpAddr, u8), String> {
    let (addr, len) = text
        .split_once('/')
        .ok_or_else(|| "is not in address/length form".to_string())?;
    let addr: IpAddr = addr
        .trim()
        .parse()
        .map_err(|_| format!("{addr:?} is not an IP address"))?;
    let len: u8 = len
        .trim()
        .parse()
        .map_err(|_| format!("{len:?} is not a prefix length"))?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    if len > max {
        return Err(format!("prefix length {len} exceeds {max} for this family"));
    }
    Ok((addr, len))
}

/// Renders the policies back out as a document, for a diagnostic
/// endpoint that answers "what is actually loaded right now".
pub fn to_document(policies: &[DetectionPolicy]) -> PolicyDocument {
    PolicyDocument {
        schema_version: POLICY_SCHEMA_VERSION,
        defaults: PolicyDefaults::default(),
        tenants: Vec::new(),
        policies: policies
            .iter()
            .map(|policy| PolicyEntry {
                id: policy.id.clone(),
                name: policy.name.clone(),
                description: policy.description.clone(),
                enabled: policy.enabled,
                tenant: policy.tenant.clone(),
                scope_type: policy.scope_type,
                selector: policy.selector.clone(),
                address_family: policy.address_family,
                direction: policy.direction,
                window: render_duration(policy.window),
                thresholds: policy
                    .thresholds
                    .iter()
                    .map(|(metric, value)| (metric, Magnitude::Number(value)))
                    .collect(),
                clear_percent: Some(policy.clear_percent.get()),
                trigger_for: render_duration(policy.trigger_for),
                clear_for: render_duration(policy.clear_for),
                cooldown: Some(render_duration(policy.cooldown)),
                hold_down: Some(render_duration(policy.hold_down)),
                event_update_interval: Some(render_duration(policy.event_update_interval)),
                severity: Some(policy.severity),
                execution_mode: Some(policy.execution_mode),
                priority: policy.priority,
                labels: policy.labels.clone(),
                version: policy.version,
            })
            .collect(),
    }
}

/// The shortest exact spelling of a duration, so a round trip through
/// the document does not turn `"5m"` into `"300000ms"`.
fn render_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    for (unit, size) in [
        ("d", 86_400_000u128),
        ("h", 3_600_000),
        ("m", 60_000),
        ("s", 1_000),
    ] {
        if millis != 0 && millis.is_multiple_of(size) {
            return format!("{}{unit}", millis / size);
        }
    }
    format!("{millis}ms")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
      "schemaVersion": 1,
      "policies": [
        {
          "id": "p-host-bps",
          "name": "host inbound bps",
          "tenant": "acme",
          "scopeType": "host",
          "direction": "incoming",
          "window": "5s",
          "thresholds": { "bps": "1G" },
          "triggerFor": "15s",
          "clearFor": "30s"
        }
      ]
    }"#;

    #[test]
    fn a_minimal_document_compiles() {
        let compiled = load_policies(MINIMAL).expect("compiles");
        assert_eq!(compiled.policies.len(), 1);
        let policy = compiled.policies.get("p-host-bps").expect("present");
        assert_eq!(policy.window, Duration::from_secs(5));
        assert_eq!(policy.trigger_for, Duration::from_secs(15));
        assert_eq!(policy.clear_for, Duration::from_secs(30));
        assert_eq!(policy.thresholds.get(MetricKind::Bps), Some(1_000_000_000));
        assert_eq!(policy.clear_percent.get(), DEFAULT_CLEAR_PERCENT);
        assert_eq!(policy.severity, Severity::Major);
        assert_eq!(policy.execution_mode, ExecutionMode::AlertOnly);
        assert!(policy.enabled);
        assert_eq!(policy.version, 1);
        assert_eq!(policy.selector, PolicySelector::Any);
    }

    #[test]
    fn a_misspelt_field_is_refused_rather_than_ignored() {
        let text = MINIMAL.replace("\"triggerFor\"", "\"trigger_for\"");
        let error = load_policies(&text).expect_err("must not load");
        let message = error.to_string();
        assert!(message.contains("trigger_for"), "{message}");
    }

    #[test]
    fn an_unknown_top_level_field_is_refused() {
        let text = MINIMAL.replace("\"policies\":", "\"policiez\": [], \"policies\":");
        let error = load_policies(&text).expect_err("must not load");
        assert!(matches!(error, ConfigError::Malformed(_)));
    }

    #[test]
    fn a_future_schema_version_is_refused_not_guessed_at() {
        let text = MINIMAL.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        assert_eq!(
            load_policies(&text).err(),
            Some(ConfigError::UnsupportedSchema { found: 2 })
        );
    }

    #[test]
    fn an_oversized_document_is_refused_before_parsing() {
        let text = " ".repeat(MAX_DOCUMENT_BYTES + 1);
        assert_eq!(
            PolicyDocument::from_json(&text),
            Err(ConfigError::TooLarge {
                size: MAX_DOCUMENT_BYTES + 1
            })
        );
    }

    #[test]
    fn defaults_are_inherited_and_overridable() {
        let text = r#"{
          "schemaVersion": 1,
          "defaults": {
            "clearPercent": 50,
            "cooldown": "10m",
            "severity": "critical",
            "executionMode": "observe"
          },
          "policies": [
            {
              "id": "inherits",
              "name": "a",
              "tenant": "acme",
              "scopeType": "host",
              "direction": "incoming",
              "window": "5s",
              "thresholds": { "bps": 1000 },
              "triggerFor": "15s",
              "clearFor": "30s"
            },
            {
              "id": "overrides",
              "name": "b",
              "tenant": "acme",
              "scopeType": "host",
              "direction": "incoming",
              "window": "5s",
              "thresholds": { "bps": 1000 },
              "triggerFor": "15s",
              "clearFor": "30s",
              "clearPercent": 90,
              "cooldown": "1m",
              "severity": "info",
              "executionMode": "alertOnly"
            }
          ]
        }"#;
        let compiled = load_policies(text).expect("compiles");
        let inherits = compiled.policies.get("inherits").expect("present");
        assert_eq!(inherits.clear_percent.get(), 50);
        assert_eq!(inherits.cooldown, Duration::from_secs(600));
        assert_eq!(inherits.severity, Severity::Critical);
        assert_eq!(inherits.execution_mode, ExecutionMode::Observe);

        let overrides = compiled.policies.get("overrides").expect("present");
        assert_eq!(overrides.clear_percent.get(), 90);
        assert_eq!(overrides.cooldown, Duration::from_secs(60));
        assert_eq!(overrides.severity, Severity::Info);
        assert_eq!(overrides.execution_mode, ExecutionMode::AlertOnly);
    }

    #[test]
    fn a_selector_and_tenant_prefixes_are_read() {
        let text = r#"{
          "schemaVersion": 1,
          "tenants": [
            { "tenant": "acme", "prefixes": ["203.0.113.0/24", "2001:db8::/32"] }
          ],
          "policies": [
            {
              "id": "p-net",
              "name": "network",
              "tenant": "acme",
              "scopeType": "prefix",
              "selector": { "kind": "network", "addr": "203.0.113.0", "prefixLen": 24 },
              "direction": "incoming",
              "window": "5s",
              "thresholds": { "pps": "500k" },
              "triggerFor": "15s",
              "clearFor": "30s"
            }
          ]
        }"#;
        let compiled = load_policies(text).expect("compiles");
        let policy = compiled.policies.get("p-net").expect("present");
        assert_eq!(
            policy.selector,
            PolicySelector::Network {
                addr: "203.0.113.0".parse().expect("valid"),
                prefix_len: 24
            }
        );
        assert_eq!(policy.thresholds.get(MetricKind::Pps), Some(500_000));
        assert!(compiled
            .tenant_prefixes
            .covers("acme", "203.0.113.0".parse().expect("valid"), 24));
    }

    #[test]
    fn a_policy_aimed_outside_its_tenant_is_refused() {
        let text = r#"{
          "schemaVersion": 1,
          "tenants": [ { "tenant": "acme", "prefixes": ["203.0.113.0/24"] } ],
          "policies": [
            {
              "id": "p-wrong",
              "name": "elsewhere",
              "tenant": "acme",
              "scopeType": "prefix",
              "selector": { "kind": "network", "addr": "198.51.100.0", "prefixLen": 24 },
              "direction": "incoming",
              "window": "5s",
              "thresholds": { "bps": 1000 },
              "triggerFor": "15s",
              "clearFor": "30s"
            }
          ]
        }"#;
        let error = load_policies(text).expect_err("must not load");
        assert!(matches!(error, ConfigError::Invalid { .. }), "{error}");
    }

    #[test]
    fn a_duplicate_policy_id_is_refused() {
        let text = MINIMAL.replace(
            "\"policies\": [",
            "\"policies\": [ { \"id\": \"p-host-bps\", \"name\": \"dup\", \"tenant\": \"acme\", \
             \"scopeType\": \"host\", \"direction\": \"incoming\", \"window\": \"5s\", \
             \"thresholds\": { \"bps\": 1000 }, \"triggerFor\": \"15s\", \"clearFor\": \"30s\" },",
        );
        assert_eq!(
            load_policies(&text).err(),
            Some(ConfigError::DuplicateId {
                id: "p-host-bps".to_string()
            })
        );
    }

    #[test]
    fn an_invalid_policy_stops_the_whole_load() {
        // triggerFor shorter than the window: a policy that can never fire.
        let text = MINIMAL.replace("\"triggerFor\": \"15s\"", "\"triggerFor\": \"1s\"");
        let error = load_policies(&text).expect_err("must not load");
        assert!(matches!(error, ConfigError::Invalid { .. }), "{error}");
    }

    #[test]
    fn a_bad_prefix_names_the_tenant_and_the_prefix() {
        let text = r#"{
          "schemaVersion": 1,
          "tenants": [ { "tenant": "acme", "prefixes": ["203.0.113.0/99"] } ],
          "policies": []
        }"#;
        let error = load_policies(text).expect_err("must not load");
        let message = error.to_string();
        assert!(message.contains("acme"), "{message}");
        assert!(message.contains("203.0.113.0/99"), "{message}");
    }

    #[test]
    fn a_duration_without_a_unit_is_refused() {
        let text = MINIMAL.replace("\"triggerFor\": \"15s\"", "\"triggerFor\": \"15\"");
        let error = load_policies(&text).expect_err("must not load");
        let message = error.to_string();
        assert!(message.contains("no unit"), "{message}");
        assert!(message.contains("triggerFor"), "{message}");
    }

    #[test]
    fn durations_parse_every_supported_unit() {
        assert_eq!(parse_duration("250ms"), Ok(Duration::from_millis(250)));
        assert_eq!(parse_duration("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(parse_duration("2h"), Ok(Duration::from_secs(7200)));
        assert_eq!(parse_duration("1d"), Ok(Duration::from_secs(86_400)));
        assert_eq!(parse_duration("  10s  "), Ok(Duration::from_secs(10)));
    }

    #[test]
    fn a_bad_duration_explains_itself() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("s").is_err());
        assert!(parse_duration("10 s").is_err());
        assert!(parse_duration("10weeks").is_err());
        assert!(parse_duration("18446744073709551615d").is_err());
    }

    #[test]
    fn magnitudes_parse_decimal_suffixes() {
        assert_eq!(parse_magnitude("250"), Ok(250));
        assert_eq!(parse_magnitude("1k"), Ok(1_000));
        assert_eq!(parse_magnitude("1K"), Ok(1_000));
        assert_eq!(parse_magnitude("500M"), Ok(500_000_000));
        assert_eq!(parse_magnitude("10G"), Ok(10_000_000_000));
        assert_eq!(parse_magnitude("1T"), Ok(1_000_000_000_000));
    }

    #[test]
    fn a_bad_magnitude_explains_itself() {
        assert!(parse_magnitude("").is_err());
        assert!(parse_magnitude("Gb").is_err());
        assert!(parse_magnitude("10Gi").is_err());
        assert!(parse_magnitude("18446744073709551615T").is_err());
    }

    #[test]
    fn a_threshold_may_be_a_number_or_a_string() {
        assert_eq!(Magnitude::Number(1000).resolve(), Ok(1000));
        assert_eq!(Magnitude::Text("1k".to_string()).resolve(), Ok(1000));
    }

    #[test]
    fn a_document_round_trips_through_json() {
        let compiled = load_policies(MINIMAL).expect("compiles");
        let policies: Vec<_> = compiled.policies.iter().cloned().collect();
        let document = to_document(&policies);
        let text = serde_json::to_string(&document).expect("serializes");
        let back = load_policies(&text).expect("recompiles");
        assert_eq!(back.policies.len(), compiled.policies.len());
        let original = compiled.policies.get("p-host-bps").expect("present");
        let reloaded = back.policies.get("p-host-bps").expect("present");
        assert_eq!(original, reloaded);
    }

    #[test]
    fn durations_render_in_their_shortest_exact_form() {
        assert_eq!(render_duration(Duration::from_secs(300)), "5m");
        assert_eq!(render_duration(Duration::from_secs(5)), "5s");
        assert_eq!(render_duration(Duration::from_millis(250)), "250ms");
        assert_eq!(render_duration(Duration::from_secs(86_400)), "1d");
        assert_eq!(render_duration(Duration::from_secs(7_200)), "2h");
        assert_eq!(render_duration(Duration::ZERO), "0ms");
    }

    #[test]
    fn an_empty_policy_list_compiles_to_an_empty_set() {
        let text = r#"{ "schemaVersion": 1, "policies": [] }"#;
        let compiled = load_policies(text).expect("compiles");
        assert!(compiled.policies.is_empty());
    }

    #[test]
    fn malformed_json_is_reported_not_panicked_on() {
        assert!(matches!(
            load_policies("{ not json"),
            Err(ConfigError::Malformed(_))
        ));
        assert!(matches!(load_policies(""), Err(ConfigError::Malformed(_))));
    }

    #[test]
    fn a_disabled_policy_loads_and_stays_disabled() {
        let text = MINIMAL.replace(
            "\"name\": \"host inbound bps\"",
            "\"name\": \"x\", \"enabled\": false",
        );
        let compiled = load_policies(&text).expect("compiles");
        assert!(
            !compiled
                .policies
                .get("p-host-bps")
                .expect("present")
                .enabled
        );
    }
}
