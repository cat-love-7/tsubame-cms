extern crate env_logger;

use std::sync::Arc;
use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use actix_web::middleware::Logger;

use crate::on_premises::page_scopes::collection_scope::collection_scope;
use crate::on_premises::page_scopes::composite_field_scope::composite_field_scope;
use crate::on_premises::page_scopes::file_scope::file_scope;
use crate::on_premises::page_scopes::single_page_scope::single_page_scope;
use crate::{AppModule};



#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body("Hello, on-premises world!")
}

pub async fn start_server(app_module: Arc<AppModule>) -> std::io::Result<()> {

    env_logger::init();

    HttpServer::new(move || {
        let cors = Cors::permissive();
        App::new()
            .wrap(cors)
            .wrap(Logger::default())
            .app_data(web::Data::new(app_module.clone()))
            .service(index)
            .service(collection_scope())
            .service(composite_field_scope())
            .service(single_page_scope())
            .service(file_scope())
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}