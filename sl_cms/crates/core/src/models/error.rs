use std::fmt::Display;
use std::error::Error;

/// The codes a client can recognise without reading English.
///
/// A client that knows the code chooses its own wording; one that does not shows `message`,
/// which is why a new code cannot break an old client. The list is the contract: a Rust test
/// keeps `frontend/sl_cms/src/assets/error-codes.json` equal to it, and a frontend test keeps
/// its translations covering that file.
pub const ERROR_CODES: &[&str] = &[
    "bad_request",
    "unauthorized",
    "forbidden",
    "not_found",
    "conflict",
    "too_many_requests",
    "internal_error",
    "not_implemented",
];

/// The code a status stands for, when a site has nothing more specific to say.
pub fn default_code(status_code: u16) -> &'static str {
    match status_code {
        STATUS_BAD_REQUEST => "bad_request",
        STATUS_UNAUTHORIZED => "unauthorized",
        STATUS_FORBIDDEN => "forbidden",
        STATUS_NOT_FOUND => "not_found",
        STATUS_CONFLICT => "conflict",
        STATUS_TOO_MANY_REQUESTS => "too_many_requests",
        501 => "not_implemented",
        _ => "internal_error",
    }
}

/// HTTP status codes this application produces.
pub const STATUS_BAD_REQUEST: u16 = 400;
pub const STATUS_UNAUTHORIZED: u16 = 401;
pub const STATUS_FORBIDDEN: u16 = 403;
pub const STATUS_NOT_FOUND: u16 = 404;
pub const STATUS_CONFLICT: u16 = 409;
pub const STATUS_TOO_MANY_REQUESTS: u16 = 429;
pub const STATUS_INTERNAL_SERVER_ERROR: u16 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status_code: u16,
    /// A stable identifier for what went wrong, alongside the English `message`. Clients key
    /// their wording off this (see [`ERROR_CODES`]).
    pub code: &'static str,
    pub message: String,
    /// Set by the 429 answers so the response can carry `Retry-After`.
    pub retry_after_seconds: Option<u64>,
}
impl HttpError {
    /// Say what went wrong more precisely than the status does.
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = code;
        self
    }

    pub fn new(status_code: u16, message: &str) -> Self {
        HttpError {
            code: default_code(status_code),
            status_code,
            message: message.to_string(),
            retry_after_seconds: None,
        }
    }

    // The PascalCase names are kept for readability at the call sites.

    /// The input was understood but is invalid.
    #[allow(non_snake_case)]
    pub fn BadRequest(message: &str) -> Self {
        HttpError::new(STATUS_BAD_REQUEST, message)
    }

    /// No credentials, or credentials that are not valid.
    #[allow(non_snake_case)]
    pub fn Unauthorized(message: &str) -> Self {
        HttpError::new(STATUS_UNAUTHORIZED, message)
    }

    /// Authenticated, but not allowed to do this.
    #[allow(non_snake_case)]
    pub fn Forbidden(message: &str) -> Self {
        HttpError::new(STATUS_FORBIDDEN, message)
    }

    /// The resource does not exist.
    #[allow(non_snake_case)]
    pub fn NotFound(message: &str) -> Self {
        HttpError::new(STATUS_NOT_FOUND, message)
    }

    /// The request conflicts with the current state, e.g. creating something twice.
    #[allow(non_snake_case)]
    pub fn Conflict(message: &str) -> Self {
        HttpError::new(STATUS_CONFLICT, message)
    }

    /// Too many attempts: the caller has to wait, and is told for how long.
    #[allow(non_snake_case)]
    pub fn TooManyRequests(wait: std::time::Duration) -> Self {
        // At least one second: "try again in 0 seconds" would invite an immediate retry.
        let seconds = wait.as_secs().max(1);
        HttpError {
            code: default_code(STATUS_TOO_MANY_REQUESTS),
            status_code: STATUS_TOO_MANY_REQUESTS,
            message: format!("too many failed attempts; try again in {seconds} seconds"),
            retry_after_seconds: Some(seconds),
        }
    }

    #[allow(non_snake_case)]
    pub fn InternalServerError(message: &str) -> Self {
        HttpError::new(STATUS_INTERNAL_SERVER_ERROR, message)
    }
}
impl Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP Error {}: {}", self.status_code, self.message)
    }
}
impl Error for HttpError {}

/// Convert any error into an opaque 500, so internal details are not leaked to clients.
pub fn map_internal_error<E: std::fmt::Display>(e: E) -> HttpError {
    HttpError::InternalServerError(&e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_carry_the_expected_status() {
        assert_eq!(HttpError::BadRequest("x").status_code, 400);
        assert_eq!(HttpError::Unauthorized("x").status_code, 401);
        assert_eq!(HttpError::Forbidden("x").status_code, 403);
        assert_eq!(HttpError::NotFound("x").status_code, 404);
        assert_eq!(HttpError::Conflict("x").status_code, 409);
        assert_eq!(HttpError::InternalServerError("x").status_code, 500);
    }

    #[test]
    fn errors_are_comparable_by_status_and_message() {
        assert_eq!(HttpError::Conflict("taken"), HttpError::new(409, "taken"));
        assert_ne!(HttpError::Conflict("taken"), HttpError::Conflict("other"));
    }


    /// The client's copy of the list. A Rust test owns it so the two cannot drift: adding a
    /// code here without adding it there fails, and the frontend test then fails on the
    /// translations.
    #[test]
    fn the_published_code_list_matches_the_one_clients_carry() {
        let published: Vec<String> = serde_json::from_str(include_str!(
            "../../../../../frontend/sl_cms/src/assets/error-codes.json"
        ))
        .expect("the fixture is a JSON array of strings");

        assert_eq!(published, ERROR_CODES, "regenerate the fixture when this changes");
    }

    #[test]
    fn every_status_has_a_code_from_the_list() {
        for status in [400, 401, 403, 404, 409, 429, 500, 501, 418] {
            let code = default_code(status);
            assert!(ERROR_CODES.contains(&code), "{status} → {code} is not published");
        }
    }
}
