use std::sync::Arc;
use actix_web::{HttpResponse, Responder, Scope, delete, get, post, put, web};

use crate::{AppModule, models::{field::FieldSchema, schema::CompositeFieldId}, on_premises::page_scopes::get_actix_error};

pub fn composite_field_scope() -> Scope {
    web::scope("/models/composite_fields")
        .service(list_composite_field_schemas)
        .service(get_composite_field_schema)
        .service(add_composite_field_schema)
        .service(update_composite_field_schema)
        .service(delete_composite_field_schema)
}

#[get("")]
async fn list_composite_field_schemas(app_module: web::Data<Arc<AppModule>>) -> impl Responder {
    let result = app_module.static_composite_field_service.list_composite_field_schemas();
    match result {
        Ok(schemas) => HttpResponse::Ok().json(schemas),
        Err(e) => get_actix_error(&e),
    }
}
#[get("/{id}")]
async fn get_composite_field_schema(app_module: web::Data<Arc<AppModule>>, id: web::Path<CompositeFieldId>) -> impl Responder {
    let result = app_module.static_composite_field_service.get_composite_field_schema(&id);
    match result {
        Ok(schema) => HttpResponse::Ok().json(schema),
        Err(e) => get_actix_error(&e),
    }
}
#[post("/{id}")]
async fn add_composite_field_schema(app_module: web::Data<Arc<AppModule>>, id: web::Path<CompositeFieldId>, schema: web::Json<Vec<FieldSchema>>) -> impl Responder {
    let result = app_module.static_composite_field_service.add_composite_field_schema(&id, &schema);
    match result {
        Ok(_) => HttpResponse::Ok().body("Composite field schema added successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[put("/{id}")]
async fn update_composite_field_schema(app_module: web::Data<Arc<AppModule>>, id: web::Path<CompositeFieldId>, schema: web::Json<Vec<FieldSchema>>) -> impl Responder {
    let result = app_module.static_composite_field_service.update_composite_field_schema(&id, &schema);
    match result {
        Ok(_) => HttpResponse::Ok().body("Composite field schema updated successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[delete("/{id}")]
async fn delete_composite_field_schema(app_module: web::Data<Arc<AppModule>>, id: web::Path<CompositeFieldId>) -> impl Responder {
    let result = app_module.static_composite_field_service.delete_composite_field_schema(&id);
    match result {
        Ok(_) => HttpResponse::Ok().body("Composite field schema deleted successfully"),
        Err(e) => get_actix_error(&e),
    }
}
