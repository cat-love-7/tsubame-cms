use std::fmt::Display;
use std::error::Error;

/// The codes that name what went wrong, where the status alone could not say it usefully.
///
/// A client that shows its own wording for one of these loses nothing: the code is the whole
/// story, and the wording can be in the reader's language.
pub const SITUATIONAL_ERROR_CODES: &[&str] = &[
    "invalid_credentials",
    "invalid_token",
    "session_ended",
    "account_disabled",
    "not_provisioned",
    "invalid_username",
    "weak_password",
    "username_taken",
    "last_administrator",
    // A value a field declared unique already holds.
    "value_taken",
    // What a schema said about a value, named precisely enough to mark the input it belongs to.
    "field_required",
    "field_too_long",
    "field_too_short",
    "field_type_mismatch",
    "invalid_enum_value",
    "unknown_composite_field",
    "composite_id_mismatch",
    "nested_arrays",
    // A slug with nothing usable in it: nothing survives normalisation (see `models::slug`).
    "invalid_slug",
    // Publishing lost a race with a save: the working copy changed after it was read.
    "draft_changed",
    // Deleting an image for good without putting it in the trash first: the second step of the
    // two-step delete, asked for out of order.
    "image_not_trashed",
];

/// The fallback code a status stands for, when a site has nothing more specific to say.
///
/// These are *not* the whole story: `message` is. A client that replaced it with its own wording
/// for "bad request" would hide the one sentence that says which input was wrong, so the
/// published wording of these is used only where nothing better is at hand.
pub const STATUS_ERROR_CODES: &[&str] = &[
    "bad_request",
    "unauthorized",
    "forbidden",
    "not_found",
    "conflict",
    "too_many_requests",
    "internal_error",
    "not_implemented",
];

/// Every code a client can recognise without reading English.
///
/// The list is the contract: a Rust test keeps `frontend/sl_cms/src/assets/error-codes.json`
/// equal to it, split the same way, and a frontend test keeps its translations covering every
/// code in it.
pub const ERROR_CODES: &[&str] = &[
    // What went wrong, where the status alone is too coarse to say anything useful to a user.
    "invalid_credentials",
    "invalid_token",
    "session_ended",
    "account_disabled",
    "not_provisioned",
    "invalid_username",
    "weak_password",
    "username_taken",
    "last_administrator",
    "value_taken",
    "field_required",
    "field_too_long",
    "field_too_short",
    "field_type_mismatch",
    "invalid_enum_value",
    "unknown_composite_field",
    "composite_id_mismatch",
    "nested_arrays",
    "invalid_slug",
    // Publishing lost a race with a save: the working copy changed after it was read.
    "draft_changed",
    // Deleting an image for good without putting it in the trash first.
    "image_not_trashed",
    // The fallback for a status that has nothing more specific to say.
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

/// A value the schema refused.
///
/// The API publishes three parts of it: the `code` a client translates, the `field` the refusal
/// is about — a path into the item, so a form can mark the input even when the problem is nested
/// — and the English `message` an operator reads in a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRefusal {
    pub code: &'static str,
    /// Where the input is, from the item's own fields down into composites and array items:
    /// `title`, `tags[2]`, `seo.description`.
    pub field: String,
    pub message: String,
}

impl FieldRefusal {
    fn new(code: &'static str, field: &str, message: String) -> Self {
        FieldRefusal {
            code,
            field: field.to_string(),
            message,
        }
    }

    /// Whether this is "a required field held nothing" - the one refusal a working copy may make.
    pub fn is_missing_required(&self) -> bool {
        self.code == "field_required"
    }

    /// A required field held nothing.
    pub fn required(field: &str) -> Self {
        Self::new("field_required", field, format!("field {field} is required"))
    }

    pub fn too_long(field: &str, max_length: usize) -> Self {
        Self::new(
            "field_too_long",
            field,
            format!("field {field} exceeds maximum length of {max_length}"),
        )
    }

    pub fn too_short(field: &str, min_length: usize) -> Self {
        Self::new(
            "field_too_short",
            field,
            format!("field {field} is below minimum length of {min_length}"),
        )
    }

    /// The value is of a kind the field does not accept (including an array item that matches
    /// none of the declared item types).
    pub fn type_mismatch(field: &str) -> Self {
        Self::new(
            "field_type_mismatch",
            field,
            format!("field {field} does not accept a value of this type"),
        )
    }

    pub fn enum_value(field: &str, value: &str) -> Self {
        Self::new(
            "invalid_enum_value",
            field,
            format!("field {field} holds '{value}', which is not one of its options"),
        )
    }

    pub fn unknown_composite(field: &str, id: &str) -> Self {
        Self::new(
            "unknown_composite_field",
            field,
            format!("field {field} refers to the composite field '{id}', which does not exist"),
        )
    }

    pub fn composite_mismatch(field: &str) -> Self {
        Self::new(
            "composite_id_mismatch",
            field,
            format!("field {field} holds a composite field other than the one the schema names"),
        )
    }

    /// A slug that has nothing a URL could use in it.
    pub fn invalid_slug(field: &str) -> Self {
        Self::new(
            "invalid_slug",
            field,
            format!("field {field} has no characters a slug can be made of"),
        )
    }

    pub fn nested_array(field: &str) -> Self {
        Self::new(
            "nested_arrays",
            field,
            format!("field {field} holds an array inside an array, which is not supported"),
        )
    }

    /// The answer a form can act on: the status says the input was wrong, the code says how, and
    /// the field says where.
    pub fn into_http_error(self) -> HttpError {
        HttpError::BadRequest(&self.message)
            .with_code(self.code)
            .with_field(&self.field)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status_code: u16,
    /// A stable identifier for what went wrong, alongside the English `message`. Clients key
    /// their wording off this (see [`ERROR_CODES`]).
    pub code: &'static str,
    pub message: String,
    /// Which field the refusal is about, when it is about one.
    ///
    /// A form can mark that input rather than showing a sentence about nothing in particular;
    /// codes and messages alone leave it guessing.
    pub field: Option<String>,
    /// Set by the 429 answers so the response can carry `Retry-After`.
    pub retry_after_seconds: Option<u64>,
}
impl HttpError {
    /// Say what went wrong more precisely than the status does.
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = code;
        self
    }

    /// Name the field the refusal is about.
    pub fn with_field(mut self, field: &str) -> Self {
        self.field = Some(field.to_string());
        self
    }

    pub fn new(status_code: u16, message: &str) -> Self {
        HttpError {
            code: default_code(status_code),
            status_code,
            message: message.to_string(),
            field: None,
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
            field: None,
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
///
/// The detail goes to the log and nowhere else. It used to be the body's message, which made the
/// sentence above untrue: a storage error names the table, the key or the query it failed on, and
/// a client (or anyone who can make the request fail) has no business reading that.
pub fn map_internal_error<E: std::fmt::Display>(e: E) -> HttpError {
    tracing::error!("internal error: {e}");
    HttpError::InternalServerError(OPAQUE_INTERNAL_MESSAGE)
}

/// What every 500 says, whatever went wrong: the log has the rest.
pub const OPAQUE_INTERNAL_MESSAGE: &str = "the request could not be completed";

#[cfg(test)]
mod tests {
    use super::*;

    /// A 500 says nothing about what went wrong, whatever it was.
    ///
    /// The message used to be the error's own `Display`, so a DynamoDB failure answered with the
    /// table and the key it was about. The detail belongs in the log.
    #[tokio::test]
    async fn a_500_body_carries_no_internal_detail() {
        use axum::response::IntoResponse;

        let response = map_internal_error(
            "dynamodb put_item failed: ResourceNotFoundException: table cms-prod-7f3a not found",
        )
        .into_response();
        assert_eq!(response.status(), 500);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("the body");
        let body = String::from_utf8_lossy(&body);
        assert!(
            body.contains("internal_error"),
            "the code is what a client can translate: {body}"
        );
        for leak in ["dynamodb", "cms-prod-7f3a", "ResourceNotFound", "put_item"] {
            assert!(!body.contains(leak), "the body leaked {leak}: {body}");
        }
    }

    #[test]
    fn constructors_carry_the_expected_status() {
        assert_eq!(HttpError::BadRequest("x").status_code, 400);
        assert_eq!(HttpError::Unauthorized("x").status_code, 401);
        assert_eq!(HttpError::Forbidden("x").status_code, 403);
        assert_eq!(HttpError::NotFound("x").status_code, 404);
        assert_eq!(HttpError::Conflict("x").status_code, 409);
        assert_eq!(HttpError::InternalServerError("x").status_code, 500);
    }

    /// A refused value becomes something a form can act on, not just a sentence: the code says
    /// what is wrong and the field says which input to mark.
    #[test]
    fn a_field_refusal_becomes_a_bad_request_naming_the_field() {
        let error = FieldRefusal::too_long("seo.description", 20).into_http_error();

        assert_eq!(error.status_code, STATUS_BAD_REQUEST);
        assert_eq!(error.code, "field_too_long");
        assert_eq!(error.field.as_deref(), Some("seo.description"));
        assert_eq!(error.message, "field seo.description exceeds maximum length of 20");
        assert!(ERROR_CODES.contains(&error.code));
    }

    #[test]
    fn errors_are_comparable_by_status_and_message() {
        assert_eq!(HttpError::Conflict("taken"), HttpError::new(409, "taken"));
        assert_ne!(HttpError::Conflict("taken"), HttpError::Conflict("other"));
    }


    /// The client's copy of the lists, split the same way. A Rust test owns it so the two cannot
    /// drift: adding a code here without adding it there fails, and the frontend test then fails
    /// on the translations.
    #[test]
    fn the_published_code_list_matches_the_one_clients_carry() {
        #[derive(serde::Deserialize)]
        struct Published {
            situational: Vec<String>,
            status: Vec<String>,
        }

        let published: Published = serde_json::from_str(include_str!(
            "../../../../../frontend/sl_cms/src/assets/error-codes.json"
        ))
        .expect("the fixture names both lists");

        assert_eq!(published.situational, SITUATIONAL_ERROR_CODES);
        assert_eq!(published.status, STATUS_ERROR_CODES);
    }

    /// The three lists are one list: the two halves must make up [`ERROR_CODES`] exactly, or a
    /// code could be reachable without being published (or published twice).
    #[test]
    fn the_two_halves_make_up_the_published_list() {
        let mut both: Vec<&str> = SITUATIONAL_ERROR_CODES
            .iter()
            .chain(STATUS_ERROR_CODES)
            .copied()
            .collect();
        both.sort_unstable();
        both.dedup();
        assert_eq!(both.len(), SITUATIONAL_ERROR_CODES.len() + STATUS_ERROR_CODES.len());

        let mut all = ERROR_CODES.to_vec();
        all.sort_unstable();
        assert_eq!(both, all);
    }

    #[test]
    fn every_status_has_a_code_from_the_list() {
        for status in [400, 401, 403, 404, 409, 429, 500, 501, 418] {
            let code = default_code(status);
            assert!(ERROR_CODES.contains(&code), "{status} → {code} is not published");
        }
    }
}
