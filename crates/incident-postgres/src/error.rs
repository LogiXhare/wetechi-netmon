//! Errors from mapping incidents to rows and from the SQL that stores them.

use wetechinetmon_incident::error::IncidentError;
use wetechinetmon_incident::id::IncidentId;

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    /// The domain holds a value the schema has no column value for.
    #[error("{field} cannot be stored: {detail}")]
    Unrepresentable { field: &'static str, detail: String },

    /// A stored column holds something the domain cannot read.
    #[error("column {column} holds a value the domain cannot read: {detail}")]
    Corrupt {
        column: &'static str,
        detail: String,
    },

    /// Every column parsed, but the incident they describe is inconsistent.
    #[error("the stored incident failed reconstitution: {0}")]
    Rejected(#[source] IncidentError),

    /// The row is no longer at the version the call loaded, so another
    /// transaction changed it first (ADR 0034's flush guard).
    #[error("incident {incident_id} is no longer at version {loaded_version}")]
    VersionConflict {
        incident_id: IncidentId,
        loaded_version: u64,
    },

    #[error("database error: {0}")]
    Database(#[from] tokio_postgres::Error),
}

impl PersistError {
    pub(crate) fn corrupt(column: &'static str, detail: impl Into<String>) -> Self {
        PersistError::Corrupt {
            column,
            detail: detail.into(),
        }
    }

    pub(crate) fn unrepresentable(field: &'static str, detail: impl Into<String>) -> Self {
        PersistError::Unrepresentable {
            field,
            detail: detail.into(),
        }
    }
}
