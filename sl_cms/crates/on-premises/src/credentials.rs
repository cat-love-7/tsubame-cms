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

use crate::repository::{Repository, CREDENTIAL_STORE};

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

impl Repository {
    /// The stored PHC string, without the storage lock (`delete_user` already holds it).
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

impl LocalCredentials for Repository {
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
        // No lock: reading a credential is a snapshot read, and hashing is deliberately slow —
        // holding a mutex across it would serialise every sign-in in the process.
        let Some(hash) = self.stored_password(user_id)? else {
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
}
