//! Tenant-scoped, permission-checked reads for the API (5D, ADR 0038
//! gate 5).
//!
//! Commands are authorized inside the domain's unit of work. Reads have no
//! command, so this module is their boundary:
//!
//! - **The permission is checked here,** not in HTTP. A caller without
//!   `incident.read` gets `Unauthorized` for any id, so the refusal says
//!   nothing about whether the incident exists.
//! - **The tenant comes only from the context,** and every query is
//!   scoped by it. Another tenant's incident is `NotFound`, the same as
//!   none at all.
//! - **One consistent snapshot:** an incident and its notes, tags and
//!   policy references are read in one `REPEATABLE READ READ ONLY`
//!   transaction, so a concurrent write cannot be half seen.

use tokio_postgres::{Client, IsolationLevel};
use wetechinetmon_incident::authorization::{AuthorizationContext, Permission};
use wetechinetmon_incident::error::IncidentError;
use wetechinetmon_incident::id::IncidentId;
use wetechinetmon_incident::incident::Incident;

use crate::service::Outcome;
use crate::sql::{load_incident, Locking};

/// One incident, if the caller may read it and it is in their tenant.
pub async fn get_incident(
    client: &mut Client,
    auth: &AuthorizationContext,
    incident_id: &IncidentId,
) -> Outcome<Incident> {
    if !auth.has(Permission::IncidentRead) {
        return Ok(Err(IncidentError::Unauthorized));
    }
    let transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let found = load_incident(&transaction, auth.tenant(), incident_id, Locking::NoLock).await?;
    transaction.commit().await?;
    Ok(found.ok_or(IncidentError::NotFound))
}
