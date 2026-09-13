//! On-premises storage adapter: rkv (LMDB) for structured data, local filesystem for
//! uploaded image bytes.
//!
//! The HTTP layer is shared (see [`crate::http`]); this module only provides the
//! composition root for the `on-premises` feature.

use std::sync::Arc;

use rkv::backend::{SafeMode, SafeModeEnvironment};
use rkv::{Manager, Rkv};

use crate::app_module::AppModule;
use crate::auth::token::TokenIssuer;
use crate::config::Config;

pub mod repository;

/// Number of named LMDB databases the environment is allowed to open.
///
/// rkv's default ([`rkv::env::DEFAULT_MAX_DBS`]) is only 5, but this adapter needs one
/// fixed database per concern (`id_counter`, `collection_schema`,
/// `composite_field_schema`, `single_page_schema`, `single_page_item`, `image`, `user`)
/// *plus* one named database per collection (`collection_<name>`). With the default the
/// environment fails with "environment maxdbs reached" almost immediately.
///
/// LMDB never releases a named-database slot once used, so this remains a real ceiling on
/// the number of collections until the per-collection database layout is replaced by
/// composite keys inside a single database.
pub const MAX_NAMED_DATABASES: std::os::raw::c_uint = 512;

/// Build the on-premises composition root described by `config`.
pub fn build_app_module(config: &Config) -> AppModule<repository::Repository> {
    let rkv_dir = config.rkv_dir();
    std::fs::create_dir_all(&rkv_dir).expect("failed to create the rkv data directory");

    let env = {
        let mut manager = Manager::<SafeModeEnvironment>::singleton()
            .write()
            .expect("rkv manager lock poisoned");
        manager
            .get_or_create_with_capacity(
                rkv_dir.as_path(),
                MAX_NAMED_DATABASES,
                Rkv::with_capacity::<SafeMode>,
            )
            .expect("failed to open the rkv environment")
    };

    let repository = Arc::new(repository::Repository::new(env, config.images_dir()));
    let token_issuer = TokenIssuer::new(&config.jwt_secret, config.token_ttl_hours);
    let notifier = crate::webhook::build_notifier(
        config.webhook_urls.clone(),
        config.webhook_secret.clone(),
    );
    AppModule::new(repository, token_issuer, notifier)
}
