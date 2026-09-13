use rkv::{Rkv, SingleStore, StoreOptions};
use rkv::backend::{SafeModeDatabase, SafeModeEnvironment};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub mod composite_field_repository;
pub mod collection_repository;
pub mod single_page_repository;
pub mod image_repository;
pub mod user_repository;

pub struct Repository {
    pub rkv: Arc<RwLock<Rkv<SafeModeEnvironment>>>,
    pub counter_store: SingleStore<SafeModeDatabase>,
    
    pub file_upload_keys: Arc<RwLock<HashMap<String, String>>>,
}

impl Repository {
    pub fn new(rkv: Arc<RwLock<Rkv<SafeModeEnvironment>>>) -> Self {
        let binding = Arc::clone(&rkv);
        let env = binding.read().unwrap();
        let counter_store = env.open_single("id_counter", StoreOptions::create()).unwrap();
        Repository {
            rkv,
            counter_store,
            file_upload_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
