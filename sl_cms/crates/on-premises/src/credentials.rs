//! Passwords for the accounts this deployment owns: Argon2id hashes in their own rkv database.
//!
//! The credential is deliberately *not* part of the user record (see
//! [`LocalCredentials`](sl_cms_core::repositories::local_credentials::LocalCredentials)): the
//! record is what the CMS reasons about — identity, permissions, whether the account may sign
//! in — and the password is the one thing the CMS never needs to put in a response, a log line
//! or a debug print. Keeping them in separate places makes that structural rather than a rule
//! someone has to remember.
//!
//! Hashes are stored as PHC strings (`$argon2id$v=19$...`), so the parameters used when a
//! password was set travel with it and can be raised later without invalidating credentials.
//! `password-hash` 0.6 generates the random salt internally, so there is no salt handling here
//! to get wrong.

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use rkv::{StoreOptions, Value};
use sl_cms_core::models::user::UserId;
use sl_cms_core::repositories::local_credentials::LocalCredentials;
use sl_cms_core::repositories::user_repository::BoxError;

use crate::repository::{RkvRepository, CREDENTIAL_STORE};

/// Hash `password` with a fresh random salt.
pub fn hash_password(password: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| format!("failed to hash password: {e}"))
}

/// Verify `password` against a stored PHC hash.
///
/// Returns `false` (never an error) for a malformed hash, so a corrupted record cannot be
/// distinguished from a wrong password by the caller.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored_hash)
        .is_ok()
}

impl RkvRepository {
    /// The stored PHC string. The caller must hold the storage lock (see [`RkvRepository::begin`]),
    /// because rkv opens the store on every call and LMDB refuses to open one while another
    /// transaction is active.
    pub(crate) fn stored_password(&self, user_id: &UserId) -> Result<Option<String>, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(CREDENTIAL_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(hash)) => Ok(Some(hash.to_string())),
            _ => Ok(None),
        }
    }
}

impl LocalCredentials for RkvRepository {
    async fn set_password(&self, user_id: &UserId, password: &str) -> Result<(), BoxError> {
        let hash = hash_password(password).map_err(|e| e.to_string())?;
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(CREDENTIAL_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        store.put(&mut writer, user_id.as_bytes(), &Value::Str(&hash))?;
        writer.commit()?;
        Ok(())
    }

    async fn verify_password(&self, user_id: &UserId, password: &str) -> Result<bool, BoxError> {
        // The lock covers the read of the hash and nothing else. It has to cover that much
        // (rkv's `open_single` opens the store, and LMDB refuses to open a database while
        // another transaction is active, which is exactly what a second request is doing),
        // but it must not cover the hashing: Argon2id is deliberately slow, and holding the
        // mutex across it would serialise every sign-in in the process.
        let stored = {
            let _guard = self.begin();
            self.stored_password(user_id)?
        };
        let Some(hash) = stored else {
            return Ok(false);
        };
        Ok(verify_password(password, &hash))
    }

    async fn delete_password(&self, user_id: &UserId) -> Result<(), BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(CREDENTIAL_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        // rkv reports deleting an absent key as an error.
        if store.get(&reader, user_id.as_bytes())?.is_some() {
            let mut writer = env.write()?;
            store.delete(&mut writer, user_id.as_bytes())?;
            writer.commit()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_a_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2"), "unexpected hash format: {hash}");
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn same_password_hashes_differently_because_of_the_salt() {
        let a = hash_password("same").unwrap();
        let b = hash_password("same").unwrap();
        assert_ne!(a, b);
        assert!(verify_password("same", &a) && verify_password("same", &b));
    }

    #[test]
    fn malformed_hash_never_verifies() {
        assert!(!verify_password("anything", "not-a-phc-string"));
        assert!(!verify_password("anything", ""));
    }

    /// A second request must not turn a sign-in into a 500.
    ///
    /// rkv opens the credential store on every read, and the safe backend refuses to open a
    /// database while any other read transaction is live (`attempted to open DB during
    /// transaction`). The adapter's storage lock is what prevents that, so verification has to
    /// take it for the read — this is the regression that made a sign-in fail whenever the admin
    /// UI was loading data at the same time.
    #[test]
    fn verifying_a_password_waits_for_another_request_instead_of_failing() {
        use crate::open_test_repository;
        use sl_cms_core::repositories::local_credentials::LocalCredentials;
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = std::env::temp_dir().join(format!(
            "sl-cms-verify-concurrency-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        // A previous run of the same test in this process: start from a clean environment.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repository = open_test_repository(&dir);
        let user_id = UserId::from("concurrency@example.com");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async {
            repository
                .set_password(&user_id, "correct horse battery staple")
                .await
                .unwrap();
        });

        // Another request, holding the lock and a read transaction for as long as it works.
        let (txn_open, txn_ready) = mpsc::channel();
        let holder_repository = std::sync::Arc::clone(&repository);
        let holder = std::thread::spawn(move || {
            let guard = holder_repository.begin();
            let env = holder_repository.rkv.read().unwrap();
            let reader = env.read().unwrap();
            txn_open.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            // The transaction and the environment are dropped *before* the lock, which is the
            // order they were taken in. Releasing the lock first would leave a live transaction
            // for another thread to open a database under - exactly what LMDB refuses - and the
            // sign-in that slipped into that window failed with `OpenAttemptedDuringTransaction`.
            drop(reader);
            drop(env);
            drop(guard);
        });
        txn_ready.recv().unwrap();

        // The sign-in must wait for it (and then succeed), not fail while the transaction lives.
        let result = runtime
            .block_on(repository.verify_password(&user_id, "correct horse battery staple"))
            .unwrap();
        holder.join().unwrap();
        assert!(result);

        drop(repository);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
