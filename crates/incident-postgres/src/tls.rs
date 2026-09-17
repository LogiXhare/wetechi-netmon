//! The PostgreSQL TLS connector (ADR 0023).
//!
//! What ADR 0023 requires, and where this module enforces it:
//!
//! - **Full verification.** [`rustls_connector`] validates the server's
//!   certificate chain against the CA certificates the operator supplies,
//!   and rustls checks the certificate against the host name being
//!   connected to. There is no switch to turn either off.
//! - **Custom CA through explicit trust.** The trust store holds only the
//!   supplied certificates. An empty or unreadable CA bundle is refused
//!   rather than read as "trust nothing" or "trust anything".
//! - **Client certificates not precluded.** [`TlsSettings::client_identity`]
//!   is optional and off by default.
//! - **Never a silent fallback to plaintext.** [`require_tls_off_loopback`]
//!   refuses a configuration that could connect to a non-loopback host
//!   without TLS, which includes `sslmode=prefer`: that mode falls back to
//!   plaintext when the server declines TLS. A loopback test database may
//!   run without TLS.
//! - **Handshake failure is unavailability.** A connection that fails its
//!   handshake fails like any other unreachable database: the pool reports
//!   it and nothing retries it into plaintext.
//!
//! The crypto provider is named explicitly (aws-lc-rs, rustls's default)
//! rather than read from process-wide state, and the protocol versions are
//! rustls's safe defaults: TLS 1.2 minimum, 1.3 preferred, no override.

use std::net::IpAddr;
use std::sync::Arc;

use deadpool_postgres::Pool;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use tokio_postgres::config::{Host, SslMode};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::error::PersistError;
use crate::pool::{build_pool, PoolPolicy};

/// A client certificate chain and its private key, both PEM.
#[derive(Clone)]
pub struct ClientIdentity {
    pub certificate_chain_pem: Vec<u8>,
    pub private_key_pem: Vec<u8>,
}

impl std::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("private_key_pem", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// What the connector trusts and, optionally, who it presents as. The PEM
/// bytes are read by the caller from operator-managed files; none belong
/// in Git.
#[derive(Debug, Clone)]
pub struct TlsSettings {
    /// One or more CA certificates, PEM. Only these are trusted.
    pub ca_certificates_pem: Vec<u8>,
    /// Mutual TLS. Not required by default.
    pub client_identity: Option<ClientIdentity>,
}

impl TlsSettings {
    pub fn trusting(ca_certificates_pem: impl Into<Vec<u8>>) -> Self {
        TlsSettings {
            ca_certificates_pem: ca_certificates_pem.into(),
            client_identity: None,
        }
    }
}

fn rejected(detail: impl Into<String>) -> PersistError {
    PersistError::TlsConfiguration(detail.into())
}

/// Builds a verifying rustls connector from explicit trust anchors.
pub fn rustls_connector(settings: &TlsSettings) -> Result<MakeRustlsConnect, PersistError> {
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(&settings.ca_certificates_pem) {
        let certificate = certificate
            .map_err(|error| rejected(format!("the CA bundle is not valid PEM: {error}")))?;
        roots
            .add(certificate)
            .map_err(|error| rejected(format!("a CA certificate was refused: {error}")))?;
    }
    if roots.is_empty() {
        return Err(rejected("the CA bundle holds no certificate"));
    }

    let builder = ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| rejected(format!("no safe protocol version: {error}")))?
    .with_root_certificates(roots);

    let config = match &settings.client_identity {
        None => builder.with_no_client_auth(),
        Some(identity) => {
            let chain = CertificateDer::pem_slice_iter(&identity.certificate_chain_pem)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    rejected(format!(
                        "the client certificate chain is not valid PEM: {error}"
                    ))
                })?;
            if chain.is_empty() {
                return Err(rejected(
                    "the client certificate chain holds no certificate",
                ));
            }
            let key = PrivateKeyDer::from_pem_slice(&identity.private_key_pem)
                .map_err(|_| rejected("the client private key is not a readable PEM key"))?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|error| rejected(format!("the client identity was refused: {error}")))?
        }
    };
    Ok(MakeRustlsConnect::new(config))
}

fn is_loopback_name(name: &str) -> bool {
    let name = name.trim_start_matches('[').trim_end_matches(']');
    match name.parse::<IpAddr>() {
        Ok(address) => address.is_loopback(),
        Err(_) => name.eq_ignore_ascii_case("localhost"),
    }
}

/// Whether every place this configuration can connect to is on this host:
/// loopback names and addresses, and Unix sockets.
pub fn is_loopback_only(config: &tokio_postgres::Config) -> bool {
    let hosts_local = config.get_hosts().iter().all(|host| match host {
        Host::Tcp(name) => is_loopback_name(name),
        #[cfg(unix)]
        Host::Unix(_) => true,
    });
    let addresses_local = config
        .get_hostaddrs()
        .iter()
        .all(|address| address.is_loopback());
    hosts_local && addresses_local
}

/// Refuses a configuration that could reach a non-loopback host without
/// TLS. Only `sslmode=require` passes off loopback.
pub fn require_tls_off_loopback(config: &tokio_postgres::Config) -> Result<(), PersistError> {
    if config.get_ssl_mode() == SslMode::Require || is_loopback_only(config) {
        return Ok(());
    }
    Err(rejected(format!(
        "a non-loopback connection needs sslmode=require, not {:?}",
        config.get_ssl_mode()
    )))
}

/// The production pool: TLS required off loopback, every server
/// certificate verified against `settings`.
pub fn build_tls_pool(
    pg_config: tokio_postgres::Config,
    settings: &TlsSettings,
    policy: PoolPolicy,
) -> Result<Pool, PersistError> {
    require_tls_off_loopback(&pg_config)?;
    build_pool(pg_config, rustls_connector(settings)?, policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> tokio_postgres::Config {
        text.parse().expect("test connection string")
    }

    #[test]
    fn plaintext_is_refused_off_loopback_and_allowed_on_it() {
        for text in [
            "host=db.example.net sslmode=disable",
            "host=db.example.net sslmode=prefer",
            "host=10.0.0.5 sslmode=disable",
            "host=localhost hostaddr=10.0.0.5 sslmode=disable",
            "host=127.0.0.1,db.example.net sslmode=disable",
        ] {
            let error = require_tls_off_loopback(&config(text)).unwrap_err();
            assert!(matches!(error, PersistError::TlsConfiguration(_)), "{text}");
            assert!(!error.is_retryable(), "{text}");
        }
        for text in [
            "host=db.example.net sslmode=require",
            "host=localhost sslmode=disable",
            "host=127.0.0.1 sslmode=prefer",
            "host=::1 sslmode=disable",
            "host=localhost hostaddr=127.0.0.1 sslmode=disable",
        ] {
            assert!(require_tls_off_loopback(&config(text)).is_ok(), "{text}");
        }
    }

    #[test]
    fn a_connector_needs_real_trust_anchors() {
        for bundle in [
            &b""[..],
            b"not pem at all",
            b"-----BEGIN CERTIFICATE-----\n!!\n-----END CERTIFICATE-----\n",
        ] {
            let Err(error) = rustls_connector(&TlsSettings::trusting(bundle)) else {
                panic!("an unusable CA bundle must be refused");
            };
            assert!(matches!(error, PersistError::TlsConfiguration(_)));
        }
    }

    #[test]
    fn a_client_identity_never_prints_its_key() {
        let identity = ClientIdentity {
            certificate_chain_pem: Vec::new(),
            private_key_pem: b"secret".to_vec(),
        };
        assert!(!format!("{identity:?}").contains("secret"));
    }
}
