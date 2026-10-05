//! Exit codes, per the CLI plan. Distinct codes let a wrapper tell "retry
//! later" (6, 7) from "your input is wrong" (2, 4) without parsing text.

use hyper::StatusCode;

pub const SUCCESS: i32 = 0;
pub const FAILURE: i32 = 1;
pub const USAGE: i32 = 2;
pub const AUTH: i32 = 3;
pub const CONFLICT: i32 = 4;
pub const NOT_FOUND: i32 = 5;
pub const RATE_LIMITED: i32 = 6;
pub const UNAVAILABLE: i32 = 7;

/// The exit code for an API answer that was not a success.
pub fn for_status(status: StatusCode) -> i32 {
    match status.as_u16() {
        400 | 413 | 415 | 422 => USAGE,
        401 | 403 => AUTH,
        404 => NOT_FOUND,
        409 => CONFLICT,
        429 => RATE_LIMITED,
        500..=599 if status != StatusCode::NOT_IMPLEMENTED => UNAVAILABLE,
        _ => FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_class_has_its_code() {
        for (status, code) in [
            (400, USAGE),
            (422, USAGE),
            (401, AUTH),
            (403, AUTH),
            (404, NOT_FOUND),
            (409, CONFLICT),
            (429, RATE_LIMITED),
            (500, UNAVAILABLE),
            (503, UNAVAILABLE),
            (501, FAILURE),
            (418, FAILURE),
        ] {
            assert_eq!(
                for_status(StatusCode::from_u16(status).unwrap()),
                code,
                "{status}"
            );
        }
    }
}
