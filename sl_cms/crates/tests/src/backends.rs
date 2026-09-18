//! The two backends the contract suite drives.
//!
//! Each wrapper owns the scratch storage a run needs and knows how to put its routes together.
//! The adapters themselves cannot carry this: it is the harness's view of them, so it belongs
//! on this side of the dependency.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;

use sl_cms_aws::AwsRepository;
use sl_cms_core::app_module::AppModule;
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_core::http;
use sl_cms_core::models::user::{Permission, User};
use sl_cms_core::repositories::user_repository::UserRepository;
use sl_cms_on_premises::repository::RkvRepository;

use crate::{ADMIN_EMAIL, ADMIN_PASSWORD, TEST_SECRET, TEST_TOKEN_TTL_HOURS, TestBackend};

/// Mint a token the way the harness's issuer would, so a backend can sign someone in without a
/// password endpoint.
fn mint(user: &User) -> String {
    TokenIssuer::new(TEST_SECRET, TEST_TOKEN_TTL_HOURS)
        .issue(user)
        .expect("a token")
        .0
}

/// The local adapter: an rkv environment and an image directory, in a scratch directory of
/// their own that goes away when the run does.
pub struct OnPremises {
    repository: Arc<RkvRepository>,
    dir: PathBuf,
}

impl TestBackend for OnPremises {
    type Storage = RkvRepository;

    const SERVES_IMAGE_BYTES: bool = true;
    const PASSWORD_LOGIN: bool = true;

    async fn open(hint: &str) -> Self {
        // Under `target/`, so a leaked directory from a panicking run is ignored by git and
        // is thrown away by `cargo clean` like everything else. The path is derived from the
        // test binary rather than `CARGO_TARGET_TMPDIR`, which only exists for integration
        // tests and not for this library.
        let dir = scratch_root().join(hint);
        std::fs::create_dir_all(&dir).expect("could not create the scratch directory");
        OnPremises {
            repository: sl_cms_on_premises::open_test_repository(&dir),
            dir,
        }
    }

    fn storage(&self) -> Arc<Self::Storage> {
        self.repository.clone()
    }

    fn router(module: Arc<AppModule<Self::Storage>>) -> Router {
        sl_cms_on_premises::build_router(module, test_cors())
    }

    async fn sign_in_admin(&self, module: &Arc<AppModule<Self::Storage>>) -> String {
        module
            .auth_service
            .bootstrap_admin(Some(ADMIN_EMAIL), Some(ADMIN_PASSWORD), None)
            .await
            .unwrap_or_else(|e| panic!("bootstrap admin: {e}"))
            .expect("the store was empty, so an administrator is created");
        let response = module
            .auth_service
            .login(ADMIN_EMAIL, ADMIN_PASSWORD)
            .await
            .expect("the administrator can sign in");
        response.token
    }

    async fn user_for(&self, username: &str) -> User {
        self.repository
            .get_user_from_username(username)
            .await
            .expect("a readable store")
            .unwrap_or_else(|| panic!("no account for {username}"))
    }

    async fn create_account(
        &self,
        username: &str,
        is_admin: bool,
        permission: Permission,
    ) -> Result<User, String> {
        // The suite creates accounts through the endpoint here, because that is where the
        // password gets set; this exists for a backend that has no such endpoint.
        let user = User::new(username, is_admin, permission);
        self.repository
            .add_user(&user)
            .await
            .map(|_| user)
            .map_err(|e| e.to_string())
    }
}

impl Drop for OnPremises {
    fn drop(&mut self) {
        // Best effort: the rkv environment may still be open.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The AWS adapter: a table of its own in DynamoDB Local, and the emulator's MinIO for bytes.
///
/// Unlike the local one this needs something running, so it fails with instructions rather
/// than quietly passing when `docker compose` has not been started. `scripts/test-rust.sh`
/// checks first and skips this file with a note.
pub struct Aws {
    repository: Arc<AwsRepository>,
}

impl TestBackend for Aws {
    type Storage = AwsRepository;

    const SERVES_IMAGE_BYTES: bool = false;
    const PASSWORD_LOGIN: bool = false;

    async fn open(hint: &str) -> Self {
        let (repository, _table) =
            sl_cms_aws::open_test_repository(hint)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "could not open a DynamoDB Local table for {hint}: {e}\n\
                     start the emulators with `docker compose -f sl_cms/docker-compose.yml up -d`"
                    )
                });
        Aws { repository }
    }

    fn storage(&self) -> Arc<Self::Storage> {
        self.repository.clone()
    }

    fn router(module: Arc<AppModule<Self::Storage>>) -> Router {
        sl_cms_aws::build_router(module, test_cors())
    }

    async fn sign_in_admin(&self, _module: &Arc<AppModule<Self::Storage>>) -> String {
        // No password endpoint to sign in through: this is what a deployment does once Cognito
        // has authenticated someone — resolve the local record and mint the token for it.
        let user = User::new(ADMIN_EMAIL, true, Permission::admin());
        self.repository
            .add_user(&user)
            .await
            .expect("the administrator record");
        mint(&user)
    }

    async fn user_for(&self, username: &str) -> User {
        self.repository
            .get_user_from_username(username)
            .await
            .expect("a readable table")
            .unwrap_or_else(|| panic!("no account for {username}"))
    }

    async fn create_account(
        &self,
        username: &str,
        is_admin: bool,
        permission: Permission,
    ) -> Result<User, String> {
        let user = User::new(username, is_admin, permission);
        self.repository
            .add_user(&user)
            .await
            .map(|_| user)
            .map_err(|e| e.to_string())
    }
}

impl Drop for Aws {
    fn drop(&mut self) {
        // The table belongs to this test, so it is dropped with it. `Drop` cannot await, so the
        // delete is handed to the runtime the test runs on — best effort, which is all this
        // needs to be: the emulator keeps its tables in memory.
        let repository = self.repository.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = repository.delete_table().await;
            });
        }
    }
}

/// Where a run's scratch storage lives: `target/tmp/sl-cms-tests`, next to the test binaries.
fn scratch_root() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary has a path");
    // target/<profile>/deps/<binary> -> target
    let target = exe
        .parent()
        .and_then(|deps| deps.parent())
        .and_then(|profile| profile.parent())
        .expect("the test binary lives under target/<profile>/deps");
    target.join("tmp").join("sl-cms-tests")
}

/// The origins the Angular client uses, as a deployment would allow.
pub fn test_cors() -> tower_http::cors::CorsLayer {
    http::cors_layer(&["http://localhost:4200".to_string()])
}
