//! End-to-end behaviour of the account and vault operations.

#![cfg(feature = "client")]

use ne_crypto::account::{
    AccountError, AccountRecord, RecoveryRecord, VaultRecord, begin_login, change_password,
    change_vault_password, create_account, create_vault, recover_account, unlock_vault,
    unlock_vault_with_recovery_key, unlock_with_password, unlock_with_recovery_key,
};
use ne_crypto::auth;
use ne_crypto::envelope::{open_update, seal_update};
use ne_crypto::key_wrap::{WrapContext, unwrap_key};
use ne_crypto::keys::{RootKey, SecretKey, VaultKey, WorkspaceKey, WrappingKey};
use ne_crypto::password::{KdfError, KdfParams, KdfPolicy, derive_keys};
use ne_crypto::recovery::{RecoveryKey, RecoveryKind};

/// Small costs keep the suite fast.
const FAST: KdfParams = KdfParams {
    memory_kib: 1024,
    iterations: 1,
    parallelism: 1,
};
const POLICY: KdfPolicy = KdfPolicy::LENIENT;
const PASSWORD: &[u8] = b"correct horse battery staple";

#[test]
fn create_then_unlock_with_password() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let secrets = unlock_with_password(PASSWORD, &created.record, &POLICY).expect("unlock");
    assert_eq!(secrets.root_key, created.secrets.root_key);
    assert_eq!(secrets.workspace_key, created.secrets.workspace_key);
}

#[test]
fn two_step_login_matches_server_verifier() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let record = created.record;

    let attempt =
        begin_login(PASSWORD, &record.kdf_salt, record.kdf_params, &POLICY).expect("begin");
    let auth_key = attempt.auth_key();
    assert!(auth::verify(auth_key.as_bytes(), &record.auth_verifier));
    assert_eq!(auth_key, created.auth_key);
    assert!(attempt.unlock(&record).is_ok());

    let wrong = begin_login(
        b"another password",
        &record.kdf_salt,
        record.kdf_params,
        &POLICY,
    )
    .expect("begin");
    assert!(!auth::verify(
        wrong.auth_key().as_bytes(),
        &record.auth_verifier
    ));
    assert_eq!(wrong.unlock(&record).err(), Some(AccountError::WrongSecret));
}

#[test]
fn wrong_password_is_rejected() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    assert_eq!(
        unlock_with_password(b"wrong", &created.record, &POLICY).err(),
        Some(AccountError::WrongSecret)
    );
}

#[test]
fn empty_password_is_rejected() {
    assert_eq!(
        create_account(b"", FAST, &POLICY).err(),
        Some(AccountError::EmptyPassword)
    );
}

#[test]
fn recovery_key_unlocks_and_wrong_one_does_not() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let secrets = unlock_with_recovery_key(&created.recovery_key, &created.record).expect("unlock");
    assert_eq!(secrets.workspace_key, created.secrets.workspace_key);

    assert_eq!(
        unlock_with_recovery_key(&RecoveryKey::generate(), &created.record).err(),
        Some(AccountError::WrongSecret)
    );
}

#[test]
fn recovery_key_survives_display_round_trip_and_typo_is_caught() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let shown = created.recovery_key.to_display_string();
    let typed = RecoveryKey::parse(&shown.to_lowercase()).expect("parse");
    assert!(unlock_with_recovery_key(&typed, &created.record).is_ok());

    let first = shown.chars().next().expect("non-empty");
    let typo = format!("{}{}", if first == '2' { '3' } else { '2' }, &shown[1..]);
    assert!(RecoveryKey::parse(&typo).is_err());
}

#[test]
fn password_change_keeps_data_keys_and_recovery() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let old_record = created.record.clone();
    let changed = change_password(
        &created.secrets,
        &old_record,
        b"a brand new password",
        FAST,
        &POLICY,
    )
    .expect("change");

    assert_ne!(changed.record.kdf_salt, old_record.kdf_salt);
    assert_ne!(changed.record.auth_verifier, old_record.auth_verifier);
    assert!(auth::verify(
        changed.auth_key.as_bytes(),
        &changed.record.auth_verifier
    ));
    assert_eq!(changed.record.workspace_key, old_record.workspace_key);

    // Old password no longer works; new one opens the same workspace key.
    assert!(unlock_with_password(PASSWORD, &changed.record, &POLICY).is_err());
    let secrets =
        unlock_with_password(b"a brand new password", &changed.record, &POLICY).expect("unlock");
    assert_eq!(secrets.workspace_key, created.secrets.workspace_key);

    // Data sealed before the change still opens, and the recovery key still works.
    let envelope =
        seal_update(&created.secrets.workspace_key, "w", "d", "dev", b"x").expect("seal");
    assert_eq!(
        open_update(&secrets.workspace_key, "w", "d", "dev", &envelope).expect("open"),
        b"x"
    );
    assert!(unlock_with_recovery_key(&created.recovery_key, &changed.record).is_ok());
}

#[test]
fn password_change_refuses_a_foreign_record() {
    let mine = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let theirs = create_account(PASSWORD, FAST, &POLICY).expect("create");
    assert_eq!(
        change_password(
            &mine.secrets,
            &theirs.record,
            b"new password",
            FAST,
            &POLICY
        )
        .err(),
        Some(AccountError::WrongSecret)
    );
}

#[test]
fn recovery_then_set_new_password() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let secrets =
        unlock_with_recovery_key(&created.recovery_key, &created.record).expect("recover");
    let changed =
        change_password(&secrets, &created.record, b"fresh start", FAST, &POLICY).expect("change");
    assert!(unlock_with_password(b"fresh start", &changed.record, &POLICY).is_ok());
}

fn stored_of(record: &AccountRecord) -> RecoveryRecord {
    RecoveryRecord {
        root_key_by_recovery: record.root_key_by_recovery.clone(),
        workspace_key: record.workspace_key.clone(),
    }
}

#[test]
fn recover_account_sets_a_new_password_and_keeps_data_keys() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let sealed =
        seal_update(&created.secrets.workspace_key, "ws", "doc", "dev", b"note").expect("seal");
    let recovered = recover_account(
        &created.recovery_key,
        &stored_of(&created.record),
        b"fresh start",
        FAST,
        &POLICY,
    )
    .expect("recover");

    assert_eq!(recovered.secrets.root_key, created.secrets.root_key);
    assert_eq!(
        recovered.secrets.workspace_key,
        created.secrets.workspace_key
    );
    assert_eq!(
        open_update(
            &recovered.secrets.workspace_key,
            "ws",
            "doc",
            "dev",
            &sealed
        )
        .expect("open"),
        b"note"
    );
    // The new record logs in with the new password only, and the verifiers
    // match the keys that were returned.
    let record = &recovered.record;
    assert!(unlock_with_password(b"fresh start", record, &POLICY).is_ok());
    assert_eq!(
        unlock_with_password(PASSWORD, record, &POLICY).err(),
        Some(AccountError::WrongSecret)
    );
    assert!(auth::verify(
        recovered.auth_key.as_bytes(),
        &record.auth_verifier
    ));
    assert!(auth::verify(
        recovered.recovery_auth_key.as_bytes(),
        &created.record.recovery_verifier
    ));
    assert_eq!(record.recovery_verifier, created.record.recovery_verifier);
    assert_ne!(record.kdf_salt, created.record.kdf_salt);
    // The recovery key is not rotated: it still works afterwards.
    assert!(unlock_with_recovery_key(&created.recovery_key, record).is_ok());
}

#[test]
fn recover_account_rejects_wrong_keys_and_bad_passwords() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let stored = stored_of(&created.record);
    assert_eq!(
        recover_account(&RecoveryKey::generate(), &stored, b"x", FAST, &POLICY).err(),
        Some(AccountError::WrongSecret)
    );
    // The vault flavour of a recovery key must not unlock the account.
    let other = create_account(PASSWORD, FAST, &POLICY).expect("create");
    assert_eq!(
        recover_account(&other.recovery_key, &stored, b"x", FAST, &POLICY).err(),
        Some(AccountError::WrongSecret)
    );
    assert_eq!(
        recover_account(&created.recovery_key, &stored, b"", FAST, &POLICY).err(),
        Some(AccountError::EmptyPassword)
    );
    assert_eq!(
        recover_account(
            &created.recovery_key,
            &stored,
            b"x",
            FAST,
            &KdfPolicy::PRODUCTION
        )
        .err(),
        Some(AccountError::Kdf(KdfError::OutOfPolicy))
    );
}

#[test]
fn recovery_auth_key_reveals_nothing_about_the_wrapping_key() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let auth_key = created.recovery_key.auth_key(RecoveryKind::Account);
    // Using the auth secret as a wrapping key does not open the root key.
    let impostor = WrappingKey::from_bytes(auth_key.as_bytes());
    assert!(
        unwrap_key::<WrappingKey, RootKey>(
            &impostor,
            WrapContext::RootKeyByRecovery,
            &created.record.root_key_by_recovery
        )
        .is_err()
    );
    assert!(created.record.supports_recovery());
}

#[test]
fn records_without_a_recovery_verifier_deserialise_as_unrecoverable() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let mut value = serde_json::to_value(&created.record).expect("serialise");
    value
        .as_object_mut()
        .expect("object")
        .remove("recovery_verifier");
    let legacy: AccountRecord = serde_json::from_value(value).expect("legacy record");
    assert!(!legacy.supports_recovery());
    assert!(unlock_with_password(PASSWORD, &legacy, &POLICY).is_ok());
}

#[test]
fn password_change_keeps_the_recovery_verifier() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let changed =
        change_password(&created.secrets, &created.record, b"new", FAST, &POLICY).expect("change");
    assert_eq!(
        changed.record.recovery_verifier,
        created.record.recovery_verifier
    );
}

#[test]
fn kdf_policy_blocks_downgraded_or_oversized_records() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    // FAST parameters are below the production floor: a server returning them
    // for an existing account must not be obeyed.
    assert_eq!(
        unlock_with_password(PASSWORD, &created.record, &KdfPolicy::PRODUCTION).err(),
        Some(AccountError::Kdf(KdfError::OutOfPolicy))
    );
    assert_eq!(
        create_account(PASSWORD, FAST, &KdfPolicy::PRODUCTION).err(),
        Some(AccountError::Kdf(KdfError::OutOfPolicy))
    );
}

#[test]
fn account_record_holds_only_serialisable_non_secret_fields() {
    let created = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let json = serde_json::to_string(&created.record).expect("serialise");
    let back: AccountRecord = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, created.record);

    let value: serde_json::Value = serde_json::from_str(&json).expect("json");
    let mut fields: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "auth_verifier",
            "kdf_params",
            "kdf_salt",
            "recovery_verifier",
            "root_key_by_password",
            "root_key_by_recovery",
            "workspace_key"
        ]
    );

    // No raw secret appears in the serialised record.
    let b64 = |bytes: &[u8]| {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    };
    for secret in [
        b64(created.secrets.root_key.as_bytes()),
        b64(created.secrets.workspace_key.as_bytes()),
        b64(created.auth_key.as_bytes()),
        b64(created
            .recovery_key
            .auth_key(RecoveryKind::Account)
            .as_bytes()),
        b64(created
            .recovery_key
            .wrapping_key(RecoveryKind::Account)
            .as_bytes()),
    ] {
        assert!(!json.contains(&secret));
    }
}

#[test]
fn vault_round_trip_recovery_and_wrong_secrets() {
    let created = create_vault(b"second password", FAST, &POLICY).expect("create");
    let key = unlock_vault(b"second password", &created.record, &POLICY).expect("unlock");
    assert_eq!(key, created.vault_key);

    assert_eq!(
        unlock_vault(b"nope", &created.record, &POLICY).err(),
        Some(AccountError::WrongSecret)
    );
    let recovered =
        unlock_vault_with_recovery_key(&created.recovery_key, &created.record).expect("recover");
    assert_eq!(recovered, created.vault_key);
    assert_eq!(
        unlock_vault_with_recovery_key(&RecoveryKey::generate(), &created.record).err(),
        Some(AccountError::WrongSecret)
    );

    let json = serde_json::to_string(&created.record).expect("serialise");
    let back: VaultRecord = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, created.record);
}

#[test]
fn vault_password_change_keeps_key_and_recovery() {
    let created = create_vault(b"second password", FAST, &POLICY).expect("create");
    let record = change_vault_password(
        &created.vault_key,
        &created.record,
        b"third password",
        FAST,
        &POLICY,
    )
    .expect("change");
    assert_ne!(record.kdf_salt, created.record.kdf_salt);
    assert!(unlock_vault(b"second password", &record, &POLICY).is_err());
    assert_eq!(
        unlock_vault(b"third password", &record, &POLICY).expect("unlock"),
        created.vault_key
    );
    assert!(unlock_vault_with_recovery_key(&created.recovery_key, &record).is_ok());
}

#[test]
fn account_and_vault_recovery_keys_are_not_interchangeable() {
    let acct = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let vault = create_vault(b"second password", FAST, &POLICY).expect("create");
    assert!(unlock_vault_with_recovery_key(&acct.recovery_key, &vault.record).is_err());
    assert!(unlock_with_recovery_key(&vault.recovery_key, &acct.record).is_err());
}

/// The vault key is not reachable from anything the account password gives.
#[test]
fn account_password_alone_cannot_unwrap_the_vault_key() {
    // Worst case for the vault: the user reuses the same password for both.
    let acct = create_account(PASSWORD, FAST, &POLICY).expect("create account");
    let vault = create_vault(PASSWORD, FAST, &POLICY).expect("create vault");
    let blob = &vault.record.vault_key_by_password;

    // 1. The account's password-derived keys, under every context label.
    let account_keys = derive_keys(PASSWORD, &acct.record.kdf_salt, FAST).expect("derive");
    let account_wrapping = WrappingKey::from_bytes(account_keys.encryption_key());
    // 2. The account root and workspace keys the password unlocks.
    let secrets = unlock_with_password(PASSWORD, &acct.record, &POLICY).expect("unlock");

    let contexts = [
        WrapContext::VaultKeyByPassword,
        WrapContext::VaultKeyByRecovery,
        WrapContext::RootKeyByPassword,
        WrapContext::RootKeyByRecovery,
        WrapContext::WorkspaceKey,
    ];
    for context in contexts {
        assert!(unwrap_key::<_, VaultKey>(&account_wrapping, context, blob).is_err());
        assert!(unwrap_key::<_, VaultKey>(&secrets.root_key, context, blob).is_err());
        assert!(unwrap_key::<_, VaultKey>(&secrets.workspace_key, context, blob).is_err());
        assert!(
            unwrap_key::<_, VaultKey>(
                &account_wrapping,
                context,
                &vault.record.vault_key_by_recovery
            )
            .is_err()
        );
    }
    // 3. Everything in the account record, including its wrapped blobs, is
    //    unrelated to the vault: the account recovery key fails too.
    assert!(unlock_vault_with_recovery_key(&acct.recovery_key, &vault.record).is_err());
    // 4. The account unlock path never yields the vault key.
    assert_ne!(secrets.root_key.as_bytes(), vault.vault_key.as_bytes());
    assert_ne!(secrets.workspace_key.as_bytes(), vault.vault_key.as_bytes());
    // Sanity: the vault does open with its own password.
    assert!(unlock_vault(PASSWORD, &vault.record, &POLICY).is_ok());
}

/// Wrapped account keys cannot be passed off as the wrong role.
#[test]
fn wrapped_blobs_are_bound_to_their_role() {
    let acct = create_account(PASSWORD, FAST, &POLICY).expect("create");
    let keys = derive_keys(PASSWORD, &acct.record.kdf_salt, FAST).expect("derive");
    let wrapping = WrappingKey::from_bytes(keys.encryption_key());
    let blob = &acct.record.root_key_by_password;
    assert!(unwrap_key::<_, RootKey>(&wrapping, WrapContext::RootKeyByPassword, blob).is_ok());
    assert!(unwrap_key::<_, VaultKey>(&wrapping, WrapContext::VaultKeyByPassword, blob).is_err());
    assert!(unwrap_key::<_, WorkspaceKey>(&wrapping, WrapContext::WorkspaceKey, blob).is_err());
}
