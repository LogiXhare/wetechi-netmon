//! Collector configuration.
//!
//! Phase 2 deliberately uses plain environment variables rather than a
//! config file/service: `crates/configuration` (the real Configuration
//! Service, per docs/functional-requirements.md FR-15.1) is not built
//! yet, and inventing a bespoke config-file format here would just be
//! thrown away when that lands. This is a documented, intentional MVP
//! simplification, not an oversight.

use std::net::SocketAddr;

const DEFAULT_BIND: &str = "0.0.0.0:2055";
const DEFAULT_METRICS_BIND: &str = "0.0.0.0:9090";

const BIND_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_BIND";
const METRICS_BIND_ENV_VAR: &str = "WETECHINETMON_COLLECTOR_METRICS_BIND";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// UDP address the collector listens for IPFIX datagrams on.
    /// Reference lab default port is 2055 (see docs/product-charter.md /
    /// the reference deployment in `prompts/CLAUDE_MASTER_PROMPT.md` §4)
    /// — never hardcoded as anything other than a default, per NFR-7.
    pub bind: SocketAddr,
    /// TCP address the Prometheus `/metrics` endpoint is served on.
    pub metrics_bind: SocketAddr,
}

impl Config {
    /// Reads configuration from environment variables, falling back to
    /// documented defaults. An invalid (unparseable) value is treated as
    /// a startup error rather than silently falling back — a typo'd bind
    /// address should fail loudly, not silently listen on the wrong
    /// interface.
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind = parse_env_or_default(BIND_ENV_VAR, DEFAULT_BIND)?;
        let metrics_bind = parse_env_or_default(METRICS_BIND_ENV_VAR, DEFAULT_METRICS_BIND)?;
        Ok(Config { bind, metrics_bind })
    }
}

fn parse_env_or_default(var: &str, default: &str) -> Result<SocketAddr, ConfigError> {
    match std::env::var(var) {
        Ok(value) => value.parse().map_err(|_| ConfigError::InvalidSocketAddr {
            var: var.to_string(),
            value,
        }),
        Err(std::env::VarError::NotPresent) => Ok(default
            .parse()
            .expect("DEFAULT_BIND/DEFAULT_METRICS_BIND constants must be valid addresses")),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidSocketAddr {
            var: var.to_string(),
            value: "<non-utf8>".to_string(),
        }),
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{var} is set to '{value}', which is not a valid host:port address")]
    InvalidSocketAddr { var: String, value: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // std::env::set_var affects the whole process; serialize these tests
    // so they don't race each other's environment mutations.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn defaults_are_used_when_env_vars_are_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(BIND_ENV_VAR);
        std::env::remove_var(METRICS_BIND_ENV_VAR);

        let config = Config::from_env().unwrap();
        assert_eq!(config.bind, DEFAULT_BIND.parse().unwrap());
        assert_eq!(config.metrics_bind, DEFAULT_METRICS_BIND.parse().unwrap());
    }

    #[test]
    fn env_vars_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(BIND_ENV_VAR, "127.0.0.1:2100");
        std::env::set_var(METRICS_BIND_ENV_VAR, "127.0.0.1:9191");

        let config = Config::from_env().unwrap();
        assert_eq!(config.bind, "127.0.0.1:2100".parse().unwrap());
        assert_eq!(config.metrics_bind, "127.0.0.1:9191".parse().unwrap());

        std::env::remove_var(BIND_ENV_VAR);
        std::env::remove_var(METRICS_BIND_ENV_VAR);
    }

    #[test]
    fn invalid_bind_address_is_a_startup_error_not_a_silent_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(BIND_ENV_VAR, "not-an-address");

        let result = Config::from_env();
        assert_eq!(
            result,
            Err(ConfigError::InvalidSocketAddr {
                var: BIND_ENV_VAR.to_string(),
                value: "not-an-address".to_string(),
            })
        );

        std::env::remove_var(BIND_ENV_VAR);
    }
}
