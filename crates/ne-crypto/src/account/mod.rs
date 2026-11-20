//! High-level account and vault operations.
//!
//! These compose [`crate::password`],
//! [`crate::key_wrap`] and [`crate::recovery`] so callers never handle raw wrapping
//! keys. The records they produce ([`AccountRecord`], [`VaultRecord`]) are
//! what the server stores: salts, KDF parameters, a login verifier and wrapped
//! blobs, nothing else.
//!
//! The account and the vault are unlocked independently. Nothing derived from
//! the account password or held in [`AccountSecrets`] can open a
//! [`VaultRecord`].

mod login;
mod records;
mod vault;

pub use login::{
    AccountRecovery, LoginAttempt, NewAccount, PasswordChange, begin_login, change_password,
    create_account, recover_account, unlock_with_password, unlock_with_recovery_key,
};
pub use records::{AccountRecord, AccountSecrets, RecoveryRecord, SALT_LEN, VaultRecord};
pub use vault::{
    NewVault, change_vault_password, create_vault, unlock_vault, unlock_vault_with_recovery_key,
};

use crate::keys::{SecretKey, WrappingKey};
use crate::password::{self, DerivedKeys, KdfError, KdfParams, KdfPolicy};
use crate::random;

/// Why an account or vault operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountError {
    /// The password is empty.
    EmptyPassword,
    /// Key derivation failed or its parameters are rejected.
    Kdf(KdfError),
    /// Wrong password or recovery key, or the stored record was modified. The
    /// three cases cannot be told apart, on purpose.
    WrongSecret,
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPassword => f.write_str("password must not be empty"),
            Self::Kdf(err) => write!(f, "{err}"),
            Self::WrongSecret => f.write_str("wrong password or recovery key"),
        }
    }
}

impl std::error::Error for AccountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Kdf(err) => Some(err),
            _ => None,
        }
    }
}

impl From<KdfError> for AccountError {
    fn from(err: KdfError) -> Self {
        Self::Kdf(err)
    }
}

impl From<crate::aead::AeadError> for AccountError {
    fn from(_: crate::aead::AeadError) -> Self {
        Self::WrongSecret
    }
}

/// Derives the password keys after checking the password and the parameters.
fn derive(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<DerivedKeys, AccountError> {
    if password.is_empty() {
        return Err(AccountError::EmptyPassword);
    }
    policy.check(&params)?;
    Ok(password::derive_keys(password, salt, params)?)
}

fn wrapping_key_of(keys: &DerivedKeys) -> WrappingKey {
    WrappingKey::from_bytes(keys.encryption_key())
}

fn fresh_salt() -> [u8; SALT_LEN] {
    random::array::<SALT_LEN>()
}
