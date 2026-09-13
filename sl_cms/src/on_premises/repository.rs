use rkv::{Rkv, SingleStore, StoreOptions};
use rkv::backend::{SafeModeDatabase, SafeModeEnvironment};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub mod composite_field_repository;
pub mod collection_repository;
pub mod single_page_repository;
pub mod image_repository;
pub mod user_repository;

pub struct Repository {
    pub rkv: Arc<RwLock<Rkv<SafeModeEnvironment>>>,
    pub counter_store: SingleStore<SafeModeDatabase>,

    /// One-shot upload tokens: `token -> authorised file name`.
    ///
    /// This deliberately no longer lives in the HTTP layer, and storing the file name
    /// (rather than just presence) stops a token issued for one upload from being
    /// replayed against a different file name.
    upload_keys: Arc<RwLock<HashMap<String, String>>>,

    /// Directory holding uploaded image bytes (derived from `DATA_ROOT`).
    images_dir: PathBuf,
}

impl Repository {
    pub fn new(rkv: Arc<RwLock<Rkv<SafeModeEnvironment>>>, images_dir: PathBuf) -> Self {
        let binding = Arc::clone(&rkv);
        let env = binding.read().unwrap();
        let counter_store = env.open_single("id_counter", StoreOptions::create()).unwrap();
        Repository {
            rkv,
            counter_store,
            upload_keys: Arc::new(RwLock::new(HashMap::new())),
            images_dir,
        }
    }

    pub(crate) fn images_dir(&self) -> &Path {
        &self.images_dir
    }

    pub(crate) fn register_upload_key(&self, key: String, file_name: String) {
        self.upload_keys.write().unwrap().insert(key, file_name);
    }

    pub(crate) fn consume_upload_key(&self, key: &str) -> Option<String> {
        self.upload_keys.write().unwrap().remove(key)
    }
}
