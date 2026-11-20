//! Turns an account password into the two keys the client needs.
//!
//! The password never leaves the device. Argon2id stretches it into a master
//! secret, and HKDF splits that secret into an authentication key (sent to the
//! server, which stores only a hash of it) and an encryption key (kept local,
//! used to unwrap the account root key).

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

const KEY_LEN: usize = 32;
const MIN_SALT_LEN: usize = 16;
const AUTH_INFO: &[u8] = b"note-everywhere/v1/auth-key";
const ENCRYPTION_INFO: &[u8] = b"note-everywhere/v1/encryption-key";

/// Argon2id cost settings. Stored next to the salt so they can be raised later
/// without locking out existing accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Bounds accepted for KDF parameters that arrive from outside, such as a
/// server record. Without a floor, a malicious server could hand back weak
/// parameters so the auth key the client then sends is cheap to brute-force;
/// without a ceiling it could exhaust the client's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfPolicy {
    pub min_memory_kib: u32,
    pub max_memory_kib: u32,
    pub min_iterations: u32,
    pub max_iterations: u32,
    pub max_parallelism: u32,
}

impl KdfPolicy {
    /// For real accounts: at least 19 MiB and 2 passes (the OWASP minimum for
    /// Argon2id), at most 1 GiB, 16 passes and 16 lanes.
    pub const PRODUCTION: Self = Self {
        min_memory_kib: 19 * 1024,
        max_memory_kib: 1024 * 1024,
        min_iterations: 2,
        max_iterations: 16,
        max_parallelism: 16,
    };

    /// Only the upper limits of [`Self::PRODUCTION`]; lets tests and tools use
    /// tiny parameters. Never use it for real accounts.
    pub const LENIENT: Self = Self {
        min_memory_kib: 8,
        min_iterations: 1,
        ..Self::PRODUCTION
    };

    /// Accepts `params` only when every value is inside the policy.
    pub fn check(&self, params: &KdfParams) -> Result<(), KdfError> {
        let ok = (self.min_memory_kib..=self.max_memory_kib).contains(&params.memory_kib)
            && (self.min_iterations..=self.max_iterations).contains(&params.iterations)
            && (1..=self.max_parallelism).contains(&params.parallelism);
        if ok {
            Ok(())
        } else {
            Err(KdfError::OutOfPolicy)
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfError {
    SaltTooShort,
    InvalidParams,
    /// Parameters are valid for Argon2 but outside the accepted [`KdfPolicy`].
    OutOfPolicy,
    DerivationFailed,
}

impl std::fmt::Display for KdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::SaltTooShort => "salt must be at least 16 bytes",
            Self::InvalidParams => "Argon2 parameters are out of range",
            Self::OutOfPolicy => "Argon2 parameters are outside the accepted policy",
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
    fn policy_rejects_weak_and_excessive_params() {
        assert_eq!(KdfPolicy::PRODUCTION.check(&KdfParams::DEFAULT), Ok(()));
        assert_eq!(
            KdfPolicy::PRODUCTION.check(&FAST),
            Err(KdfError::OutOfPolicy)
        );
        let huge = KdfParams {
            memory_kib: u32::MAX,
            ..KdfParams::DEFAULT
        };
        assert_eq!(KdfPolicy::LENIENT.check(&huge), Err(KdfError::OutOfPolicy));
        let no_lanes = KdfParams {
            parallelism: 0,
            ..FAST
        };
        assert_eq!(
            KdfPolicy::LENIENT.check(&no_lanes),
            Err(KdfError::OutOfPolicy)
        );
        assert_eq!(KdfPolicy::LENIENT.check(&FAST), Ok(()));
    }

    #[test]
    fn short_salt_is_rejected() {
        assert!(matches!(
            derive_keys(b"correct horse", b"short", FAST),
            Err(KdfError::SaltTooShort)
        ));
    }
}
