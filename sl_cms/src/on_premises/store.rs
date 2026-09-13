use std::fs;
use std::sync::{Arc, RwLock};
use rkv::{Manager, Rkv, SingleStore, Value, StoreOptions};
use rkv::backend::{SafeMode, SafeModeDatabase, SafeModeEnvironment};



pub fn get_rkv() -> Arc<RwLock<Rkv<SafeModeEnvironment>>> {
    let path = std::path::Path::new("./data/on_premises/rkv_data");
    fs::create_dir_all(path).unwrap();
    let mut manager = Manager::<SafeModeEnvironment>::singleton().write().unwrap();
    let created_arc = manager.get_or_create(path, Rkv::new::<SafeMode>).unwrap();
    created_arc
}