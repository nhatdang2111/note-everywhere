//! The vault: lifecycle, re-authentication, clipboard, auto-lock, import and
//! persistence, on a local-only workspace.

mod support;

use std::time::Duration;

use ne_platform::activity::{ActivityError, ActivityMonitor, IdleSource, MonitorConfig};
use ne_platform::secure_store::SecretBackend;
use ne_vault::LockEvent;
use note_everywhere_desktop_lib::app::CoreEvent;
use note_everywhere_desktop_lib::app::types::{GeneratePasswordArgs, VaultItemInput};
use support::Device;

const VAULT_PASSWORD: &str = "second secret passphrase";

fn login(title: &str, password: &str) -> VaultItemInput {
    VaultItemInput {
        id: None,
        kind: "login".into(),
        title: title.into(),
        username: Some("alice".into()),
        password: Some(password.into()),
        value: None,
        urls: Some(vec!["https://example.test".into()]),
        notes: Some("private note".into()),
        tags: Some(vec!["work".into()]),
    }
}

async fn unlocked() -> Device {
    let device = Device::local().await;
    device
        .core
        .vault_create(VAULT_PASSWORD.into())
        .await
        .expect("create vault");
    device
}

fn last_state(device: &Device) -> String {
    match device.events.named("vault:state").last() {
        Some(CoreEvent::VaultState { state }) => (*state).to_owned(),
        other => panic!("no vault state event: {other:?}"),
    }
}

#[tokio::test]
async fn create_unlock_and_lock() {
    let device = Device::local().await;
    assert_eq!(
        device.core.vault_state().await.expect("state"),
        "uninitialised"
    );
    assert_eq!(
        device
            .core
            .vault_list(None)
            .await
            .expect_err("no vault")
            .code,
        "vault_uninitialised"
    );
    let (recovery, same) = device
        .core
        .vault_create(VAULT_PASSWORD.into())
        .await
        .expect("create");
    assert!(!same, "a local workspace has no account password to reuse");
    assert_eq!(recovery.replace('-', "").len(), 44);
    assert_eq!(device.core.vault_state().await.expect("state"), "unlocked");
    assert_eq!(last_state(&device), "unlocked");
    assert_eq!(
        device
            .core
            .vault_create("another".into())
            .await
            .expect_err("twice")
            .code,
        "vault_exists"
    );

    device.core.vault_lock().await.expect("lock");
    assert_eq!(last_state(&device), "locked");
    assert_eq!(
        device.core.vault_list(None).await.expect_err("locked").code,
        "vault_locked"
    );
    assert_eq!(
        device
            .core
            .vault_unlock("wrong".into())
            .await
            .expect_err("wrong")
            .code,
        "wrong_password"
    );
    assert_eq!(device.core.vault_state().await.expect("state"), "locked");
    device
        .core
        .vault_unlock(VAULT_PASSWORD.into())
        .await
        .expect("unlock");
    assert_eq!(last_state(&device), "unlocked");
}

#[tokio::test]
async fn items_keep_omitted_secrets_on_update() {
    let device = unlocked().await;
    let core = &device.core;
    let saved = core
        .vault_save(login("Mail", "hunter2"))
        .await
        .expect("save");
    assert_eq!(
        (saved.kind, saved.username.as_deref(), saved.urls.len()),
        ("login", Some("alice"), 1)
    );

    // Renaming without resending the secrets keeps them.
    let mut edit = login("Mail (work)", "");
    edit.id = Some(saved.id.clone());
    edit.password = None;
    edit.notes = None;
    edit.username = None;
    let renamed = core.vault_save(edit).await.expect("update");
    assert_eq!(renamed.title, "Mail (work)");
    assert_eq!(renamed.username.as_deref(), Some("alice"));
    assert_eq!(
        core.vault_reveal(&saved.id, "password", None)
            .await
            .expect("reveal"),
        "hunter2"
    );
    assert_eq!(
        core.vault_reveal(&saved.id, "notes", None)
            .await
            .expect("notes"),
        "private note"
    );

    // An empty string is an explicit clear.
    let mut clear = login("Mail (work)", "");
    clear.id = Some(saved.id.clone());
    core.vault_save(clear).await.expect("clear");
    assert_eq!(
        core.vault_reveal(&saved.id, "password", None)
            .await
            .expect("reveal"),
        ""
    );

    let note = core
        .vault_save(VaultItemInput {
            kind: "secure_note".into(),
            title: "Wifi".into(),
            notes: Some("the code is 1234".into()),
            ..login("", "")
        })
        .await
        .expect("note");
    assert_eq!(note.kind, "secure_note");
    assert_eq!(
        core.vault_reveal(&note.id, "notes", None)
            .await
            .expect("text"),
        "the code is 1234"
    );
    let key = core
        .vault_save(VaultItemInput {
            kind: "api_key".into(),
            title: "CI".into(),
            value: Some("tok_123".into()),
            notes: None,
            username: None,
            password: None,
            urls: None,
            ..login("", "")
        })
        .await
        .expect("key");
    assert_eq!(
        core.vault_reveal(&key.id, "value", None)
            .await
            .expect("value"),
        "tok_123"
    );
    assert_eq!(
        core.vault_reveal(&key.id, "password", None)
            .await
            .expect_err("no field")
            .code,
        "no_such_field"
    );

    assert_eq!(
        core.vault_list(Some("wifi")).await.expect("search").len(),
        1
    );
    assert_eq!(core.vault_list(None).await.expect("list").len(), 3);
    core.vault_delete(&key.id).await.expect("delete");
    assert_eq!(
        core.vault_delete(&key.id).await.expect_err("gone").code,
        "not_found"
    );
    assert_eq!(
        core.vault_save(VaultItemInput {
            kind: "card".into(),
            ..login("x", "y")
        })
        .await
        .expect_err("kind")
        .code,
        "invalid_input"
    );
}

#[tokio::test]
async fn secrets_need_the_password_again_after_the_grace_period() {
    let device = unlocked().await;
    let core = &device.core;
    let item = core
        .vault_save(login("Bank", "s3cret"))
        .await
        .expect("save");
    assert_eq!(
        core.vault_reveal(&item.id, "password", None)
            .await
            .expect("fresh"),
        "s3cret"
    );

    device.clock.advance(Duration::from_secs(61));
    assert_eq!(
        core.vault_reveal(&item.id, "password", None)
            .await
            .expect_err("stale")
            .code,
        "reauth_required"
    );
    assert_eq!(
        core.vault_copy(&item.id, "password", None)
            .await
            .expect_err("stale")
            .code,
        "reauth_required"
    );
    assert_eq!(
        core.vault_reveal(&item.id, "password", Some("wrong".into()))
            .await
            .expect_err("wrong")
            .code,
        "wrong_password"
    );
    // Supplying the password restarts the grace period.
    assert_eq!(
        core.vault_reveal(&item.id, "password", Some(VAULT_PASSWORD.into()))
            .await
            .expect("again"),
        "s3cret"
    );
    assert_eq!(
        core.vault_reveal(&item.id, "password", None)
            .await
            .expect("fresh again"),
        "s3cret"
    );
    // Ordinary operations never ask.
    device.clock.advance(Duration::from_secs(600));
    assert_eq!(core.vault_list(None).await.expect("list").len(), 1);
}

#[tokio::test]
async fn copying_clears_the_clipboard_only_if_it_still_holds_the_secret() {
    let device = unlocked().await;
    let core = &device.core;
    let item = core
        .vault_save(login("Mail", "hunter2"))
        .await
        .expect("save");
    let seconds = core
        .vault_copy(&item.id, "password", None)
        .await
        .expect("copy");
    assert_eq!(
        seconds, 0,
        "the test deadline is 300 ms, below a whole second"
    );
    assert_eq!(
        device.clipboard.0.lock().expect("lock").as_deref(),
        Some("hunter2")
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        device.clipboard.0.lock().expect("lock").as_deref(),
        Some("")
    );

    // Something else was copied in the meantime: leave it alone.
    core.vault_copy(&item.id, "username", None)
        .await
        .expect("copy username");
    *device.clipboard.0.lock().expect("lock") = Some("something the person copied".into());
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        device.clipboard.0.lock().expect("lock").as_deref(),
        Some("something the person copied")
    );

    // A newer copy replaces the older deadline.
    core.vault_copy(&item.id, "password", None)
        .await
        .expect("copy");
    tokio::time::sleep(Duration::from_millis(150)).await;
    core.vault_copy(&item.id, "password", None)
        .await
        .expect("copy again");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        device.clipboard.0.lock().expect("lock").as_deref(),
        Some("hunter2"),
        "the first deadline was cancelled"
    );
}

struct Idle(Duration);

impl IdleSource for Idle {
    fn idle_time(&self) -> Result<Duration, ActivityError> {
        Ok(self.0)
    }
}

#[tokio::test]
async fn activity_events_lock_the_vault() {
    let device = unlocked().await;
    let core = &device.core;
    assert!(!core.vault_lock_event(LockEvent::IdleElapsed(Duration::from_secs(299))));
    assert!(!core.vault_lock_event(LockEvent::AppBackgrounded));
    assert_eq!(core.vault_state().await.expect("state"), "unlocked");
    assert!(core.vault_lock_event(LockEvent::IdleElapsed(Duration::from_secs(300))));
    assert_eq!(last_state(&device), "locked");

    for event in [LockEvent::SystemSlept, LockEvent::ScreenLocked] {
        core.vault_unlock(VAULT_PASSWORD.into())
            .await
            .expect("unlock");
        assert!(core.vault_lock_event(event), "{event:?}");
        assert_eq!(core.vault_state().await.expect("state"), "locked");
    }
}

#[tokio::test]
async fn the_activity_monitor_and_the_idle_setting_drive_the_lock() {
    let device = unlocked().await;
    let core = &device.core;
    core.settings_set(note_everywhere_desktop_lib::app::types::SettingsPatch {
        vault_idle_lock_seconds: Some(60),
        ..Default::default()
    })
    .await
    .expect("set idle time");
    let config = MonitorConfig {
        idle_report_after: Some(Duration::from_secs(60)),
        ..MonitorConfig::default()
    };
    let mut quiet = ActivityMonitor::with_source(
        Idle(Duration::from_secs(30)),
        config,
        std::time::Instant::now(),
        std::time::SystemTime::now(),
    );
    core.poll_activity(&mut quiet);
    assert_eq!(core.vault_state().await.expect("state"), "unlocked");
    let mut away = ActivityMonitor::with_source(
        Idle(Duration::from_secs(61)),
        config,
        std::time::Instant::now(),
        std::time::SystemTime::now(),
    );
    core.poll_activity(&mut away);
    assert_eq!(core.vault_state().await.expect("state"), "locked");

    // Turning the idle lock off means idle time alone never locks.
    core.vault_unlock(VAULT_PASSWORD.into())
        .await
        .expect("unlock");
    core.settings_set(note_everywhere_desktop_lib::app::types::SettingsPatch {
        vault_idle_lock_seconds: Some(0),
        ..Default::default()
    })
    .await
    .expect("disable");
    let mut long_away = ActivityMonitor::with_source(
        Idle(Duration::from_secs(86_400)),
        config,
        std::time::Instant::now(),
        std::time::SystemTime::now(),
    );
    core.poll_activity(&mut long_away);
    assert_eq!(core.vault_state().await.expect("state"), "unlocked");
}

#[tokio::test]
async fn recovery_key_and_password_changes() {
    let device = Device::local().await;
    let core = &device.core;
    let (recovery, _) = core
        .vault_create(VAULT_PASSWORD.into())
        .await
        .expect("create");
    let item = core
        .vault_save(login("Mail", "hunter2"))
        .await
        .expect("save");

    core.vault_change_password("wrong".into(), "new password".into())
        .await
        .expect_err("wrong current");
    core.vault_change_password(VAULT_PASSWORD.into(), "new password".into())
        .await
        .expect("change");
    core.vault_lock().await.expect("lock");
    assert_eq!(
        core.vault_unlock(VAULT_PASSWORD.into())
            .await
            .expect_err("old")
            .code,
        "wrong_password"
    );
    core.vault_unlock("new password".into())
        .await
        .expect("new password works");

    core.vault_lock().await.expect("lock");
    assert_eq!(
        core.vault_recover("not a key", "x".into())
            .await
            .expect_err("typo")
            .code,
        "invalid_recovery_key"
    );
    let other = Device::local().await;
    let (foreign, _) = other
        .core
        .vault_create("pw".into())
        .await
        .expect("other vault");
    assert_eq!(
        core.vault_recover(&foreign, "x".into())
            .await
            .expect_err("foreign")
            .code,
        "wrong_recovery_key"
    );
    core.vault_recover(&recovery, "brand new".into())
        .await
        .expect("recover");
    assert_eq!(core.vault_state().await.expect("state"), "unlocked");
    assert_eq!(
        core.vault_reveal(&item.id, "password", None)
            .await
            .expect("kept"),
        "hunter2"
    );
    core.vault_lock().await.expect("lock");
    core.vault_unlock("brand new".into())
        .await
        .expect("recovered password");
}

#[tokio::test]
async fn the_vault_survives_a_restart_and_the_key_is_never_stored() {
    let device = unlocked().await;
    let item = device
        .core
        .vault_save(login("Mail", "hunter2"))
        .await
        .expect("save");
    let device = device.restart().await;
    assert_eq!(device.core.vault_state().await.expect("state"), "locked");
    assert_eq!(
        device.core.vault_list(None).await.expect_err("locked").code,
        "vault_locked"
    );
    device
        .core
        .vault_unlock(VAULT_PASSWORD.into())
        .await
        .expect("unlock");
    let items = device.core.vault_list(None).await.expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, item.id);
    assert_eq!(
        device
            .core
            .vault_reveal(&item.id, "password", None)
            .await
            .expect("reveal"),
        "hunter2"
    );
    // A local workspace has nothing in the keychain, and the vault adds nothing.
    assert!(
        device
            .secrets
            .get("dev.noteeverywhere.app", "default/root-key")
            .expect("get")
            .is_none()
    );
}

#[tokio::test]
async fn the_vault_never_reaches_search() {
    let device = unlocked().await;
    device
        .core
        .workspace_create_local()
        .await
        .expect_err("exists");
    device
        .core
        .vault_save(login("Zebra bank", "pw"))
        .await
        .expect("save");
    assert!(
        device
            .core
            .search_query("zebra", None)
            .await
            .expect("search")
            .is_empty()
    );
}

#[tokio::test]
async fn the_generator_reports_entropy() {
    let device = Device::local().await;
    let args = |mode: &str, length| GeneratePasswordArgs {
        mode: mode.into(),
        length,
        lowercase: None,
        uppercase: None,
        digits: Some(false),
        symbols: Some(false),
        exclude_ambiguous: Some(true),
    };
    let password = device
        .core
        .vault_generate_password(args("characters", 24))
        .await
        .expect("password");
    assert_eq!(password.value.chars().count(), 24);
    assert!(password.value.chars().all(|c| c.is_ascii_alphabetic()));
    assert!(password.entropy_bits > 100.0);
    let phrase = device
        .core
        .vault_generate_password(args("passphrase", 5))
        .await
        .expect("phrase");
    assert_eq!(phrase.value.split('-').count(), 5);
    assert_eq!(
        device
            .core
            .vault_generate_password(args("characters", 1))
            .await
            .expect_err("short")
            .code,
        "invalid_input"
    );
    assert_eq!(
        device
            .core
            .vault_generate_password(args("morse", 8))
            .await
            .expect_err("mode")
            .code,
        "invalid_input"
    );
}

#[tokio::test]
async fn csv_import_reports_skipped_rows() {
    let device = unlocked().await;
    let file = device.dir.path().join("logins.csv");
    std::fs::write(
        &file,
        "type,title,username,password,url,notes,tags\n\
         login,Mail,alice,pw1,https://mail.example.test,,\n\
         login,,,,,,\n\
         apikey,CI token,,tok_9,,,ci\n",
    )
    .expect("write");
    assert!(
        device
            .core
            .vault_import_csv("generic")
            .await
            .expect("cancelled")
            .is_none()
    );
    device.dialogs.queue_file(Some(file.clone()));
    let report = device
        .core
        .vault_import_csv("generic")
        .await
        .expect("import")
        .expect("report");
    assert_eq!(report.imported, 2);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].row, 2);
    assert_eq!(device.core.vault_list(None).await.expect("list").len(), 2);
    assert_eq!(
        device
            .core
            .vault_import_csv("lotus")
            .await
            .expect_err("format")
            .code,
        "invalid_input"
    );

    device.core.vault_lock().await.expect("lock");
    device.dialogs.queue_file(Some(file));
    assert_eq!(
        device
            .core
            .vault_import_csv("generic")
            .await
            .expect_err("locked")
            .code,
        "vault_locked"
    );
}
