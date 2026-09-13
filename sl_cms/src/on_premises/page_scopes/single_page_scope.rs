use std::sync::Arc;
use actix_web::{HttpResponse, Responder, Scope, delete, get, post, put, web};

use crate::{AppModule, models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema}, on_premises::page_scopes::get_actix_error};

pub fn single_page_scope() -> Scope {
    web::scope("/models/single_pages")
        .service(get_single_pages)
        .service(get_single_page_schema)
        .service(add_single_page_schema)
        .service(update_single_page_schema)
        .service(delete_single_page)
        .service(get_single_page_item)
        .service(update_single_page_item)
}
#[get("")]
async fn get_single_pages(app_module: web::Data<Arc<AppModule>>) -> impl Responder {
    let pages = app_module.static_single_page_service.get_all_page_names();
    match pages {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(e) => get_actix_error(&e),
    }
}
#[get("/{page_name}/schema")]
async fn get_single_page_schema(app_module: web::Data<Arc<AppModule>>, page_name: web::Path<SinglePageName>) -> impl Responder {
    let page = app_module.static_single_page_service.get_single_page_schema(&page_name);
    match page {
        Ok(schema) => HttpResponse::Ok().json(schema),
        Err(e) => get_actix_error(&e),
    }
}
#[post("/{page_name}/schema")]
async fn add_single_page_schema(app_module: web::Data<Arc<AppModule>>, page_name: web::Path<SinglePageName>, schema: web::Json<SinglePageSchema>) -> impl Responder {
    let result = app_module.static_single_page_service.add_single_page_schema(&page_name, &schema);
    match result {
        Ok(_) => HttpResponse::Ok().body("Single page schema added successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[put("/{page_name}/schema")]
async fn update_single_page_schema(app_module: web::Data<Arc<AppModule>>, page_name: web::Path<SinglePageName>, schema: web::Json<SinglePageSchema>) -> impl Responder {
    let result = app_module.static_single_page_service.update_single_page_schema(&page_name, &schema);
    match result {
        Ok(_) => HttpResponse::Ok().body("Single page schema updated successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[delete("/{page_name}")]
async fn delete_single_page(app_module: web::Data<Arc<AppModule>>, page_name: web::Path<SinglePageName>) -> impl Responder {
    let result = app_module.static_single_page_service.delete_single_page(&page_name);
    match result {
        Ok(_) => HttpResponse::Ok().body("Single page schema deleted successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[get("/{page_name}/item")]
async fn get_single_page_item(app_module: web::Data<Arc<AppModule>>, page_name: web::Path<SinglePageName>) -> impl Responder {
    let page = app_module.static_single_page_service.get_single_page_item(&page_name);
    match page {
        Ok(items) => HttpResponse::Ok().json(items),
        Err(e) => get_actix_error(&e),
    }
}
#[put("/{page_name}/item")]
async fn update_single_page_item(app_module: web::Data<Arc<AppModule>>, page_name: web::Path<SinglePageName>, item_data: web::Json<SinglePageItem>) -> impl Responder {
    let result = app_module.static_single_page_service.update_single_page_item(&page_name, &item_data);
    match result {
        Ok(_) => HttpResponse::Ok().body("Single page item updated successfully"),
        Err(e) => get_actix_error(&e),
    }
}