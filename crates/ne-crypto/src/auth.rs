//! Server-side login verifier.
//!
//! The client sends its auth key (see [`crate::password`]); the server stores
//! only `auth_verifier(auth_key)`. A plain domain-separated SHA-256 is enough
//! because the auth key is already a 256-bit output of Argon2id and HKDF, so
//! there is nothing left to brute-force. If the server's storage leaks, the
//! verifier cannot be turned back into an auth key.
//!
//! This module has no OS-specific dependencies and is intended to also build
//! for `wasm32-unknown-unknown` (the sync Worker uses it).

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const VERIFIER_DOMAIN: &[u8] = b"note-everywhere/v1/auth-verifier";

/// Length in bytes of an auth key and of its verifier.
pub const LEN: usize = 32;

/// Computes the value a server stores for an account.
pub fn auth_verifier(auth_key: &[u8; LEN]) -> [u8; LEN] {
    let mut hasher = Sha256::new();
    hasher.update(VERIFIER_DOMAIN);
    hasher.update(auth_key);
    hasher.finalize().into()
}

/// Checks a presented auth key against a stored verifier in constant time.
pub fn verify(auth_key: &[u8; LEN], stored_verifier: &[u8; LEN]) -> bool {
    auth_verifier(auth_key).ct_eq(stored_verifier).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_matches_only_the_same_key() {
        let key = [7u8; LEN];
        let verifier = auth_verifier(&key);
        assert!(verify(&key, &verifier));
        assert!(!verify(&[8u8; LEN], &verifier));
    }

    #[test]
    fn verifier_is_not_the_key_and_is_domain_separated() {
        let key = [7u8; LEN];
        let verifier = auth_verifier(&key);
        assert_ne!(verifier, key);
        let plain: [u8; LEN] = Sha256::digest(key).into();
        assert_ne!(verifier, plain);
    }

    #[test]
    fn verifier_known_answer() {
        // SHA-256("note-everywhere/v1/auth-verifier" || 32 zero bytes)
        let mut hasher = Sha256::new();
        hasher.update(b"note-everywhere/v1/auth-verifier");
        hasher.update([0u8; 32]);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(auth_verifier(&[0u8; LEN]), expected);
    }
}
