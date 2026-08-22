//! Collector configuration.
//!
//! Phase 2/3 deliberately use plain environment variables rather than a
//! config file/service: `crates/configuration` (the real Configuration
//! Service, per docs/functional-requirements.md FR-15.1) is not built
//! yet, and inventing a bespoke config-file format here would just be
//! thrown away when that lands. This is a documented, intentional MVP
//! simplification, not an oversight. Full docs:
//! docs/configuration/aggregation.md, docs/configuration/prefixes.md.

use std::net::SocketAddr;

use wetechinetmon_classifier::PrefixConfigEntry;

const DEFAULT_BIND: &str = "0.0.0.0:2055";
const DEFAULT_METRICS_BIND: &str = "0.0.0.0:9090";
const DEFAULT_QUEUE_CAPACITY: usize = 10_000;
const DEFAULT_MAX_HOSTS: usize = 100_000;
const DEFAULT_MAX_NETWORKS: usize = 50_000;
const DEFAULT_MAX_HOSTGROUPS: usize = 1_000;
const DEFAULT_MAX_ASNS: usize = 10_000;
const DEFAULT_INACTIVITY_TTL_SECS: u64 = 300;
const DEFAULT_DETECTION_WINDOW_SECS: u64 = 5;
const DEFAULT_DETECTION_MAX_SCOPES: usize = 100_000;
const DEFAULT_DETECTION_EVENT_BUFFER: usize = 10_000;
const DEFAULT_DETECTION_STALE_SECS: u64 = 180;

const BIND_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_BIND";
const METRICS_BIND_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_METRICS_BIND";
const QUEUE_CAPACITY_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_QUEUE_CAPACITY";
const LOCAL_PREFIXES_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_LOCAL_PREFIXES";
const MAX_HOSTS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_MAX_HOSTS";
const MAX_NETWORKS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_MAX_NETWORKS";
const MAX_HOSTGROUPS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_MAX_HOSTGROUPS";
const MAX_ASNS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_MAX_ASNS";
const INACTIVITY_TTL_SECS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_INACTIVITY_TTL_SECS";
const SAMPLING_GLOBAL_DEFAULT_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_SAMPLING_GLOBAL_DEFAULT";
const CLICKHOUSE_URL_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_CLICKHOUSE_URL";
const DETECTION_POLICY_FILE_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_DETECTION_POLICY_FILE";
const DETECTION_WINDOW_SECS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_DETECTION_WINDOW_SECS";
const DETECTION_MAX_SCOPES_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_DETECTION_MAX_SCOPES";
const DETECTION_EVENT_BUFFER_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_DETECTION_EVENT_BUFFER";
const DETECTION_STALE_SECS_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_DETECTION_STALE_SECS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// UDP address the collector listens for IPFIX datagrams on.
    /// Reference lab default port is 2055 (see docs/product-charter.md /
    /// the reference deployment in `prompts/CLAUDE_MASTER_PROMPT.md` §4)
    /// — never hardcoded as anything other than a default, per NFR-7.
    pub bind: SocketAddr,
    /// TCP address the Prometheus `/metrics` endpoint is served on.
    pub metrics_bind: SocketAddr,
    /// Bounded capacity of the in-process channel between the UDP
    /// receive loop and the classify/aggregate stage — this is also the
    /// backpressure control (ADR 0004) and the `queue_depth` metric's
    /// ceiling.
    pub queue_capacity: usize,
    /// Local-prefix registry entries, per docs/configuration/prefixes.md.
    pub local_prefixes: Vec<PrefixConfigEntry>,
    pub max_hosts: usize,
    pub max_networks: usize,
    pub max_hostgroups: usize,
    pub max_asns: usize,
    pub inactivity_ttl_secs: u64,
    /// Global-default sampling rate (lowest priority tier — see
    /// docs/architecture/aggregation.md sampling-correction section).
    pub sampling_global_default: Option<u32>,
    /// ClickHouse HTTP URL (e.g. `http://localhost:8123`). ClickHouse
    /// export is entirely disabled when unset — see
    /// docs/integrations/clickhouse.md.
    pub clickhouse_url: Option<String>,
    /// Path to a detection policy document. **Detection is entirely off
    /// when unset** — no policies means nothing to detect against, and
    /// silently detecting nothing is better expressed as detecting
    /// nothing on purpose. See docs/configuration/detection-policies.md.
    pub detection_policy_file: Option<String>,
    /// How long the detector accumulates per-scope counters before
    /// evaluating them. Must match the `window` of the policies loaded.
    pub detection_window_secs: u64,
    /// Cap on scopes tracked per dimension, and on detection states.
    pub detection_max_scopes: usize,
    /// How many detection events may wait for the ClickHouse export
    /// tick before the oldest is dropped.
    pub detection_event_buffer: usize,
    /// How long an open detection may go without a snapshot before it is
    /// force-closed as stale.
    pub detection_stale_secs: u64,
}

impl Config {
    /// Reads configuration from environment variables, falling back to
    /// documented defaults. An invalid (unparseable) value is treated as
    /// a startup error rather than silently falling back — a typo'd bind
    /// address should fail loudly, not silently listen on the wrong
    /// interface.
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind = parse_socket_addr_or_default(BIND_ENV_VAR, DEFAULT_BIND)?;
        let metrics_bind =
            parse_socket_addr_or_default(METRICS_BIND_ENV_VAR, DEFAULT_METRICS_BIND)?;
        let queue_capacity =
            parse_usize_or_default(QUEUE_CAPACITY_ENV_VAR, DEFAULT_QUEUE_CAPACITY)?;
        let max_hosts = parse_usize_or_default(MAX_HOSTS_ENV_VAR, DEFAULT_MAX_HOSTS)?;
        let max_networks = parse_usize_or_default(MAX_NETWORKS_ENV_VAR, DEFAULT_MAX_NETWORKS)?;
        let max_hostgroups =
            parse_usize_or_default(MAX_HOSTGROUPS_ENV_VAR, DEFAULT_MAX_HOSTGROUPS)?;
        let max_asns = parse_usize_or_default(MAX_ASNS_ENV_VAR, DEFAULT_MAX_ASNS)?;
        let inactivity_ttl_secs =
            parse_u64_or_default(INACTIVITY_TTL_SECS_ENV_VAR, DEFAULT_INACTIVITY_TTL_SECS)?;
        let sampling_global_default = parse_optional_u32(SAMPLING_GLOBAL_DEFAULT_ENV_VAR)?;
        let local_prefixes = parse_local_prefixes(LOCAL_PREFIXES_ENV_VAR)?;
        let clickhouse_url = env_value(CLICKHOUSE_URL_ENV_VAR)?;
        let detection_policy_file = env_value(DETECTION_POLICY_FILE_ENV_VAR)?;
        let detection_window_secs =
            parse_u64_or_default(DETECTION_WINDOW_SECS_ENV_VAR, DEFAULT_DETECTION_WINDOW_SECS)?;
        let detection_max_scopes =
            parse_usize_or_default(DETECTION_MAX_SCOPES_ENV_VAR, DEFAULT_DETECTION_MAX_SCOPES)?;
        let detection_event_buffer = parse_usize_or_default(
            DETECTION_EVENT_BUFFER_ENV_VAR,
            DEFAULT_DETECTION_EVENT_BUFFER,
        )?;
        let detection_stale_secs =
            parse_u64_or_default(DETECTION_STALE_SECS_ENV_VAR, DEFAULT_DETECTION_STALE_SECS)?;

        Ok(Config {
            bind,
            metrics_bind,
            queue_capacity,
            local_prefixes,
            max_hosts,
            max_networks,
            max_hostgroups,
            max_asns,
            inactivity_ttl_secs,
            sampling_global_default,
            clickhouse_url,
            detection_policy_file,
            detection_window_secs,
            detection_max_scopes,
            detection_event_buffer,
            detection_stale_secs,
        })
    }
}

fn parse_socket_addr_or_default(var: &str, default: &str) -> Result<SocketAddr, ConfigError> {
    match env_value(var)? {
        Some(value) => value.parse().map_err(|_| ConfigError::InvalidValue {
            var: var.to_string(),
            value,
            expected: "a host:port address",
        }),
        None => Ok(default
            .parse()
            .expect("default socket address constants must be valid")),
    }
}

fn parse_usize_or_default(var: &str, default: usize) -> Result<usize, ConfigError> {
    match env_value(var)? {
        Some(value) => value.parse().map_err(|_| ConfigError::InvalidValue {
            var: var.to_string(),
            value,
            expected: "a non-negative integer",
        }),
        None => Ok(default),
    }
}

fn parse_u64_or_default(var: &str, default: u64) -> Result<u64, ConfigError> {
    match env_value(var)? {
        Some(value) => value.parse().map_err(|_| ConfigError::InvalidValue {
            var: var.to_string(),
            value,
            expected: "a non-negative integer",
        }),
        None => Ok(default),
    }
}

fn parse_optional_u32(var: &str) -> Result<Option<u32>, ConfigError> {
    match env_value(var)? {
        Some(value) => value
            .parse()
            .map(Some)
            .map_err(|_| ConfigError::InvalidValue {
                var: var.to_string(),
                value,
                expected: "a positive integer sampling rate",
            }),
        None => Ok(None),
    }
}

/// Parses `WETECHINETMON_COLLECTOR_LOCAL_PREFIXES`: a comma-separated
/// list of `network/prefix_len[@tenant[@hostgroup]]` entries, e.g.
/// `10.0.0.0/8@wetechi@core,2001:db8::/32`. `@` (not `:`) separates the
/// tenant/hostgroup fields specifically because IPv6 addresses contain
/// colons themselves — using `:` as a field separator would make
/// `2001:db8::/32:tenant` ambiguous to split correctly. `tenant` defaults
/// to `"default"` and `hostgroup` to none when omitted. See
/// docs/configuration/prefixes.md for the full reference.
fn parse_local_prefixes(var: &str) -> Result<Vec<PrefixConfigEntry>, ConfigError> {
    let Some(raw) = env_value(var)? else {
        return Ok(Vec::new());
    };
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut segments = part.split('@');
        let network_str = segments.next().unwrap_or_default();
        let tenant = segments.next().unwrap_or("default").to_string();
        let hostgroup = segments.next().map(|s| s.to_string());

        let (addr_str, len_str) =
            network_str
                .split_once('/')
                .ok_or_else(|| ConfigError::InvalidValue {
                    var: var.to_string(),
                    value: raw.clone(),
                    expected: "network/prefix_len[@tenant[@hostgroup]] entries",
                })?;
        let network = addr_str.parse().map_err(|_| ConfigError::InvalidValue {
            var: var.to_string(),
            value: raw.clone(),
            expected: "a valid IPv4 or IPv6 network address",
        })?;
        let prefix_len: u8 = len_str.parse().map_err(|_| ConfigError::InvalidValue {
            var: var.to_string(),
            value: raw.clone(),
            expected: "a valid prefix length",
        })?;

        entries.push(PrefixConfigEntry {
            network,
            prefix_len,
            tenant,
            hostgroup,
        });
    }
    Ok(entries)
}

fn env_value(var: &str) -> Result<Option<String>, ConfigError> {
    match std::env::var(var) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidValue {
            var: var.to_string(),
            value: "<non-utf8>".to_string(),
            expected: "a UTF-8 string",
        }),
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{var} is set to '{value}', which is not valid: expected {expected}")]
    InvalidValue {
        var: String,
        value: String,
        expected: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // std::env::set_var affects the whole process; serialize these tests
    // so they don't race each other's environment mutations.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ALL_ENV_VARS: &[&str] = &[
        BIND_ENV_VAR,
        METRICS_BIND_ENV_VAR,
        QUEUE_CAPACITY_ENV_VAR,
        LOCAL_PREFIXES_ENV_VAR,
        MAX_HOSTS_ENV_VAR,
        MAX_NETWORKS_ENV_VAR,
        MAX_HOSTGROUPS_ENV_VAR,
        MAX_ASNS_ENV_VAR,
        INACTIVITY_TTL_SECS_ENV_VAR,
        SAMPLING_GLOBAL_DEFAULT_ENV_VAR,
        CLICKHOUSE_URL_ENV_VAR,
    ];

    fn clear_all() {
        for var in ALL_ENV_VARS {
            std::env::remove_var(var);
        }
    }

    #[test]
    fn defaults_are_used_when_env_vars_are_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();

        let config = Config::from_env().unwrap();
        assert_eq!(config.bind, DEFAULT_BIND.parse().unwrap());
        assert_eq!(config.metrics_bind, DEFAULT_METRICS_BIND.parse().unwrap());
        assert_eq!(config.queue_capacity, DEFAULT_QUEUE_CAPACITY);
        assert_eq!(config.max_hosts, DEFAULT_MAX_HOSTS);
        assert!(config.local_prefixes.is_empty());
        assert_eq!(config.sampling_global_default, None);
        assert_eq!(config.clickhouse_url, None);

        clear_all();
    }

    #[test]
    fn clickhouse_url_is_read_when_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        std::env::set_var(CLICKHOUSE_URL_ENV_VAR, "http://localhost:8123");

        let config = Config::from_env().unwrap();
        assert_eq!(
            config.clickhouse_url.as_deref(),
            Some("http://localhost:8123")
        );

        clear_all();
    }

    #[test]
    fn env_vars_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        std::env::set_var(BIND_ENV_VAR, "127.0.0.1:2100");
        std::env::set_var(METRICS_BIND_ENV_VAR, "127.0.0.1:9191");
        std::env::set_var(QUEUE_CAPACITY_ENV_VAR, "500");
        std::env::set_var(MAX_HOSTS_ENV_VAR, "10");
        std::env::set_var(SAMPLING_GLOBAL_DEFAULT_ENV_VAR, "100");

        let config = Config::from_env().unwrap();
        assert_eq!(config.bind, "127.0.0.1:2100".parse().unwrap());
        assert_eq!(config.metrics_bind, "127.0.0.1:9191".parse().unwrap());
        assert_eq!(config.queue_capacity, 500);
        assert_eq!(config.max_hosts, 10);
        assert_eq!(config.sampling_global_default, Some(100));

        clear_all();
    }

    #[test]
    fn invalid_bind_address_is_a_startup_error_not_a_silent_fallback() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        std::env::set_var(BIND_ENV_VAR, "not-an-address");

        let result = Config::from_env();
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));

        clear_all();
    }

    #[test]
    fn parses_local_prefixes_with_tenant_and_hostgroup() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        std::env::set_var(
            LOCAL_PREFIXES_ENV_VAR,
            "10.0.0.0/8@wetechi@core,2001:db8::/32",
        );

        let config = Config::from_env().unwrap();
        assert_eq!(config.local_prefixes.len(), 2);
        assert_eq!(config.local_prefixes[0].tenant, "wetechi");
        assert_eq!(config.local_prefixes[0].hostgroup.as_deref(), Some("core"));
        assert_eq!(config.local_prefixes[1].tenant, "default");
        assert_eq!(config.local_prefixes[1].hostgroup, None);

        clear_all();
    }

    #[test]
    fn invalid_prefix_entry_is_a_startup_error() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        std::env::set_var(LOCAL_PREFIXES_ENV_VAR, "not-a-prefix");

        let result = Config::from_env();
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));

        clear_all();
    }
}
