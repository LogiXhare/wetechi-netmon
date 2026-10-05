//! A UUIDv7 id for every request (ADR 0038, gate 7).
//!
//! It is returned in `X-Request-Id`, put in every problem body, and
//! recorded on the request's tracing span, so an error report can be
//! matched to the log lines that explain it. An id sent by the client is
//! never trusted: it would let a caller forge or collide log correlation.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;
use uuid::{NoContext, Timestamp, Uuid};

pub const HEADER: &str = "x-request-id";

tokio::task_local! {
    static REQUEST_ID: String;
}

/// The id of the request this task is serving, if any.
pub fn current() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

/// A fresh UUIDv7: time-ordered, so ids sort with the logs.
pub fn generate() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp =
        Timestamp::from_unix(NoContext, since_epoch.as_secs(), since_epoch.subsec_nanos());
    Uuid::new_v7(timestamp).to_string()
}

/// Assigns the id, runs the request inside its scope and span, and
/// returns the id in `X-Request-Id`.
pub async fn middleware(request: Request, next: Next) -> Response {
    let id = generate();
    let span = tracing::info_span!(
        "request",
        request_id = %id,
        method = %request.method(),
        path = %request.uri().path(),
    );
    let mut response = REQUEST_ID
        .scope(id.clone(), next.run(request).instrument(span))
        .await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(HEADER, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_version_7_and_distinct() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
        assert_eq!(Uuid::parse_str(&a).unwrap().get_version_num(), 7);
    }

    #[tokio::test]
    async fn the_current_id_is_visible_only_inside_its_scope() {
        assert_eq!(current(), None);
        let seen = REQUEST_ID
            .scope("req-1".to_string(), async { current() })
            .await;
        assert_eq!(seen.as_deref(), Some("req-1"));
    }
}
