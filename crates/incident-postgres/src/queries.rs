//! Tenant-scoped, permission-checked reads for the API (5D, ADR 0038
//! gate 5).
//!
//! Commands are authorized inside the domain's unit of work. Reads have no
//! command, so this module is their boundary:
//!
//! - **The permission is checked here,** not in HTTP. A caller without
//!   `incident.read` (or `incident.list`) is refused for any id or filter,
//!   so the refusal says nothing about what exists.
//! - **The tenant comes only from the context,** and every query is
//!   scoped by it. Another tenant's incident is `NotFound`, the same as
//!   none at all.
//! - **One consistent snapshot:** an incident and its notes, tags and
//!   policy references are read in one `REPEATABLE READ READ ONLY`
//!   transaction, so a concurrent write cannot be half seen.
//! - **Lists are keyset-paginated** on `(sort column, incident_id)`, which
//!   the V16 indexes serve, so a page is a bounded index range however
//!   much history the tenant has.

use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, IsolationLevel, Row};
use wetechinetmon_incident::authorization::{AuthorizationContext, Permission};
use wetechinetmon_incident::error::IncidentError;
use wetechinetmon_incident::id::IncidentId;
use wetechinetmon_incident::incident::Incident;

use crate::error::PersistError;
use crate::service::Outcome;
use crate::sql::{load_incident, required_micros, Locking};

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

/// What the list is ordered by. `incident_id` breaks ties, so every
/// position is unique and keyset pagination never skips or repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSort {
    OpenedAt,
    LastDetectedAt,
}

impl ListSort {
    pub fn as_str(self) -> &'static str {
        match self {
            ListSort::OpenedAt => "opened_at",
            ListSort::LastDetectedAt => "last_detected_at",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "opened_at" => Some(ListSort::OpenedAt),
            "last_detected_at" => Some(ListSort::LastDetectedAt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            SortOrder::Asc => "asc",
            SortOrder::Desc => "desc",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "asc" => Some(SortOrder::Asc),
            "desc" => Some(SortOrder::Desc),
            _ => None,
        }
    }
}

/// A validated list request. Every value is bound as a parameter; the
/// caller has already checked each against its vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListFilter {
    pub states: Vec<String>,
    pub severities: Vec<String>,
    pub priorities: Vec<String>,
    pub direction: Option<String>,
    pub target_type: Option<String>,
    /// `[from, to)` on `opened_at`, in microseconds.
    pub opened_between: Option<(i64, i64)>,
    pub sort: ListSort,
    pub order: SortOrder,
    /// Resume after this position: the sort key in microseconds and the
    /// incident id.
    pub after: Option<(i64, String)>,
    /// Page size; the caller bounds it.
    pub limit: u32,
}

/// One incident in a list: the columns of `incidents` alone, so a page is
/// one query rather than one per incident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentSummary {
    pub incident_id: String,
    pub incident_number: String,
    pub title: String,
    pub state: String,
    pub severity: String,
    pub priority: String,
    pub category: String,
    pub direction: String,
    pub address_family: i16,
    pub target_type: String,
    pub target_id: String,
    pub opened_at: i64,
    pub last_detected_at: i64,
    pub last_updated_at: i64,
    pub assigned_kind: Option<String>,
    pub assigned_id: Option<String>,
    pub version: i64,
}

impl IncidentSummary {
    /// The value of the column the list is sorted by.
    pub fn sort_key(&self, sort: ListSort) -> i64 {
        match sort {
            ListSort::OpenedAt => self.opened_at,
            ListSort::LastDetectedAt => self.last_detected_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    pub items: Vec<IncidentSummary>,
    /// Whether another page follows the last item.
    pub has_more: bool,
}

const SUMMARY_COLUMNS: &str = "\
SELECT incident_id::text AS incident_id, incident_number, title, state, severity, priority,
    category, direction, address_family, target_type,
    COALESCE(host(target_addr), target_network::text, target_hostgroup, '') AS target_id,
    opened_at, last_detected_at, last_updated_at, assigned_kind, assigned_id, version
FROM incidents";

const FROM_MICROS: &str = "TIMESTAMPTZ 'epoch' + $?::bigint * interval '1 microsecond'";

type Params = Vec<Box<dyn ToSql + Sync + Send>>;

/// Appends `clause`, with its `$?` numbered as the next parameter.
fn bind(sql: &mut String, params: &mut Params, clause: &str, value: Box<dyn ToSql + Sync + Send>) {
    params.push(value);
    sql.push_str(&clause.replace("$?", &format!("${}", params.len())));
}

/// The tenant predicate and the filters, as SQL and its parameters.
fn where_clause(
    auth: &AuthorizationContext,
    filter: &ListFilter,
    with_cursor: bool,
) -> (String, Params) {
    let mut sql = String::from(" WHERE tenant_id = $1");
    let mut params: Params = vec![Box::new(auth.tenant().as_str().to_string())];
    if !filter.states.is_empty() {
        bind(
            &mut sql,
            &mut params,
            " AND state = ANY($?)",
            Box::new(filter.states.clone()),
        );
    }
    if !filter.severities.is_empty() {
        bind(
            &mut sql,
            &mut params,
            " AND severity = ANY($?)",
            Box::new(filter.severities.clone()),
        );
    }
    if !filter.priorities.is_empty() {
        bind(
            &mut sql,
            &mut params,
            " AND priority = ANY($?)",
            Box::new(filter.priorities.clone()),
        );
    }
    if let Some(direction) = &filter.direction {
        bind(
            &mut sql,
            &mut params,
            " AND direction = $?",
            Box::new(direction.clone()),
        );
    }
    if let Some(target_type) = &filter.target_type {
        bind(
            &mut sql,
            &mut params,
            " AND target_type = $?",
            Box::new(target_type.clone()),
        );
    }
    if let Some((from, to)) = filter.opened_between {
        bind(
            &mut sql,
            &mut params,
            &format!(" AND opened_at >= {FROM_MICROS}"),
            Box::new(from),
        );
        bind(
            &mut sql,
            &mut params,
            &format!(" AND opened_at < {FROM_MICROS}"),
            Box::new(to),
        );
    }
    if with_cursor {
        if let Some((key, id)) = &filter.after {
            let column = filter.sort.as_str();
            let comparison = match filter.order {
                SortOrder::Desc => "<",
                SortOrder::Asc => ">",
            };
            bind(
                &mut sql,
                &mut params,
                &format!(" AND ({column}, incident_id) {comparison} ({FROM_MICROS}"),
                Box::new(*key),
            );
            bind(
                &mut sql,
                &mut params,
                ", $?::text::uuid)",
                Box::new(id.clone()),
            );
        }
    }
    (sql, params)
}

/// One page of the caller's tenant's incidents, under `incident.list`.
pub async fn list_incidents(
    client: &mut Client,
    auth: &AuthorizationContext,
    filter: &ListFilter,
) -> Outcome<ListPage> {
    if !auth.has(Permission::IncidentList) {
        return Ok(Err(IncidentError::Unauthorized));
    }
    let (where_sql, mut params) = where_clause(auth, filter, true);
    let column = filter.sort.as_str();
    let order = filter.order.as_str();
    params.push(Box::new(i64::from(filter.limit) + 1));
    let statement = format!(
        "{SUMMARY_COLUMNS}{where_sql} ORDER BY {column} {order}, incident_id {order} LIMIT ${}",
        params.len()
    );
    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref() as _).collect();
    let rows = client.query(statement.as_str(), &refs).await?;
    let mut items = rows
        .iter()
        .map(summary_from_row)
        .collect::<Result<Vec<_>, PersistError>>()?;
    let has_more = items.len() > filter.limit as usize;
    items.truncate(filter.limit as usize);
    Ok(Ok(ListPage { items, has_more }))
}

/// How many incidents match, ignoring the cursor and page size.
pub async fn count_incidents(
    client: &mut Client,
    auth: &AuthorizationContext,
    filter: &ListFilter,
) -> Outcome<i64> {
    if !auth.has(Permission::IncidentList) {
        return Ok(Err(IncidentError::Unauthorized));
    }
    let (where_sql, params) = where_clause(auth, filter, false);
    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref() as _).collect();
    let statement = format!("SELECT count(*) FROM incidents{where_sql}");
    let row = client.query_one(statement.as_str(), &refs).await?;
    Ok(Ok(row.try_get(0)?))
}

fn summary_from_row(row: &Row) -> Result<IncidentSummary, PersistError> {
    Ok(IncidentSummary {
        incident_id: row.try_get("incident_id")?,
        incident_number: row.try_get("incident_number")?,
        title: row.try_get("title")?,
        state: row.try_get("state")?,
        severity: row.try_get("severity")?,
        priority: row.try_get("priority")?,
        category: row.try_get("category")?,
        direction: row.try_get("direction")?,
        address_family: row.try_get("address_family")?,
        target_type: row.try_get("target_type")?,
        target_id: row.try_get("target_id")?,
        opened_at: required_micros(row, "opened_at")?,
        last_detected_at: required_micros(row, "last_detected_at")?,
        last_updated_at: required_micros(row, "last_updated_at")?,
        assigned_kind: row.try_get("assigned_kind")?,
        assigned_id: row.try_get("assigned_id")?,
        version: row.try_get("version")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wetechinetmon_incident::authorization::Actor;
    use wetechinetmon_incident::correlation::TenantId;

    fn auth() -> AuthorizationContext {
        AuthorizationContext::new(
            TenantId::new("acme"),
            Actor::Operator { id: "a".into() },
            vec![Permission::IncidentList],
        )
    }

    fn filter() -> ListFilter {
        ListFilter {
            states: vec![],
            severities: vec![],
            priorities: vec![],
            direction: None,
            target_type: None,
            opened_between: None,
            sort: ListSort::OpenedAt,
            order: SortOrder::Desc,
            after: None,
            limit: 50,
        }
    }

    #[test]
    fn the_tenant_is_always_the_first_parameter() {
        let (sql, params) = where_clause(&auth(), &filter(), true);
        assert_eq!(sql, " WHERE tenant_id = $1");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn filters_and_the_cursor_number_their_parameters_in_order() {
        let mut f = filter();
        f.states = vec!["open".into()];
        f.direction = Some("incoming".into());
        f.opened_between = Some((1, 2));
        f.after = Some((10, "0192f3c4-8a7b-7e1f-9c2d-3e4f5a6b7c8d".into()));
        let (sql, params) = where_clause(&auth(), &f, true);
        assert_eq!(params.len(), 7);
        assert!(sql.contains("state = ANY($2)"), "{sql}");
        assert!(sql.contains("direction = $3"), "{sql}");
        assert!(
            sql.contains("opened_at >= TIMESTAMPTZ 'epoch' + $4::bigint"),
            "{sql}"
        );
        assert!(
            sql.contains("opened_at < TIMESTAMPTZ 'epoch' + $5::bigint"),
            "{sql}"
        );
        assert!(
            sql.contains("(opened_at, incident_id) < (TIMESTAMPTZ 'epoch' + $6::bigint"),
            "{sql}"
        );
        assert!(sql.ends_with(", $7::text::uuid)"), "{sql}");
        // The count ignores the cursor.
        let (count_sql, count_params) = where_clause(&auth(), &f, false);
        assert_eq!(count_params.len(), 5);
        assert!(!count_sql.contains("incident_id"));
    }

    #[test]
    fn ascending_pages_move_forward() {
        let mut f = filter();
        f.order = SortOrder::Asc;
        f.sort = ListSort::LastDetectedAt;
        f.after = Some((10, "x".into()));
        let (sql, _) = where_clause(&auth(), &f, true);
        assert!(sql.contains("(last_detected_at, incident_id) > ("), "{sql}");
    }
}
