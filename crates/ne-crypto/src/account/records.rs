//! Types stored on the server and secrets held in memory by an unlocked client.

use serde::{Deserialize, Serialize};

use crate::auth;
use crate::key_wrap::WrappedKey;
use crate::keys::{RootKey, WorkspaceKey};
use crate::password::KdfParams;
use crate::serde_b64;

/// Length of the Argon2 salts.
pub const SALT_LEN: usize = 16;

/// Everything the server stores for an account. Contains no plaintext secret:
/// the verifier cannot be reversed into the auth key, and each key is wrapped.
///
/// The salt and parameters must be served to a client before it logs in, so it
/// can derive its auth key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRecord {
    #[serde(with = "serde_b64")]
    pub kdf_salt: [u8; SALT_LEN],
    pub kdf_params: KdfParams,
    /// `auth::auth_verifier` of the password-derived auth key.
    #[serde(with = "serde_b64")]
    pub auth_verifier: [u8; auth::LEN],
    /// `auth::auth_verifier` of the recovery-derived auth key, which lets the
    /// server accept a password reset from someone who holds the recovery key.
    /// All zeros for an account created before recovery verification existed:
    /// such an account cannot be recovered through the server.
    #[serde(with = "serde_b64", default)]
    pub recovery_verifier: [u8; auth::LEN],
    /// Root key wrapped by the password-derived key.
    pub root_key_by_password: WrappedKey,
    /// Root key wrapped by the account recovery key.
    pub root_key_by_recovery: WrappedKey,
    /// Workspace key wrapped by the root key.
    pub workspace_key: WrappedKey,
}

impl AccountRecord {
    /// Whether the server can verify a recovery attempt for this account.
    pub fn supports_recovery(&self) -> bool {
        self.recovery_verifier != [0u8; auth::LEN]
    }
}

/// The part of an [`AccountRecord`] a client needs to recover a forgotten
/// password: the wrapped keys the recovery key opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryRecord {
    pub root_key_by_recovery: WrappedKey,
    pub workspace_key: WrappedKey,
}

/// What the server stores for the vault. Independent of [`AccountRecord`]: the
/// vault key is wrapped only by the vault password and the vault recovery key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultRecord {
    #[serde(with = "serde_b64")]
    pub kdf_salt: [u8; SALT_LEN],
    pub kdf_params: KdfParams,
    /// Vault key wrapped by the vault-password-derived key.
    pub vault_key_by_password: WrappedKey,
    /// Vault key wrapped by the vault recovery key.
    pub vault_key_by_recovery: WrappedKey,
}

/// Keys an unlocked account holds in memory. Wiped on drop.
#[derive(Debug)]
pub struct AccountSecrets {
    pub root_key: RootKey,
    pub workspace_key: WorkspaceKey,
}
