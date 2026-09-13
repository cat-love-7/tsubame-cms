pub mod collection_scope;
pub mod composite_field_scope;
pub mod single_page_scope;
pub mod file_scope;
pub mod image_scope;

use actix_web::HttpResponse;
use crate::models::error::{HttpError,ErrorKind};

pub fn get_actix_error(error:&HttpError) -> HttpResponse {
    match ErrorKind::from(error) {
        ErrorKind::NotFound => {
            HttpResponse::NotFound().body(error.message.clone())
        },
        ErrorKind::BadRequest => {
            HttpResponse::BadRequest().body(error.message.clone())
        },
        ErrorKind::InternalServerError => {
            HttpResponse::InternalServerError().body(error.message.clone())
        },
    }
}