//! Account creation, login, recovery and password change.

use crate::auth;
use crate::key_wrap::{WrapContext, unwrap_key, wrap_key};
use crate::keys::{AuthKey, RootKey, SecretKey, WorkspaceKey};
use crate::password::{DerivedKeys, KdfParams, KdfPolicy};
use crate::recovery::{RecoveryKey, RecoveryKind};

use super::records::{AccountRecord, AccountSecrets, RecoveryRecord, SALT_LEN};
use super::{AccountError, derive, fresh_salt, wrapping_key_of};

/// Result of [`create_account`].
#[derive(Debug)]
pub struct NewAccount {
    /// Keep in memory while the app is unlocked.
    pub secrets: AccountSecrets,
    /// Upload to the server.
    pub record: AccountRecord,
    /// Send to the server when registering; it stores only its verifier.
    pub auth_key: AuthKey,
    /// Show to the user once and require confirmation that it is saved.
    pub recovery_key: RecoveryKey,
}

/// Result of [`change_password`].
#[derive(Debug)]
pub struct PasswordChange {
    /// Replaces the stored record.
    pub record: AccountRecord,
    /// The new auth key, sent to the server together with the new record.
    pub auth_key: AuthKey,
}

/// A password that has been stretched but not yet checked against a record.
///
/// Logging in is two steps because Argon2 is slow and the server must see the
/// auth key before it hands out the wrapped keys: derive once, send
/// [`auth_key`](Self::auth_key), then [`unlock`](Self::unlock) with the record
/// the server returns.
pub struct LoginAttempt {
    keys: DerivedKeys,
}

impl std::fmt::Debug for LoginAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LoginAttempt(<redacted>)")
    }
}

/// Stretches `password` with the salt and parameters the server stored.
pub fn begin_login(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<LoginAttempt, AccountError> {
    Ok(LoginAttempt {
        keys: derive(password, salt, params, policy)?,
    })
}

impl LoginAttempt {
    /// The key that proves knowledge of the password to the server.
    pub fn auth_key(&self) -> AuthKey {
        AuthKey::from_bytes(self.keys.auth_key())
    }

    /// Opens the record's root key and workspace key. The record must come
    /// from the same salt and parameters that were used to begin the login.
    pub fn unlock(self, record: &AccountRecord) -> Result<AccountSecrets, AccountError> {
        let wrapping = wrapping_key_of(&self.keys);
        let root_key: RootKey = unwrap_key(
            &wrapping,
            WrapContext::RootKeyByPassword,
            &record.root_key_by_password,
        )?;
        open_workspace(root_key, record)
    }
}

fn open_workspace(
    root_key: RootKey,
    record: &AccountRecord,
) -> Result<AccountSecrets, AccountError> {
    let workspace_key: WorkspaceKey =
        unwrap_key(&root_key, WrapContext::WorkspaceKey, &record.workspace_key)?;
    Ok(AccountSecrets {
        root_key,
        workspace_key,
    })
}

/// Creates a new account: fresh root and workspace keys, a recovery key, and
/// the record to upload.
pub fn create_account(
    password: &[u8],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<NewAccount, AccountError> {
    let salt = fresh_salt();
    let keys = derive(password, &salt, params, policy)?;
    let root_key = RootKey::generate();
    let workspace_key = WorkspaceKey::generate();
    let recovery_key = RecoveryKey::generate();

    let record = AccountRecord {
        kdf_salt: salt,
        kdf_params: params,
        auth_verifier: auth::auth_verifier(keys.auth_key()),
        recovery_verifier: auth::auth_verifier(
            recovery_key.auth_key(RecoveryKind::Account).as_bytes(),
        ),
        root_key_by_password: wrap_key(
            &wrapping_key_of(&keys),
            WrapContext::RootKeyByPassword,
            &root_key,
        )?,
        root_key_by_recovery: wrap_key(
            &recovery_key.wrapping_key(RecoveryKind::Account),
            WrapContext::RootKeyByRecovery,
            &root_key,
        )?,
        workspace_key: wrap_key(&root_key, WrapContext::WorkspaceKey, &workspace_key)?,
    };
    Ok(NewAccount {
        auth_key: AuthKey::from_bytes(keys.auth_key()),
        secrets: AccountSecrets {
            root_key,
            workspace_key,
        },
        record,
        recovery_key,
    })
}

/// Logs in with the password in one call (derives, then unlocks). Use
/// [`begin_login`] when the auth key must be sent first.
pub fn unlock_with_password(
    password: &[u8],
    record: &AccountRecord,
    policy: &KdfPolicy,
) -> Result<AccountSecrets, AccountError> {
    begin_login(password, &record.kdf_salt, record.kdf_params, policy)?.unlock(record)
}

/// Unlocks locally with the account recovery key. To reset the password on
/// the server use [`recover_account`].
pub fn unlock_with_recovery_key(
    recovery_key: &RecoveryKey,
    record: &AccountRecord,
) -> Result<AccountSecrets, AccountError> {
    let wrapping = recovery_key.wrapping_key(RecoveryKind::Account);
    let root_key: RootKey = unwrap_key(
        &wrapping,
        WrapContext::RootKeyByRecovery,
        &record.root_key_by_recovery,
    )?;
    open_workspace(root_key, record)
}

/// Result of [`recover_account`].
#[derive(Debug)]
pub struct AccountRecovery {
    /// The unlocked keys, to keep in memory.
    pub secrets: AccountSecrets,
    /// Replaces the stored password material. Its recovery fields equal the
    /// ones the server already holds.
    pub record: AccountRecord,
    /// Auth key of the new password, sent so the server can store its verifier.
    pub auth_key: AuthKey,
    /// Proves possession of the recovery key to the server.
    pub recovery_auth_key: AuthKey,
}

/// Recovers a forgotten password. Unlocks the root and workspace keys with the
/// recovery key (a wrong key fails here, before anything is sent) and wraps
/// the root key under `new_password` with a fresh salt. Data is not
/// re-encrypted, and the recovery key stays valid: it is not rotated.
///
/// `stored` holds the blobs the server returned for the account name. For a
/// name that does not exist the server returns random blobs, which fail like a
/// wrong key.
pub fn recover_account(
    recovery_key: &RecoveryKey,
    stored: &RecoveryRecord,
    new_password: &[u8],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<AccountRecovery, AccountError> {
    let root_key: RootKey = unwrap_key(
        &recovery_key.wrapping_key(RecoveryKind::Account),
        WrapContext::RootKeyByRecovery,
        &stored.root_key_by_recovery,
    )?;
    let workspace_key: WorkspaceKey =
        unwrap_key(&root_key, WrapContext::WorkspaceKey, &stored.workspace_key)?;
    let salt = fresh_salt();
    let keys = derive(new_password, &salt, params, policy)?;
    let recovery_auth_key = recovery_key.auth_key(RecoveryKind::Account);
    let record = AccountRecord {
        kdf_salt: salt,
        kdf_params: params,
        auth_verifier: auth::auth_verifier(keys.auth_key()),
        recovery_verifier: auth::auth_verifier(recovery_auth_key.as_bytes()),
        root_key_by_password: wrap_key(
            &wrapping_key_of(&keys),
            WrapContext::RootKeyByPassword,
            &root_key,
        )?,
        root_key_by_recovery: stored.root_key_by_recovery.clone(),
        workspace_key: stored.workspace_key.clone(),
    };
    Ok(AccountRecovery {
        auth_key: AuthKey::from_bytes(keys.auth_key()),
        secrets: AccountSecrets {
            root_key,
            workspace_key,
        },
        record,
        recovery_auth_key,
    })
}

/// Re-wraps the root key under a new password with a fresh salt. Workspace data
/// is not re-encrypted. The caller must already hold unlocked `secrets` and
/// the server must authorise the change (for example with the old auth key).
pub fn change_password(
    secrets: &AccountSecrets,
    record: &AccountRecord,
    new_password: &[u8],
    params: KdfParams,
    policy: &KdfPolicy,
) -> Result<PasswordChange, AccountError> {
    // Refuse to rewrite a record these secrets do not belong to.
    let _: WorkspaceKey = unwrap_key(
        &secrets.root_key,
        WrapContext::WorkspaceKey,
        &record.workspace_key,
    )?;

    let salt = fresh_salt();
    let keys = derive(new_password, &salt, params, policy)?;
    let new_record = AccountRecord {
        kdf_salt: salt,
        kdf_params: params,
        auth_verifier: auth::auth_verifier(keys.auth_key()),
        recovery_verifier: record.recovery_verifier,
        root_key_by_password: wrap_key(
            &wrapping_key_of(&keys),
            WrapContext::RootKeyByPassword,
            &secrets.root_key,
        )?,
        root_key_by_recovery: record.root_key_by_recovery.clone(),
        workspace_key: record.workspace_key.clone(),
    };
    Ok(PasswordChange {
        auth_key: AuthKey::from_bytes(keys.auth_key()),
        record: new_record,
    })
}
