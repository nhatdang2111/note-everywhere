//! Vault creation, unlock, recovery and password change.
//!
//! The vault key is random and wrapped only by a key from the second password
//! (own salt, own KDF parameters) and by the vault recovery key. It is never
//! wrapped by the account root key.

use crate::key_wrap::{WrapContext, unwrap_key, wrap_key};
use crate::keys::VaultKey;
use crate::password::{KdfParams, KdfPolicy};
use crate::recovery::{RecoveryKey, RecoveryKind};

use super::records::VaultRecord;
use super::{AccountError, derive, fresh_salt, wrapping_key_of};

/// Result of [`create_vault`].
#[derive(Debug)]
pub struct NewVault {
    /// Keep in memory only while the vault is unlocked.
    pub vault_key: VaultKey,
    /// Upload to the server.
    pub record: VaultRecord,
    /// Show to the user once and require confirmation that it is saved.
    pub recovery_key: RecoveryKey,
}

/// Creates a vault protected by `second_password`.
pub fn create_vault(
    second_password: &[u8],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<NewVault, AccountError> {
    let vault_key = VaultKey::generate();
    let recovery_key = RecoveryKey::generate();
    let record = wrap_vault_key(&vault_key, &recovery_key, second_password, params, policy)?;
    Ok(NewVault {
        vault_key,
        record,
        recovery_key,
    })
}

fn wrap_vault_key(
    vault_key: &VaultKey,
    recovery_key: &RecoveryKey,
    password: &[u8],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<VaultRecord, AccountError> {
    let salt = fresh_salt();
    let keys = derive(password, &salt, params, policy)?;
    Ok(VaultRecord {
        kdf_salt: salt,
        kdf_params: params,
        vault_key_by_password: wrap_key(
            &wrapping_key_of(&keys),
            WrapContext::VaultKeyByPassword,
            vault_key,
        )?,
        vault_key_by_recovery: wrap_key(
            &recovery_key.wrapping_key(RecoveryKind::Vault),
            WrapContext::VaultKeyByRecovery,
            vault_key,
        )?,
    })
}

/// Unlocks the vault with the second password.
pub fn unlock_vault(
    second_password: &[u8],
    record: &VaultRecord,
    policy: &KdfPolicy,
) -> Result<VaultKey, AccountError> {
    let keys = derive(second_password, &record.kdf_salt, record.kdf_params, policy)?;
    Ok(unwrap_key(
        &wrapping_key_of(&keys),
        WrapContext::VaultKeyByPassword,
        &record.vault_key_by_password,
    )?)
}

/// Unlocks the vault with the vault recovery key.
pub fn unlock_vault_with_recovery_key(
    recovery_key: &RecoveryKey,
    record: &VaultRecord,
) -> Result<VaultKey, AccountError> {
    Ok(unwrap_key(
        &recovery_key.wrapping_key(RecoveryKind::Vault),
        WrapContext::VaultKeyByRecovery,
        &record.vault_key_by_recovery,
    )?)
}

/// Re-wraps the vault key under a new second password with a fresh salt. The
/// recovery wrap is kept, so the existing vault recovery key stays valid and
/// vault items are not re-encrypted.
pub fn change_vault_password(
    vault_key: &VaultKey,
    record: &VaultRecord,
    new_password: &[u8],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<VaultRecord, AccountError> {
    let salt = fresh_salt();
    let keys = derive(new_password, &salt, params, policy)?;
    let by_password = wrap_key(
        &wrapping_key_of(&keys),
        WrapContext::VaultKeyByPassword,
        vault_key,
    )?;
    Ok(VaultRecord {
        kdf_salt: salt,
        kdf_params: params,
        vault_key_by_password: by_password,
        vault_key_by_recovery: record.vault_key_by_recovery.clone(),
    })
}
