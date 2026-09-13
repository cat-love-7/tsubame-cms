use std::fmt::Display;
use std::error::Error;

/// HTTP status codes this application produces.
pub const STATUS_BAD_REQUEST: u16 = 400;
pub const STATUS_UNAUTHORIZED: u16 = 401;
pub const STATUS_FORBIDDEN: u16 = 403;
pub const STATUS_NOT_FOUND: u16 = 404;
pub const STATUS_CONFLICT: u16 = 409;
pub const STATUS_INTERNAL_SERVER_ERROR: u16 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status_code: u16,
    pub message: String,
}
impl HttpError {
    pub fn new(status_code: u16, message: &str) -> Self {
        HttpError {
            status_code,
            message: message.to_string(),
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
}
