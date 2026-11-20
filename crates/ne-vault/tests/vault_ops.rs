//! Lock state machine and item operations.

mod support;

use std::time::Duration;

use ne_crypto::account::AccountError;
use ne_vault::{
    Field, ItemId, LockEvent, LockPolicy, LockState, Resolution, SecretRef, SensitiveAction, Vault,
    VaultDoc, VaultError,
};
use support::*;

const T0: i64 = 1_700_000_000_000;

fn locked_vault(policy: LockPolicy) -> (Vault, std::sync::Arc<TestClock>, Account) {
    let account = account();
    let clock = TestClock::at(T0);
    let vault = vault_with(&account, VaultDoc::new(), policy, clock.clone());
    (vault, clock, account)
}

#[test]
fn starts_locked_and_refuses_everything() {
    let (mut vault, _, _) = locked_vault(LockPolicy::default());
    let id = ItemId::generate();
    assert_eq!(vault.state(), LockState::Locked);
    assert_eq!(vault.list().err(), Some(VaultError::Locked));
    assert_eq!(vault.search("x").err(), Some(VaultError::Locked));
    assert_eq!(vault.get(&id).err(), Some(VaultError::Locked));
    assert_eq!(
        vault.add(login("a", "u", "p")).err(),
        Some(VaultError::Locked)
    );
    assert_eq!(
        vault.update(&id, login("a", "u", "p")).err(),
        Some(VaultError::Locked)
    );
    assert_eq!(vault.delete(&id).err(), Some(VaultError::Locked));
    assert_eq!(
        vault.reveal(&id, Field::Password).err(),
        Some(VaultError::Locked)
    );
    assert_eq!(
        vault.copy(&id, Field::Password).err(),
        Some(VaultError::Locked)
    );
    assert_eq!(vault.export_csv().err(), Some(VaultError::Locked));
    assert_eq!(
        vault.reauthenticate_with_password(PASSWORD).err(),
        Some(VaultError::Locked)
    );
    assert!(matches!(
        vault.resolve(&SecretRef::new(id, Field::Password)),
        Resolution::Redacted
    ));
}

#[test]
fn wrong_password_does_not_unlock_and_recovery_key_does() {
    let (mut vault, _, account) = locked_vault(LockPolicy::default());
    assert_eq!(
        vault.unlock_with_password(b"not the password").err(),
        Some(VaultError::Account(AccountError::WrongSecret))
    );
    assert!(!vault.is_unlocked());
    assert_eq!(
        vault.unlock_with_password(b"").err(),
        Some(VaultError::Account(AccountError::EmptyPassword))
    );
    vault
        .unlock_with_recovery_key(&account.recovery)
        .expect("recovery unlock");
    assert!(vault.is_unlocked());
}

#[test]
fn add_get_update_delete_round_trip() {
    let (mut vault, clock, _) = locked_vault(LockPolicy::default());
    vault.unlock_with_password(PASSWORD).expect("unlock");
    let id = vault
        .add(login("Mail", "ana@example.test", "pw-1"))
        .expect("add");

    let view = vault.get(&id).expect("get");
    assert_eq!(
        (view.title.as_str(), view.username.as_deref()),
        ("Mail", Some("ana@example.test"))
    );
    assert_eq!((view.created_ms, view.updated_ms), (T0, T0));

    clock.advance_secs(10);
    vault
        .update(&id, login("Mail 2", "ana@example.test", "pw-2"))
        .expect("update");
    let view = vault.get(&id).expect("get");
    assert_eq!(view.title, "Mail 2");
    assert_eq!((view.created_ms, view.updated_ms), (T0, T0 + 10_000));
    assert_eq!(
        &*vault.reveal(&id, Field::Password).expect("reveal"),
        "pw-2"
    );

    vault.delete(&id).expect("delete");
    assert_eq!(vault.get(&id).err(), Some(VaultError::NotFound(id)));
    assert_eq!(vault.delete(&id).err(), Some(VaultError::NotFound(id)));
    assert_eq!(
        vault.update(&id, login("x", "", "")).err(),
        Some(VaultError::NotFound(id))
    );
    assert!(vault.list().expect("list").is_empty());
}

#[test]
fn all_item_kinds_store_and_reveal_their_fields() {
    let mut vault = unlocked_replica(&account(), TestClock::at(T0));
    let login_id = vault.add(login("L", "user", "pw")).expect("add");
    let note_id = vault.add(note("N", "line1\nline2")).expect("add");
    let key_id = vault.add(api_key("K", "tok_123")).expect("add");

    assert_eq!(
        &*vault.reveal(&login_id, Field::Username).expect("r"),
        "user"
    );
    assert_eq!(
        &*vault.reveal(&note_id, Field::Text).expect("r"),
        "line1\nline2"
    );
    assert_eq!(&*vault.reveal(&key_id, Field::Value).expect("r"), "tok_123");
    assert_eq!(
        vault.reveal(&note_id, Field::Password).err(),
        Some(VaultError::NoSuchField)
    );
    assert_eq!(
        vault
            .reveal(&ItemId::generate(), Field::Text)
            .err()
            .map(|e| matches!(e, VaultError::NotFound(_))),
        Some(true)
    );
    assert!(
        vault.add(login("   ", "u", "p")).is_err(),
        "empty title rejected"
    );
    assert_eq!(vault.get(&key_id).expect("get").tags, ["ci"]);
}

#[test]
fn list_is_sorted_and_search_ignores_case_and_diacritics() {
    let mut vault = unlocked_replica(&account(), TestClock::at(T0));
    let mut bank = login("Ngân hàng", "Đặng", "pw-bank");
    bank.tags = vec!["Tài chính".into()];
    if let ne_vault::ItemBody::Login { urls, .. } = &mut bank.body {
        urls.push("https://bank.example.test".into());
    }
    vault.add(bank).expect("add");
    vault
        .add(login("alpha", "second@example.test", "pw-alpha"))
        .expect("add");
    vault
        .add(note("Ghi chú", "very secret words"))
        .expect("add");

    assert_eq!(titles(&vault), ["alpha", "Ghi chú", "Ngân hàng"]);
    let by = |q: &str| -> Vec<String> {
        vault
            .search(q)
            .expect("search")
            .into_iter()
            .map(|v| v.title.clone())
            .collect()
    };
    assert_eq!(by("ngan hang"), ["Ngân hàng"]);
    assert_eq!(by("NGÂN"), ["Ngân hàng"]);
    assert_eq!(by("dang"), ["Ngân hàng"], "username, đ folded");
    assert_eq!(by("tai chinh"), ["Ngân hàng"], "tag");
    assert_eq!(by("bank.example"), ["Ngân hàng"], "url");
    assert_eq!(by("ghi chu"), ["Ghi chú"]);
    assert_eq!(by("SECOND@EXAMPLE"), ["alpha"]);
    assert_eq!(by("hang ngan"), ["Ngân hàng"], "word order is free");
    assert!(by("nothing like this").is_empty());
    assert_eq!(by("  ").len(), 3, "blank query lists all");
    for secret in ["pw-bank", "pw-alpha", "secret", "words"] {
        assert!(by(secret).is_empty(), "{secret} must not be searchable");
    }
}

#[test]
fn lock_removes_access_and_drops_the_cache() {
    let mut vault = unlocked_replica(&account(), TestClock::at(T0));
    let id = vault.add(login("Mail", "u", "pw")).expect("add");
    assert_eq!(vault.cached_item_count(), 1);

    vault.lock();
    assert_eq!(vault.state(), LockState::Locked);
    assert_eq!(vault.cached_item_count(), 0);
    assert_eq!(vault.get(&id).err(), Some(VaultError::Locked));
    assert_eq!(vault.list().err(), Some(VaultError::Locked));
    assert_eq!(
        vault.reveal(&id, Field::Password).err(),
        Some(VaultError::Locked)
    );
    assert!(matches!(
        vault.resolve(&SecretRef::new(id, Field::Password)),
        Resolution::Redacted
    ));
    vault.lock();

    vault.unlock_with_password(PASSWORD).expect("unlock again");
    assert_eq!(vault.cached_item_count(), 1);
    assert_eq!(&*vault.reveal(&id, Field::Password).expect("reveal"), "pw");
}

#[test]
fn events_follow_the_policy() {
    let policy = LockPolicy {
        idle_timeout: Some(Duration::from_secs(120)),
        lock_on_background: false,
        ..LockPolicy::default()
    };
    let (mut vault, _, _) = locked_vault(policy);
    let cases = [
        (LockEvent::AppBackgrounded, false),
        (LockEvent::IdleElapsed(Duration::from_secs(119)), false),
        (LockEvent::IdleElapsed(Duration::from_secs(120)), true),
        (LockEvent::SystemSlept, true),
        (LockEvent::ScreenLocked, true),
    ];
    for (event, locks) in cases {
        vault.unlock_with_password(PASSWORD).expect("unlock");
        assert_eq!(vault.handle_event(event), locks, "{event:?}");
        assert_eq!(vault.is_unlocked(), !locks, "{event:?}");
    }
    vault.set_policy(LockPolicy {
        lock_on_background: true,
        ..policy
    });
    vault.unlock_with_password(PASSWORD).expect("unlock");
    assert!(vault.handle_event(LockEvent::AppBackgrounded));
    assert!(
        vault.handle_event(LockEvent::SystemSlept),
        "already locked stays locked"
    );
}

#[test]
fn sensitive_actions_need_fresh_authentication() {
    let policy = LockPolicy {
        reauth_grace: Duration::from_secs(60),
        clipboard_clear_after: Duration::from_secs(30),
        ..LockPolicy::default()
    };
    let (mut vault, clock, _) = locked_vault(policy);
    vault.unlock_with_password(PASSWORD).expect("unlock");
    let id = vault.add(login("Mail", "u", "pw")).expect("add");
    let reference = SecretRef::new(id, Field::Password);

    clock.advance_secs(59);
    assert!(vault.reveal(&id, Field::Password).is_ok());
    assert!(matches!(vault.resolve(&reference), Resolution::Value(_)));

    clock.advance_secs(1);
    assert_eq!(
        vault.reveal(&id, Field::Password).err(),
        Some(VaultError::ReauthRequired(SensitiveAction::Reveal))
    );
    assert_eq!(
        vault
            .copy(&id, Field::Password)
            .err()
            .map(|e| e.to_string()),
        Some(VaultError::ReauthRequired(SensitiveAction::Copy).to_string())
    );
    assert_eq!(
        vault.export_csv().err(),
        Some(VaultError::ReauthRequired(SensitiveAction::Export))
    );
    assert!(matches!(vault.resolve(&reference), Resolution::Redacted));
    assert!(
        vault.get(&id).is_ok(),
        "non-secret views need no re-authentication"
    );

    assert!(vault.reauthenticate_with_password(b"wrong").is_err());
    assert!(
        vault.is_unlocked(),
        "a failed re-authentication does not lock"
    );
    assert!(vault.reveal(&id, Field::Password).is_err());
    vault
        .reauthenticate_with_password(PASSWORD)
        .expect("reauth");
    assert!(vault.reveal(&id, Field::Password).is_ok());
}

#[test]
fn copy_returns_value_and_clear_deadline() {
    let (mut vault, clock, _) = locked_vault(LockPolicy {
        clipboard_clear_after: Duration::from_secs(30),
        ..LockPolicy::default()
    });
    vault.unlock_with_password(PASSWORD).expect("unlock");
    let id = vault.add(api_key("K", "tok_abc")).expect("add");
    clock.advance_secs(5);
    let payload = vault.copy(&id, Field::Value).expect("copy");
    assert_eq!(&*payload.value, "tok_abc");
    assert_eq!(payload.clear_at_ms, T0 + 5_000 + 30_000);
    assert!(!format!("{payload:?}").contains("tok_abc"));
}

#[test]
fn zero_grace_asks_every_time() {
    let (mut vault, _, _) = locked_vault(LockPolicy {
        reauth_grace: Duration::ZERO,
        ..LockPolicy::default()
    });
    vault.unlock_with_password(PASSWORD).expect("unlock");
    let id = vault.add(login("Mail", "u", "pw")).expect("add");
    assert!(vault.reveal(&id, Field::Password).is_err());
}

#[test]
fn clock_moving_backwards_forces_reauthentication() {
    let (mut vault, clock, _) = locked_vault(LockPolicy::default());
    vault.unlock_with_password(PASSWORD).expect("unlock");
    let id = vault.add(login("Mail", "u", "pw")).expect("add");
    clock.set(T0 - 5_000);
    assert!(vault.reveal(&id, Field::Password).is_err());
}

#[test]
fn secret_references_resolve_only_while_open() {
    let mut vault = unlocked_replica(&account(), TestClock::at(T0));
    let id = vault.add(login("Mail", "u", "pw-ref")).expect("add");
    let reference: SecretRef = format!("secret://{id}/password").parse().expect("parse");
    match vault.resolve(&reference) {
        Resolution::Value(value) => assert_eq!(&*value, "pw-ref"),
        other => panic!("expected a value, got {other:?}"),
    }
    let missing = SecretRef::new(ItemId::generate(), Field::Password);
    assert!(matches!(vault.resolve(&missing), Resolution::Missing));
    let wrong_field = SecretRef::new(id, Field::Text);
    assert!(matches!(vault.resolve(&wrong_field), Resolution::Missing));

    vault.lock();
    assert!(matches!(vault.resolve(&reference), Resolution::Redacted));
    assert!(matches!(vault.resolve(&missing), Resolution::Redacted));
}

#[test]
fn password_change_keeps_items_and_recovery_key() {
    let account = account();
    let mut vault = unlocked_replica(&account, TestClock::at(T0));
    let id = vault.add(login("Mail", "u", "pw")).expect("add");
    let new_record = vault
        .change_password(b"another synthetic password", TINY_KDF)
        .expect("change");
    assert_eq!(&new_record, vault.record());
    assert_ne!(new_record, account.record);

    vault.lock();
    assert!(
        vault.unlock_with_password(PASSWORD).is_err(),
        "old password rejected"
    );
    vault
        .unlock_with_password(b"another synthetic password")
        .expect("new password");
    assert_eq!(&*vault.reveal(&id, Field::Password).expect("reveal"), "pw");
    vault.lock();
    vault
        .unlock_with_recovery_key(&account.recovery)
        .expect("recovery still valid");
}

#[test]
fn unreadable_records_are_reported_not_fatal() {
    let account = account();
    let mut honest = unlocked_replica(&account, TestClock::at(T0));
    honest.add(login("Fine", "u", "pw")).expect("add");

    // A replica sealing under a different key stands in for a hostile server
    // injecting records the real key cannot open.
    let other = ne_crypto::account::create_vault(
        b"someone else",
        TINY_KDF,
        &ne_crypto::password::KdfPolicy::LENIENT,
    )
    .expect("other vault");
    let mut foreign = vault_with(
        &support::Account {
            record: other.record.clone(),
            recovery: other.recovery_key,
        },
        VaultDoc::new(),
        LockPolicy::default(),
        TestClock::at(T0),
    );
    foreign
        .unlock_with_password(b"someone else")
        .expect("unlock foreign");
    foreign.add(login("Injected", "x", "y")).expect("add");

    sync(&foreign, &mut honest);
    assert_eq!(titles(&honest), ["Fine"]);
    assert_eq!(honest.unreadable_items().len(), 1);
}

#[test]
fn tombstoned_ids_are_visible_but_not_listed() {
    let mut vault = unlocked_replica(&account(), TestClock::at(T0));
    let id = vault.add(login("Mail", "u", "pw")).expect("add");
    vault.delete(&id).expect("delete");
    let snapshot = vault.export_snapshot().expect("snapshot");
    let doc = VaultDoc::from_snapshot(&snapshot).expect("load");
    assert!(doc.item_ids().is_empty());
    assert_eq!(doc.deleted_ids(), [id]);
}
