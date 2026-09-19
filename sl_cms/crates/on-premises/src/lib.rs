//! On-premises storage adapter: rkv (LMDB) for structured data, local filesystem for
//! uploaded image bytes.
//!
//! The HTTP layer is shared (see [`sl_cms_core::http`]); this module only provides the
//! composition root for the `on-premises` feature.

use std::sync::Arc;

use rkv::backend::{SafeMode, SafeModeEnvironment};
use rkv::{Manager, Rkv};

use sl_cms_core::app_module::AppModule;
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::config::Config;
use sl_cms_core::password_reset::PasswordResetIssuer;
use sl_cms_core::preview_link::PreviewLinkIssuer;

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

/// Where this backend keeps everything, under the configured data root.
///
/// The layout belongs to the adapter rather than to the shared configuration: another
/// backend reads `data_root` too, but it has no use for an rkv environment or an image
/// directory.
pub fn storage_dir(config: &Config) -> std::path::PathBuf {
    config.data_root.join("on_premises")
}

/// The rkv (LMDB) environment directory.
pub fn rkv_dir(config: &Config) -> std::path::PathBuf {
    storage_dir(config).join("rkv_data")
}

/// Where uploaded image bytes live.
pub fn images_dir(config: &Config) -> std::path::PathBuf {
    storage_dir(config).join("images")
}

/// Build the on-premises composition root described by `config`.
pub fn build_app_module(config: &Config) -> AppModule<repository::RkvRepository> {
    let rkv_dir = rkv_dir(config);
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

    let repository = Arc::new(repository::RkvRepository::new(env, images_dir(config)));
    let token_issuer = TokenIssuer::new(&config.jwt_secret, config.token_ttl_hours);
    let notifier = sl_cms_core::webhook::build_notifier(
        config.webhook_urls.clone(),
        config.webhook_secret.clone(),
    );
    // Preview links are signed with the same secret as the tokens: it is the one secret the
    // deployment already has to set, and the message carries its own prefix so a signature
    // can never be replayed as the other kind.
    let preview_links = PreviewLinkIssuer::new(&config.jwt_secret, config.preview_link_ttl_minutes);
    // Same secret, different prefix: a reset signature can never be replayed as a preview
    // link, a webhook body or a token.
    let password_resets =
        PasswordResetIssuer::new(&config.jwt_secret, config.password_reset_ttl_minutes);
    AppModule::new(
        repository,
        token_issuer,
        notifier,
        preview_links,
        password_resets,
        config.limits,
    )
}

/// The whole HTTP surface for this backend.
///
/// The shared router plus the routes for serving and accepting image bytes, which this backend
/// keeps itself. Composing them here — rather than compiling them in or out of the shared
/// layer — is what lets the same `http` module serve a backend whose object store does all of
/// that.
pub fn build_router(
    module: std::sync::Arc<AppModule<repository::RkvRepository>>,
    cors: tower_http::cors::CorsLayer,
) -> axum::Router {
    // Everything this backend has that a deployment without local storage does not: serving and
    // accepting image bytes, and the password endpoints (it is the CMS, not an identity
    // provider, that checks a password here).
    let extra_public = sl_cms_core::http::local_images::public_routes()
        .merge(sl_cms_core::http::password_auth::public_routes())
        .merge(sl_cms_core::http::capabilities::routes(
            sl_cms_core::models::capabilities::Capabilities::on_premises(
                module.limits.max_image_bytes,
            ),
        ));
    let extra_protected =
        sl_cms_core::http::local_images::protected_routes(module.limits.max_image_bytes)
            .merge(sl_cms_core::http::password_auth::protected_routes());

    sl_cms_core::http::router_with(module, cors, extra_public, extra_protected)
}

/// A fresh rkv environment plus a repository over it, for the integration tests.
///
/// The HTTP suite drives a real backend, and this adapter is the one that exists. Keeping the
/// setup here means the tests never open an LMDB environment themselves, so a second adapter
/// only has to offer an equivalent helper. It is public because the contract suite is a crate
/// of its own, so it cannot reach a `#[cfg(test)]` item here.
pub fn open_test_repository(dir: &std::path::Path) -> Arc<repository::RkvRepository> {
    let env = {
        let mut manager = Manager::<SafeModeEnvironment>::singleton()
            .write()
            .expect("rkv manager lock poisoned");
        manager
            .get_or_create_with_capacity(dir, MAX_NAMED_DATABASES, Rkv::with_capacity::<SafeMode>)
            .expect("failed to open the rkv environment")
    };
    Arc::new(repository::RkvRepository::new(env, dir.join("images")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The adapter owns its layout: the shared config only knows the data root.
    #[test]
    fn derives_storage_paths_from_the_data_root() {
        let config = Config {
            data_root: PathBuf::from("/tmp/cms"),
            ..Config::default()
        };
        assert_eq!(storage_dir(&config), PathBuf::from("/tmp/cms/on_premises"));
        assert_eq!(
            rkv_dir(&config),
            PathBuf::from("/tmp/cms/on_premises/rkv_data")
        );
        assert_eq!(
            images_dir(&config),
            PathBuf::from("/tmp/cms/on_premises/images")
        );
    }
}
