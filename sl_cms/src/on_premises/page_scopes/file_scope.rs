use std::{fs, sync::Arc};
use actix_web::{HttpResponse, Responder, Scope, get, put, web};

use crate::AppModule;


pub fn file_scope() -> Scope {
    web::scope("/images")
        .service(get_image_file)
        .service(put_image_file)
}
#[get("/{file_name}")]
async fn get_image_file(app_module: web::Data<Arc<AppModule>>, file_name: web::Path<String>) -> impl Responder {
    let result = fs::read(format!("./data/on_premises/images/{}", file_name));
    match result {
        Ok(file_data) => HttpResponse::Ok()
            .content_type("application/octet-stream")
            .body(file_data),
        Err(e) => HttpResponse::NotFound().body(format!("File not found: {}", e)),
    }
}

#[derive(serde::Deserialize)]
struct PutImageQuery {
    key: String,
}
#[put("/{file_name}")]
async fn put_image_file(
    app_module: web::Data<Arc<AppModule>>,
    file_name: web::Path<String>,
    query: web::Query<PutImageQuery>,
    body: web::Bytes,
) -> impl Responder {
    let file_upload_keys = app_module.repository.file_upload_keys.read().unwrap();
    if !file_upload_keys.contains_key(&query.key) {
        return HttpResponse::Unauthorized().body("Invalid upload key");
    }
    drop(file_upload_keys);

    app_module.repository.file_upload_keys.write().unwrap().remove(&query.key);
    
    if let Err(e) = fs::create_dir_all("./data/on_premises/images") {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to create directory: {}", e));
    }

    let file_path = format!("./data/on_premises/images/{}", file_name.as_str());
    match fs::write(&file_path, &body) {
        Ok(_) => HttpResponse::Created().finish(),
        Err(e) => HttpResponse::InternalServerError()
            .body(format!("Failed to write file: {}", e)),
    }
}