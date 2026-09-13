use std::sync::Arc;

use crate::services::{collection_service::CollectionService, composite_field_service::CompositeFieldService, image_service::ImageService, single_page_service::SinglePageService};

pub mod repository;
pub mod server;
pub mod page_scopes;


pub struct AppModule{
    static_collection_service: CollectionService<
        repository::Repository,
        repository::Repository,
        repository::Repository,
    >,
    static_single_page_service: SinglePageService<
        repository::Repository,
        repository::Repository,
    >,
    static_composite_field_service: CompositeFieldService<
        repository::Repository,
    >,
    static_image_service: ImageService<
        repository::Repository,
    >,
    repository: Arc<repository::Repository>,
}

impl AppModule {
    pub fn new() -> Self {
        use std::fs;
        use std::sync::Arc;
        use rkv::{Manager, Rkv};
        use rkv::backend::{SafeMode, SafeModeEnvironment};

        let path = std::path::Path::new("./data/on_premises/rkv_data");
        fs::create_dir_all(path).unwrap();
        let mut manager = Manager::<SafeModeEnvironment>::singleton().write().unwrap();
        let created_arc = manager.get_or_create(path, Rkv::new::<SafeMode>).unwrap();
        let repository = Arc::new(repository::Repository::new(created_arc));
        let static_collection_service =
            CollectionService::new(
                repository.clone(),
                repository.clone(),
                repository.clone()
            );
        let static_single_page_service =
            SinglePageService::new(
                repository.clone(),
                repository.clone()
            );
        let static_composite_field_service =
            CompositeFieldService::new(repository.clone());
        let static_image_service =
            ImageService::new(repository.clone());
        return AppModule {
            static_collection_service,
            static_single_page_service,
            static_composite_field_service,
            static_image_service,
            repository,
        }
    }
    
}