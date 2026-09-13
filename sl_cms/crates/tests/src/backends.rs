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
use sl_cms_core::http::{self, AppState};
use sl_cms_on_premises::repository::Repository;

use crate::TestBackend;

/// The local adapter: an rkv environment and an image directory, in a scratch directory of
/// their own that goes away when the run does.
pub struct OnPremises {
    repository: Arc<Repository>,
    dir: PathBuf,
}

impl TestBackend for OnPremises {
    type Storage = Repository;

    const SERVES_IMAGE_BYTES: bool = true;

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
    table: String,
}

impl TestBackend for Aws {
    type Storage = AwsRepository;

    const SERVES_IMAGE_BYTES: bool = false;

    async fn open(hint: &str) -> Self {
        let (repository, table) = sl_cms_aws::open_test_repository(hint)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "could not open a DynamoDB Local table for {hint}: {e}\n\
                     start the emulators with `docker compose -f sl_cms/docker-compose.yml up -d`"
                )
            });
        Aws { repository, table }
    }

    fn storage(&self) -> Arc<Self::Storage> {
        self.repository.clone()
    }

    fn router(module: Arc<AppModule<Self::Storage>>) -> Router {
        // No extra routes: the object store serves the bytes.
        http::router(module, test_cors())
    }
}

impl Drop for Aws {
    fn drop(&mut self) {
        // The table is per run, so leaving it behind would only accumulate; deleting it needs
        // the bridge because `Drop` cannot await.
        let _ = self.repository.delete_table_blocking();
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
