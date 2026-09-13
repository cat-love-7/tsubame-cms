use std::sync::Arc;

use crate::{AppModule, models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema}, on_premises::page_scopes::get_actix_error};

use actix_web::{HttpResponse, Responder, Scope, delete, get, post, put, web};

pub fn collection_scope() -> Scope {
    web::scope("/models/collections")
        .service(get_collections)
        .service(add_collection)
        .service(get_collection_schema)
        .service(update_collection_schema)
        .service(delete_collection)
        .service(get_collection_items)
        .service(get_collection_item)
        .service(add_collection_item)
        .service(update_collection_item)
        .service(delete_collection_item)
}

#[get("")]
async fn get_collections(app_module: web::Data<Arc<AppModule>>) -> impl Responder {
    let collections = app_module.static_collection_service.get_all_collections();
    match collections {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => get_actix_error(&e),
    }
}
#[get("/{collection_name}/schema")]
async fn get_collection_schema(app_module: web::Data<Arc<AppModule>>, collection_name: web::Path<CollectionName>) -> impl Responder {
    let collection = app_module.static_collection_service.get_collection_schema(&collection_name);
    match collection {
        Ok(schema) => HttpResponse::Ok().json(schema),
        Err(e) => get_actix_error(&e),
    }
}
#[post("/{collection_name}/schema")]
async fn add_collection(app_module: web::Data<Arc<AppModule>>, collection_name: web::Path<CollectionName>, schema: web::Json<CollectionSchema>) -> impl Responder {
    let result = app_module.static_collection_service.add_collection_schema(&collection_name, &schema);
    match result {
        Ok(_) => HttpResponse::Ok().body("Collection schema added successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[put("/{collection_name}/schema")]
async fn update_collection_schema(app_module: web::Data<Arc<AppModule>>, collection_name: web::Path<CollectionName>, schema: web::Json<CollectionSchema>) -> impl Responder {
    let result = app_module.static_collection_service.update_collection_schema(&collection_name, &schema.into_inner());
    match result {
        Ok(_) => HttpResponse::Ok().body("Collection schema updated successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[delete("/{collection_name}")]
async fn delete_collection(app_module: web::Data<Arc<AppModule>>, collection_name: web::Path<CollectionName>) -> impl Responder {
    let result = app_module.static_collection_service.delete_collection(&collection_name);
    match result {
        Ok(_) => HttpResponse::Ok().body("Collection deleted successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[get("/{collection_name}/items/")]
async fn get_collection_items(app_module: web::Data<Arc<AppModule>>, collection_name: web::Path<CollectionName>) -> impl Responder {
    let collection = app_module.static_collection_service.get_collection_items(&collection_name);
    match collection {
        Ok(items) => HttpResponse::Ok().json(items),
        Err(e) => get_actix_error(&e),
    }
}
#[get("/{collection_name}/items/{id}")]
async fn get_collection_item(app_module: web::Data<Arc<AppModule>>, path: web::Path<(CollectionName, CollectionItemId)>) -> impl Responder {
    let (collection_name, id) = path.into_inner();
    let result = app_module.static_collection_service.get_collection_item(&collection_name, id);
    match result {
        Ok(item) => HttpResponse::Ok().json(item),
        Err(e) => get_actix_error(&e),
    }
}
#[post("/{collection_name}/item")]
async fn add_collection_item(app_module: web::Data<Arc<AppModule>>, collection_name: web::Path<CollectionName>, item_data: web::Json<CollectionItem>) -> impl Responder {
    let result = app_module.static_collection_service.create_collection_item(&collection_name, &item_data);
    match result {
        Ok(item_id) => HttpResponse::Ok().json(format!("{}", item_id)),
        Err(e) => get_actix_error(&e),
    }
}
#[put("/{collection_name}/items/{id}")]
async fn update_collection_item(app_module: web::Data<Arc<AppModule>>, path: web::Path<(CollectionName, CollectionItemId)>, item_data: web::Json<CollectionItem>) -> impl Responder {
    let (collection_name, id) = path.into_inner();
    let result = app_module.static_collection_service.update_collection_item(&collection_name, id, &item_data);
    match result {
        Ok(_) => HttpResponse::Ok().body("Collection item updated successfully"),
        Err(e) => get_actix_error(&e),
    }
}
#[delete("/{collection_name}/items/{id}")]
async fn delete_collection_item(app_module: web::Data<Arc<AppModule>>, path: web::Path<(CollectionName, CollectionItemId)>) -> impl Responder {
    let (collection_name, id) = path.into_inner();
    let result = app_module.static_collection_service.delete_collection_item(&collection_name, id);
    match result {
        Ok(_) => HttpResponse::Ok().body("Collection item deleted successfully"),
        Err(e) => get_actix_error(&e),
    }
}
