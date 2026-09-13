use std::sync::Arc;

use actix_web::{HttpResponse, Responder, Scope, post, web};

use crate::{models::image::NewImageRequest, on_premises::{AppModule, page_scopes::get_actix_error}};

pub fn image_scope() -> Scope {
    web::scope("/models/images")
    .service(generate_image_upload_url)
}

#[post("/get_upload_url")]
async fn generate_image_upload_url(app_module: web::Data<Arc<AppModule>>, image: web::Json<NewImageRequest>) -> impl Responder {
    let result = app_module.static_image_service.generate_image_upload_url(image.into_inner());
    match result {
        Ok(info) => HttpResponse::Ok().json(info),
        Err(e) => get_actix_error(&e),
    }
}