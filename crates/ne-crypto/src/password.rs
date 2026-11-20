//! Turns an account password into the two keys the client needs.
//!
//! The password never leaves the device. Argon2id stretches it into a master
//! secret, and HKDF splits that secret into an authentication key (sent to the
//! server, which stores only a hash of it) and an encryption key (kept local,
//! used to unwrap the account root key).

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

const KEY_LEN: usize = 32;
const MIN_SALT_LEN: usize = 16;
const AUTH_INFO: &[u8] = b"note-everywhere/v1/auth-key";
const ENCRYPTION_INFO: &[u8] = b"note-everywhere/v1/encryption-key";

/// Argon2id cost settings. Stored next to the salt so they can be raised later
/// without locking out existing accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// Starting point pending the per-platform measurements recorded in
    /// `docs/decisions/0003-password-key-derivation.md`.
    pub const DEFAULT: Self = Self {
        memory_kib: 256 * 1024,
        iterations: 3,
        parallelism: 4,
    };
}

/// Keys derived from one password. Wiped from memory when dropped.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct DerivedKeys {
    auth_key: [u8; KEY_LEN],
    encryption_key: [u8; KEY_LEN],
}

impl DerivedKeys {
    /// Proves knowledge of the password to the server.
    pub fn auth_key(&self) -> &[u8; KEY_LEN] {
        &self.auth_key
    }

    /// Unwraps the account root key. Must never be sent anywhere.
    pub fn encryption_key(&self) -> &[u8; KEY_LEN] {
        &self.encryption_key
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum KdfError {
    SaltTooShort,
    InvalidParams,
    DerivationFailed,
}

impl std::fmt::Display for KdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::SaltTooShort => "salt must be at least 16 bytes",
            Self::InvalidParams => "Argon2 parameters are out of range",
            Self::DerivationFailed => "key derivation failed",
        };
        f.write_str(message)
    }
}

impl std::error::Error for KdfError {}

pub fn derive_keys(
    password: &[u8],
    salt: &[u8],
    params: KdfParams,
) -> Result<DerivedKeys, KdfError> {
    if salt.len() < MIN_SALT_LEN {
        return Err(KdfError::SaltTooShort);
    }
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| KdfError::InvalidParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);

    let mut master = [0u8; KEY_LEN];
    argon
        .hash_password_into(password, salt, &mut master)
        .map_err(|_| KdfError::DerivationFailed)?;

    let hkdf = Hkdf::<Sha256>::new(Some(salt), &master);
    master.zeroize();

    let mut keys = DerivedKeys {
        auth_key: [0u8; KEY_LEN],
        encryption_key: [0u8; KEY_LEN],
    };
    hkdf.expand(AUTH_INFO, &mut keys.auth_key)
        .map_err(|_| KdfError::DerivationFailed)?;
    hkdf.expand(ENCRYPTION_INFO, &mut keys.encryption_key)
        .map_err(|_| KdfError::DerivationFailed)?;
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small costs keep the test suite fast; production costs are measured
    /// by the `argon2_bench` example.
    const FAST: KdfParams = KdfParams {
        memory_kib: 1024,
        iterations: 1,
        parallelism: 1,
    };
    const SALT: &[u8] = b"0123456789abcdef";

    #[test]
    fn same_inputs_give_same_keys() {
        let first = derive_keys(b"correct horse", SALT, FAST).expect("derive");
        let second = derive_keys(b"correct horse", SALT, FAST).expect("derive");
        assert_eq!(first.auth_key(), second.auth_key());
        assert_eq!(first.encryption_key(), second.encryption_key());
    }

    #[test]
    fn auth_key_does_not_reveal_encryption_key() {
        let keys = derive_keys(b"correct horse", SALT, FAST).expect("derive");
        assert_ne!(keys.auth_key(), keys.encryption_key());
    }

    #[test]
    fn different_password_or_salt_changes_both_keys() {
        let base = derive_keys(b"correct horse", SALT, FAST).expect("derive");
        let other_password = derive_keys(b"correct horsf", SALT, FAST).expect("derive");
        let other_salt = derive_keys(b"correct horse", b"fedcba9876543210", FAST).expect("derive");
        assert_ne!(base.auth_key(), other_password.auth_key());
        assert_ne!(base.encryption_key(), other_password.encryption_key());
        assert_ne!(base.auth_key(), other_salt.auth_key());
        assert_ne!(base.encryption_key(), other_salt.encryption_key());
    }

    #[test]
    fn short_salt_is_rejected() {
        assert!(matches!(
            derive_keys(b"correct horse", b"short", FAST),
            Err(KdfError::SaltTooShort)
        ));
    }
}
