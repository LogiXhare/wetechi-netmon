//! WetechiNetMon incident REST API (Milestone 5D).
//!
//! The boundary decisions are [ADR 0037](../../docs/architecture/decisions/0037-phase5d-http-framework-and-openapi.md)
//! (axum, OpenAPI from code) and [ADR 0038](../../docs/architecture/decisions/0038-phase5d-api-boundary.md)
//! (TLS, identity, authorization, rate limits, errors). This crate is
//! the API's foundation: errors, request ids, the rate limiter, the
//! listener, health and readiness, and the OpenAPI document. The incident
//! endpoints build on it.
//!
//! Every response that is not a success is RFC 9457 problem details,
//! including unknown paths and wrong methods.

pub mod auth;
pub mod config;
pub mod openapi;
pub mod problem;
pub mod rate_limit;
pub mod request_id;
pub mod server;
pub mod token_admin;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use deadpool_postgres::Pool;
use serde::Serialize;
use utoipa::ToSchema;
use wetechinetmon_incident_postgres::pool::acquire;

use crate::problem::{ErrorCode, Problem};

/// The largest request body accepted, before any handler sees it.
pub const BODY_LIMIT_BYTES: usize = 64 * 1024;

/// What every handler shares.
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}

/// The whole API: routes, problem-details fallbacks, request ids and the
/// body limit.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(axum::extract::DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(axum::middleware::from_fn(request_id::middleware))
        .with_state(state)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// `ok` or `ready`.
    pub status: &'static str,
}

/// Liveness: the process is up. Touches nothing else.
#[utoipa::path(
    get,
    path = "/healthz",
    tag = "operations",
    responses((status = 200, description = "The process is running", body = Health))
)]
pub async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

/// Readiness: the database answers. The API fails closed without it.
#[utoipa::path(
    get,
    path = "/readyz",
    tag = "operations",
    responses(
        (status = 200, description = "The database answers", body = Health),
        (status = 503, description = "The database is unreachable", body = openapi::ProblemDocument,
            content_type = "application/problem+json"),
    )
)]
pub async fn readyz(State(state): State<AppState>) -> Response {
    let ready = async {
        let client = acquire(&state.pool).await?;
        client.query_one("SELECT 1", &[]).await?;
        Ok::<_, wetechinetmon_incident_postgres::error::PersistError>(())
    }
    .await;
    match ready {
        Ok(()) => (StatusCode::OK, Json(Health { status: "ready" })).into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "readiness check failed");
            Problem::new(ErrorCode::Unavailable)
                .retry_after(5)
                .into_response()
        }
    }
}

async fn not_found() -> Problem {
    Problem::new(ErrorCode::NotFound)
}

async fn method_not_allowed() -> Problem {
    Problem::new(ErrorCode::MethodNotAllowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;

    /// A pool that is never connected: building one opens nothing.
    fn state() -> AppState {
        let (pool, _) = wetechinetmon_incident_postgres::connect::connect(
            "host=127.0.0.1 port=1 user=nobody connect_timeout=1",
            None,
            wetechinetmon_incident_postgres::pool::PoolPolicy {
                max_size: 1,
                wait_timeout: std::time::Duration::from_millis(200),
                create_timeout: std::time::Duration::from_millis(500),
                recycle_timeout: std::time::Duration::from_millis(200),
            },
        )
        .unwrap();
        AppState { pool }
    }

    async fn call(
        method: &str,
        path: &str,
    ) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        let response = router(state())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, headers, body)
    }

    #[tokio::test]
    async fn liveness_answers_without_the_database() {
        let (status, headers, body) = call("GET", "/healthz").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert!(headers.contains_key(request_id::HEADER));
    }

    #[tokio::test]
    async fn readiness_fails_closed_when_the_database_is_unreachable() {
        let (status, headers, body) = call("GET", "/readyz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "api.unavailable");
        assert!(headers.contains_key(header::RETRY_AFTER));
        // The body names the request, matching the header.
        assert_eq!(
            body["request_id"],
            headers[request_id::HEADER].to_str().unwrap()
        );
    }

    #[tokio::test]
    async fn unknown_paths_and_methods_are_problem_details() {
        let (status, headers, body) = call("GET", "/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers[header::CONTENT_TYPE], problem::PROBLEM_CONTENT_TYPE);
        assert_eq!(body["error"], "api.not_found");

        let (status, _, body) = call("DELETE", "/healthz").await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(body["error"], "api.method_not_allowed");
    }
}
