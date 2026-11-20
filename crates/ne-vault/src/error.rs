//! The error type returned by every fallible `ne-vault` operation.

use ne_crypto::account::AccountError;

use crate::item::ItemId;
use crate::lock::SensitiveAction;

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, VaultError>;

/// Everything that can go wrong in `ne-vault`. Messages never contain secret
/// values or item titles.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    /// The operation needs the vault to be unlocked.
    #[error("the vault is locked")]
    Locked,
    /// The vault is unlocked but the grace period since the last
    /// authentication has passed; call `Vault::reauthenticate_with_password`.
    #[error("re-authentication is required to {0}")]
    ReauthRequired(SensitiveAction),
    /// No live item has this id.
    #[error("item {0} not found")]
    NotFound(ItemId),
    /// The item kind has no such field.
    #[error("the item has no such field")]
    NoSuchField,
    /// The item was rejected before sealing.
    #[error("invalid item: {0}")]
    InvalidItem(&'static str),
    /// Wrong password or recovery key, or a bad key-derivation setting.
    #[error("unlock failed: {0}")]
    Account(#[from] AccountError),
    /// A stored record is malformed, was tampered with, or belongs to another
    /// item or key. Opaque on purpose.
    #[error("a stored vault record is corrupt or does not match this key")]
    Corrupt,
    /// A stored record uses a format version this build does not know.
    #[error("unsupported vault record version {0}")]
    UnsupportedVersion(u8),
    /// Sealing failed.
    #[error("encrypting a vault item failed")]
    Crypto,
    /// The CRDT layer rejected sync bytes or a version vector.
    #[error("sync data rejected: {0}")]
    Sync(String),
    /// A CSV file could not be read at all (row-level problems are reported
    /// per row instead).
    #[error("csv import failed: {0}")]
    Import(#[from] crate::import::ImportError),
}

impl From<loro::LoroError> for VaultError {
    fn from(err: loro::LoroError) -> Self {
        Self::Sync(err.to_string())
    }
}

impl From<loro::LoroEncodeError> for VaultError {
    fn from(err: loro::LoroEncodeError) -> Self {
        Self::Sync(err.to_string())
    }
}
