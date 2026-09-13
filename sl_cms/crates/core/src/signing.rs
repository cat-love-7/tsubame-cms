//! HMAC-SHA256 signing, shared by the two things that hand out a digest a receiver has to
//! verify: webhook deliveries and preview links.
//!
//! One implementation means one place to get the comparison right - `verify` compares in
//! constant time, so a caller cannot learn the expected digest one byte at a time.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// Lower-case hex HMAC-SHA256 of `message` under `secret`.
pub fn sign(secret: &[u8], message: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts a key of any length");
    mac.update(message);
    to_hex(&mac.finalize().into_bytes())
}

/// Whether `signature` is the HMAC of `message` under `secret`.
///
/// Anything that is not valid hex is simply not a valid signature.
pub fn verify(secret: &[u8], message: &[u8], signature: &str) -> bool {
    let Some(expected) = from_hex(signature) else {
        return false;
    };
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts a key of any length");
    mac.update(message);
    mac.verify_slice(&expected).is_ok()
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing into a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push((high * 16 + low) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The expected digest was produced outside this codebase (Python's `hmac` module), so
    /// a change here that still round-trips internally would still be caught.
    #[test]
    fn signs_with_hmac_sha256_in_lower_case_hex() {
        assert_eq!(
            sign(b"test-secret", br#"{"hello":"world"}"#),
            "84cc33df716ed0b0598f07437c94069ace3730358778a592bd6bbd1423d111f3"
        );
        assert_eq!(sign(b"secret", b"{}").len(), 64);
    }

    #[test]
    fn verifies_only_its_own_signature() {
        let signature = sign(b"secret", b"body");
        assert!(verify(b"secret", b"body", &signature));

        // A different secret, a different message, and a tampered digest all fail.
        assert!(!verify(b"other", b"body", &signature));
        assert!(!verify(b"secret", b"other body", &signature));
        assert!(!verify(b"secret", b"body", &signature.replace('a', "b")));
        // ...as does something that is not hex at all.
        assert!(!verify(b"secret", b"body", "not-a-signature"));
        assert!(!verify(b"secret", b"body", "abc"));
    }

    #[test]
    fn hex_round_trips() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert_eq!(from_hex("").unwrap(), Vec::<u8>::new());
        assert!(from_hex("zz").is_none());
    }
}
