//! JSON request bodies, refused as problem details rather than axum's
//! plain-text rejections.
//!
//! - **The content type must be `application/json`** (parameters such as
//!   `charset` are allowed): otherwise `415 api.unsupported_media_type`.
//! - **The body is at most [`crate::BODY_LIMIT_BYTES`]:** otherwise
//!   `413 api.payload_too_large`.
//! - **An unknown field is `400 api.unknown_field`,** never ignored: every
//!   request type is `deny_unknown_fields`, so a misspelled field fails
//!   loudly instead of being dropped while the operator believes it took
//!   effect. Any other malformed body is `400 api.invalid_request`.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::http::{header, HeaderMap, StatusCode};
use serde::de::DeserializeOwned;

use crate::problem::{ErrorCode, Problem};

/// Parses `body` as `T`, enforcing the rules above.
pub fn json<T: DeserializeOwned>(
    headers: &HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<T, Problem> {
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"));
    if !is_json {
        return Err(Problem::new(ErrorCode::UnsupportedMediaType)
            .with_detail("the body must be application/json"));
    }
    let bytes = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            Problem::new(ErrorCode::PayloadTooLarge)
        } else {
            Problem::new(ErrorCode::InvalidRequest).with_detail("the body could not be read")
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        // serde reports an unknown field as "unknown field `name`, expected
        // ...": the field name is the caller's own input, so it is safe to
        // echo, and the rest of serde's text is not.
        let message = error.to_string();
        match message.strip_prefix("unknown field `") {
            Some(rest) => {
                let name = rest.split('`').next().unwrap_or_default();
                Problem::new(ErrorCode::UnknownField).with_detail(format!("unknown field `{name}`"))
            }
            None => Problem::new(ErrorCode::InvalidRequest)
                .with_detail("the body is not valid for this endpoint"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Probe {
        #[allow(dead_code)]
        expected_version: u64,
    }

    fn headers(content_type: Option<&'static str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = content_type {
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(value));
        }
        headers
    }

    fn parse(content_type: Option<&'static str>, body: &'static str) -> Result<Probe, ErrorCode> {
        json::<Probe>(
            &headers(content_type),
            Ok(Bytes::from_static(body.as_bytes())),
        )
        .map_err(|problem| problem.code())
    }

    #[test]
    fn json_with_or_without_a_charset_is_accepted() {
        assert!(parse(Some("application/json"), r#"{"expected_version":1}"#).is_ok());
        assert!(parse(
            Some("application/json; charset=utf-8"),
            r#"{"expected_version":1}"#
        )
        .is_ok());
    }

    #[test]
    fn anything_else_is_refused_with_its_own_code() {
        let body = r#"{"expected_version":1}"#;
        assert_eq!(
            parse(None, body).unwrap_err(),
            ErrorCode::UnsupportedMediaType
        );
        assert_eq!(
            parse(Some("text/plain"), body).unwrap_err(),
            ErrorCode::UnsupportedMediaType
        );
        assert_eq!(
            parse(Some("application/json"), r#"{"expected_version":1,"x":2}"#).unwrap_err(),
            ErrorCode::UnknownField
        );
        assert_eq!(
            parse(Some("application/json"), r#"{"expected_version":"one"}"#).unwrap_err(),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            parse(Some("application/json"), "{").unwrap_err(),
            ErrorCode::InvalidRequest
        );
    }
}
