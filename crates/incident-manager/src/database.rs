//! The database connection: the pool and startup migrations.
//!
//! **TLS (ADR 0023).** With a CA file configured, every connection uses
//! the verifying rustls connector and anything off loopback must say
//! `sslmode=require`. Without one, the connection string must reach only
//! this host; plaintext is never a silent fallback for a remote server,
//! and the choice is logged at startup.
//!
//! **Migrations (ADR 0024).** Applied before any work starts, under a
//! session advisory lock, so several instances starting together apply
//! each migration once. An operator who runs migrations separately sets
//! `WETECHINETMON_INCIDENT_MIGRATE=false`.

use deadpool_postgres::Pool;
use wetechinetmon_incident_postgres::error::PersistError;
use wetechinetmon_incident_postgres::pool::{acquire, build_pool};
use wetechinetmon_incident_postgres::tls::{
    build_tls_pool, is_loopback_only, ClientIdentity, TlsSettings,
};

use crate::config::{Config, TlsFiles};

/// The advisory-lock key migrations run under: "wnm-mig" in ASCII, so it
/// is recognisable in `pg_locks` and unlikely to collide.
pub const MIGRATION_LOCK_KEY: i64 = 0x0077_6e6d_2d6d_6967;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("the database connection string is not valid")]
    InvalidUrl,
    #[error(
        "the database is not on this host, so TLS is required: \
         set WETECHINETMON_INCIDENT_DATABASE_CA_FILE"
    )]
    TlsRequired,
    #[error("could not read {what} from {path}: {source}")]
    ReadFile {
        what: &'static str,
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Persist(#[from] PersistError),
    #[error("migrations failed: {0}")]
    Migration(String),
}

/// Whether the pool encrypts, for the startup log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    VerifiedTls,
    LoopbackPlaintext,
}

/// Builds the pool. Nothing connects yet: the first connection is opened
/// by the first use.
pub fn connect(config: &Config) -> Result<(Pool, Transport), DatabaseError> {
    // The parse error is not shown: its text can quote the connection
    // string, password included.
    let pg_config: tokio_postgres::Config = config
        .database_url
        .parse()
        .map_err(|_| DatabaseError::InvalidUrl)?;
    match &config.tls {
        Some(files) => {
            let settings = tls_settings(files)?;
            Ok((
                build_tls_pool(pg_config, &settings, config.pool)?,
                Transport::VerifiedTls,
            ))
        }
        None if is_loopback_only(&pg_config) => Ok((
            build_pool(pg_config, tokio_postgres::NoTls, config.pool)?,
            Transport::LoopbackPlaintext,
        )),
        None => Err(DatabaseError::TlsRequired),
    }
}

fn tls_settings(files: &TlsFiles) -> Result<TlsSettings, DatabaseError> {
    let read = |what: &'static str, path: &std::path::Path| {
        std::fs::read(path).map_err(|source| DatabaseError::ReadFile {
            what,
            path: path.display().to_string(),
            source,
        })
    };
    let mut settings = TlsSettings::trusting(read("the CA bundle", &files.ca_file)?);
    if let Some((cert, key)) = &files.client_identity {
        settings.client_identity = Some(ClientIdentity {
            certificate_chain_pem: read("the client certificate", cert)?,
            private_key_pem: read("the client key", key)?,
        });
    }
    Ok(settings)
}

/// Applies pending migrations under [`MIGRATION_LOCK_KEY`]. Returns how
/// many were applied.
pub async fn migrate(pool: &Pool) -> Result<usize, DatabaseError> {
    let mut client = acquire(pool).await?;
    client
        .execute("SELECT pg_advisory_lock($1)", &[&MIGRATION_LOCK_KEY])
        .await
        .map_err(PersistError::from)?;
    let applied = wetechinetmon_incident_postgres::migrations::migrations::runner()
        .run_async(&mut **client)
        .await
        .map(|report| report.applied_migrations().len())
        .map_err(|error| DatabaseError::Migration(error.to_string()));
    // Released even when the run failed: the connection goes back to the
    // pool, and a held session lock would block every later start.
    let unlocked = client
        .execute("SELECT pg_advisory_unlock($1)", &[&MIGRATION_LOCK_KEY])
        .await
        .map_err(PersistError::from);
    let applied = applied?;
    unlocked?;
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DATABASE_URL_ENV_VAR;

    fn config(url: &str, tls: Option<TlsFiles>) -> Config {
        let mut config =
            Config::from_lookup(|var| Ok((var == DATABASE_URL_ENV_VAR).then(|| url.to_string())))
                .unwrap();
        config.tls = tls;
        config
    }

    #[test]
    fn loopback_without_tls_is_allowed_and_reported() {
        let (_pool, transport) = connect(&config("host=localhost user=app", None)).unwrap();
        assert_eq!(transport, Transport::LoopbackPlaintext);
    }

    #[test]
    fn a_remote_database_without_tls_is_refused() {
        for url in [
            "host=db.example.net user=app",
            "host=db.example.net user=app sslmode=require",
            "host=localhost hostaddr=10.0.0.5 user=app",
        ] {
            assert!(
                matches!(connect(&config(url, None)), Err(DatabaseError::TlsRequired)),
                "{url}"
            );
        }
    }

    #[test]
    fn a_missing_ca_file_is_reported_with_its_path() {
        let dir = std::env::temp_dir();
        let tls = TlsFiles {
            ca_file: dir.join("wetechinetmon-incident-manager-missing-ca.pem"),
            client_identity: None,
        };
        let Err(error) = connect(&config("host=db.example.net sslmode=require", Some(tls))) else {
            panic!("a missing CA file must be refused");
        };
        assert!(matches!(error, DatabaseError::ReadFile { .. }), "{error}");
    }

    #[test]
    fn an_invalid_url_never_echoes_its_text() {
        let Err(error) = connect(&config("host=db password=hunter2 port=notaport", None)) else {
            panic!("an unparseable port is refused");
        };
        assert!(matches!(error, DatabaseError::InvalidUrl));
        assert!(!error.to_string().contains("hunter2"));
    }
}
