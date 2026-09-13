//! Password hashing (Argon2id).
//!
//! Hashes are stored as PHC strings (`$argon2id$v=19$...`), so the parameters used at
//! creation travel with the hash and can be raised later without invalidating existing
//! credentials.
//!
//! `password-hash` 0.6 generates the random salt internally (`getrandom` feature), so
//! there is no salt handling here to get wrong.

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;

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

#[cfg(test)]
mod tests {
    use super::{hash_password, verify_password};

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
