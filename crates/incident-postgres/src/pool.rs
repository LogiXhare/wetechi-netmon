//! The connection pool (ADR 0022).
//!
//! `IncidentPersistence` runs each call on one `&mut Client`, and a service
//! takes that client from this pool. What ADR 0022 requires of it:
//!
//! - **Bounded waits.** Waiting for a free connection, opening one, and
//!   verifying one each time out, so nothing blocks forever. Running out
//!   returns [`PersistError::Unavailable`], which the failure table surfaces
//!   as `503`, and it is not retried.
//! - **Verified reuse.** An idle connection is checked with a query before
//!   it is handed out, so a database restart or dropped connection does not
//!   reach a request.
//! - **No production sizing asserted.** [`PoolPolicy::starting_default`] is
//!   a starting value, not a measured one. ADR 0022 leaves sizing to the
//!   performance-test plan.
//!
//! The caller supplies the TLS connector: `crate::tls::build_tls_pool`
//! builds this pool with ADR 0023's verifying rustls connector.

use std::time::Duration;

use deadpool_postgres::{
    Manager, ManagerConfig, Object, Pool, PoolError, RecyclingMethod, Runtime,
};
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::Socket;

use crate::error::PersistError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolPolicy {
    /// The most connections the pool opens. Values below 1 behave as 1.
    pub max_size: usize,
    /// How long acquiring waits for a connection to be free.
    pub wait_timeout: Duration,
    /// How long opening a new connection may take.
    pub create_timeout: Duration,
    /// How long verifying an idle connection may take.
    pub recycle_timeout: Duration,
}

impl PoolPolicy {
    pub const fn starting_default() -> Self {
        PoolPolicy {
            max_size: 16,
            wait_timeout: Duration::from_secs(5),
            create_timeout: Duration::from_secs(5),
            recycle_timeout: Duration::from_secs(5),
        }
    }
}

impl Default for PoolPolicy {
    fn default() -> Self {
        Self::starting_default()
    }
}

/// Builds a pool that verifies every reused connection and never waits
/// without a bound.
pub fn build_pool<T>(
    pg_config: tokio_postgres::Config,
    tls: T,
    policy: PoolPolicy,
) -> Result<Pool, PersistError>
where
    T: MakeTlsConnect<Socket> + Clone + Sync + Send + 'static,
    T::Stream: Sync + Send,
    T::TlsConnect: Sync + Send,
    <T::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    let manager = Manager::from_config(
        pg_config,
        tls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Verified,
        },
    );
    Pool::builder(manager)
        .max_size(policy.max_size.max(1))
        .wait_timeout(Some(policy.wait_timeout))
        .create_timeout(Some(policy.create_timeout))
        .recycle_timeout(Some(policy.recycle_timeout))
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|error| PersistError::Unavailable(format!("the pool could not be built: {error}")))
}

/// Takes a connection from the pool. An error the server reported while
/// opening one (it carries a SQLSTATE, such as a refused login) is
/// [`PersistError::Database`]. Everything else is
/// [`PersistError::Unavailable`]: running out of time or connections, and a
/// connection that could not be established at all, including a refused
/// TLS handshake (ADR 0023).
pub async fn acquire(pool: &Pool) -> Result<Object, PersistError> {
    pool.get().await.map_err(|error| match error {
        PoolError::Backend(database) if database.as_db_error().is_some() => {
            PersistError::Database(database)
        }
        PoolError::Backend(connection) => PersistError::Unavailable(with_sources(&connection)),
        other => PersistError::Unavailable(other.to_string()),
    })
}

/// An error and its causes on one line, so a handshake failure says why.
fn with_sources(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
