use std::fmt::Display;
use std::error::Error;

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
    #[allow(non_snake_case)]
    pub fn NotFound(message: &str) -> Self {
        HttpError::new(ErrorKind::NotFound as u16, message)
    }
    #[allow(non_snake_case)]
    pub fn BadRequest(message: &str) -> Self {
        HttpError::new(ErrorKind::BadRequest as u16, message)
    }
    #[allow(non_snake_case)]
    pub fn InternalServerError(message: &str) -> Self {
        HttpError::new(ErrorKind::InternalServerError as u16, message)
    }
}
impl Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP Error {}: {}", self.status_code, self.message)
    }
}
impl Error for HttpError {}

// Helper function to convert any error to HttpError::InternalServerError
pub fn to_internal_error<E: std::fmt::Display>(e: E) -> HttpError {
    HttpError::InternalServerError(&e.to_string())
}

// Helper function for map_err
pub fn map_internal_error<E: std::fmt::Display>(e: E) -> HttpError {
    to_internal_error(e)
}

pub enum ErrorKind {
    NotFound = 404,
    BadRequest = 400,
    InternalServerError = 500,
}

impl From<&HttpError> for ErrorKind {

    fn from(value: &HttpError) -> Self {
        match value.status_code {
            404 => ErrorKind::NotFound,
            400 => ErrorKind::BadRequest,
            500 => ErrorKind::InternalServerError,
            _ => ErrorKind::InternalServerError,
        }
    }
}