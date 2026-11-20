//! Recovering a forgotten password with the recovery key, against a local
//! server. Skipped without `NE_SYNC_TEST_URL`.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ne_crypto::account::AccountError;
use ne_crypto::keys::SecretKey;
use ne_crypto::recovery::{RecoveryKey, RecoveryKind};
use ne_sync::{MemoryStore, SyncClient, SyncError, SyncFailure, SyncStatus};
use ne_sync_protocol::wire::{
    AccountRecordWire, RecoverRequest, RecoveryLookupRequest, RecoveryLookupResponse,
    b64_decode_array, b64_encode,
};

use common::{
    Harness, PASSWORD, USERNAME, fresh, raw_http, start_engine, state_of, update_of, wait_for,
    wait_synced,
};

const DOC: &str = "doc-main";
const NEW_PASSWORD: &str = "a brand new password";

/// A recovery request as the client would send it, built by hand so tests can
/// present wrong proofs and unknown names, which the client refuses locally.
async fn raw_recover(
    h: &Harness,
    client: &SyncClient,
    username: &str,
    proof: &[u8; 32],
) -> reqwest::Response {
    let record: AccountRecordWire =
        serde_json::to_value(client.account_record().await.expect("record"))
            .and_then(serde_json::from_value)
            .expect("wire record");
    let body = RecoverRequest {
        username: username.into(),
        recovery_auth_key: b64_encode(proof),
        new_auth_key: b64_encode(&[7; 32]),
        device_label: "raw".into(),
        record,
    };
    raw_http()
        .post(h.url.join("/v1/account/recover").expect("url"))
        .json(&body)
        .send()
        .await
        .expect("request")
}

async fn error_code(response: reqwest::Response) -> (u16, String, Option<u64>) {
    let status = response.status().as_u16();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    (
        status,
        body["error"].as_str().unwrap_or("").to_owned(),
        body["retry_after"].as_u64(),
    )
}

#[tokio::test]
async fn forgotten_password_is_recovered_on_a_second_device_with_data_intact() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    let recovery = signup.recovery_key.to_display_string().to_string();
    let workspace_key = *signup.unlocked.secrets.workspace_key.as_bytes();

    // Device A has synced some notes.
    let store_a = Arc::new(MemoryStore::new());
    let a = start_engine(&first, &signup.unlocked.secrets, "device-a", &store_a, None);
    for n in 1..=5 {
        a.submit(DOC, &update_of(n)).expect("submit");
    }
    wait_synced(&a).await;

    // A new device that knows only the username and the recovery key.
    let device = h.client("recovered device");
    let unlocked = device
        .recover_account(USERNAME, &recovery, NEW_PASSWORD)
        .await
        .expect("recover");
    assert_eq!(unlocked.secrets.workspace_key.as_bytes(), &workspace_key);
    assert_eq!(
        unlocked.session.workspace_id,
        signup.unlocked.session.workspace_id
    );
    assert_ne!(
        unlocked.session.session_id,
        signup.unlocked.session.session_id
    );

    // Every earlier session ended: the old device is cut off.
    assert!(matches!(
        first.list_sessions().await,
        Err(SyncError::Unauthorized)
    ));
    wait_for("device A to stop", Duration::from_secs(10), || async {
        a.status() == SyncStatus::Error(SyncFailure::Unauthorized)
    })
    .await;
    let sessions = device.list_sessions().await.expect("sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].label, "recovered device");

    // The old password is rejected, the new one works, on another device too.
    let other = h.client("other");
    assert!(matches!(
        other.login(USERNAME, PASSWORD).await,
        Err(SyncError::InvalidCredentials)
    ));
    let relogin = other
        .login(USERNAME, NEW_PASSWORD)
        .await
        .expect("new password");
    assert_eq!(relogin.secrets.workspace_key.as_bytes(), &workspace_key);

    // The recovered device syncs the data that existed before.
    let store_b = Arc::new(MemoryStore::new());
    let b = start_engine(&device, &unlocked.secrets, "device-b", &store_b, None);
    let want: BTreeSet<u32> = (1..=5).collect();
    wait_for("recovered data", Duration::from_secs(20), || async {
        state_of(&store_b, DOC, &BTreeSet::new()) == want
    })
    .await;
    wait_synced(&b).await;
    b.submit(DOC, &update_of(6)).expect("submit");
    wait_synced(&b).await;

    // The recovery key was not rotated: it works again, and it changed nothing
    // about the keys.
    let again = h.client("again");
    let second = again
        .recover_account(USERNAME, &recovery, "yet another password")
        .await
        .expect("recover twice");
    assert_eq!(second.secrets.root_key, unlocked.secrets.root_key);
    assert!(
        h.client("c")
            .login(USERNAME, "yet another password")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn recovery_works_while_password_logins_are_locked_out() {
    let h = fresh(require_server!()).await;
    let (_first, signup) = h.sign_up().await;
    let recovery = signup.recovery_key.to_display_string().to_string();

    // Someone who forgot the password fails until the account is locked.
    let guesser = h.client("guesser");
    for _ in 0..5 {
        assert!(matches!(
            guesser.login(USERNAME, "forgotten").await,
            Err(SyncError::InvalidCredentials)
        ));
    }
    assert!(matches!(
        guesser.login(USERNAME, PASSWORD).await,
        Err(SyncError::Locked(_))
    ));

    // Recovery has buckets of its own, so it is not blocked by that...
    let device = h.client("recovered");
    device
        .recover_account(USERNAME, &recovery, NEW_PASSWORD)
        .await
        .expect("recover while locked out");
    // ...and the login lock is not cleared by it.
    assert!(matches!(
        guesser.login(USERNAME, NEW_PASSWORD).await,
        Err(SyncError::Locked(_))
    ));
    tokio::time::sleep(Duration::from_millis(2300)).await;
    guesser
        .login(USERNAME, NEW_PASSWORD)
        .await
        .expect("login after the lock ends");
}

#[tokio::test]
async fn wrong_recovery_proofs_lock_recovery_with_growing_delays() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    let right = *signup
        .recovery_key
        .auth_key(RecoveryKind::Account)
        .as_bytes();
    let wrong = *RecoveryKey::generate()
        .auth_key(RecoveryKind::Account)
        .as_bytes();

    for _ in 0..5 {
        let (status, code, _) = error_code(raw_recover(&h, &first, USERNAME, &wrong).await).await;
        assert_eq!((status, code.as_str()), (401, "invalid_credentials"));
    }
    // Locked: even the right proof is refused until the delay passes.
    let (status, code, retry) = error_code(raw_recover(&h, &first, USERNAME, &right).await).await;
    assert_eq!((status, code.as_str()), (429, "locked"));
    assert!((1..=2).contains(&retry.expect("retry_after")), "{retry:?}");

    // Password login is a separate bucket and still works.
    h.client("login")
        .login(USERNAME, PASSWORD)
        .await
        .expect("login");

    tokio::time::sleep(Duration::from_millis(2300)).await;
    // One more failure doubles the lock.
    let (status, ..) = error_code(raw_recover(&h, &first, USERNAME, &wrong).await).await;
    assert_eq!(status, 401);
    let (status, _, retry) = error_code(raw_recover(&h, &first, USERNAME, &right).await).await;
    assert_eq!(status, 429);
    assert!((3..=4).contains(&retry.expect("retry_after")), "{retry:?}");

    tokio::time::sleep(Duration::from_millis(4300)).await;
    let response = raw_recover(&h, &first, USERNAME, &right).await;
    assert_eq!(response.status(), 200, "right proof after the lock expires");
}

#[tokio::test]
async fn unknown_names_cannot_be_told_from_wrong_recovery_keys() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    let recovery = signup.recovery_key.to_display_string().to_string();

    // The lookup has the same shape for a real and an unknown name, and is
    // stable for the same name.
    let lookup = |name: &str| {
        let url = h.url.join("/v1/account/recovery").expect("url");
        let name = name.to_owned();
        async move {
            raw_http()
                .post(url)
                .json(&RecoveryLookupRequest { username: name })
                .send()
                .await
                .expect("request")
                .json::<RecoveryLookupResponse>()
                .await
                .expect("lookup")
        }
    };
    let real = lookup(USERNAME).await;
    let ghost = lookup("nobody@example.test").await;
    assert_eq!(
        ghost.root_key_by_recovery,
        lookup("nobody@example.test").await.root_key_by_recovery
    );
    assert_ne!(ghost.root_key_by_recovery, real.root_key_by_recovery);
    for blob in [
        &real.root_key_by_recovery,
        &real.workspace_key,
        &ghost.root_key_by_recovery,
        &ghost.workspace_key,
    ] {
        let bytes: [u8; 73] = b64_decode_array(blob).expect("73 bytes");
        assert_eq!(bytes[0], 1, "same version byte as a real sealed key");
    }

    // The client fails the same way for a wrong key and an unknown name.
    let stranger = h.client("stranger");
    let wrong_key = RecoveryKey::generate().to_display_string().to_string();
    assert!(matches!(
        stranger
            .recover_account(USERNAME, &wrong_key, NEW_PASSWORD)
            .await,
        Err(SyncError::Account(AccountError::WrongSecret))
    ));
    assert!(matches!(
        stranger
            .recover_account("nobody@example.test", &recovery, NEW_PASSWORD)
            .await,
        Err(SyncError::Account(AccountError::WrongSecret))
    ));

    // On the wire, an unknown name is a 401 like a wrong proof and is
    // throttled per name like one.
    let proof = *signup
        .recovery_key
        .auth_key(RecoveryKind::Account)
        .as_bytes();
    for _ in 0..5 {
        let (status, code, _) =
            error_code(raw_recover(&h, &first, "nobody@example.test", &proof).await).await;
        assert_eq!((status, code.as_str()), (401, "invalid_credentials"));
    }
    let (status, code, _) =
        error_code(raw_recover(&h, &first, "nobody@example.test", &proof).await).await;
    assert_eq!((status, code.as_str()), (429, "locked"));
}

#[tokio::test]
async fn an_account_without_a_recovery_verifier_is_refused_cleanly() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    let recovery = signup.recovery_key.to_display_string().to_string();
    let response = raw_http()
        .post(h.url.join("/v1/_test/strip-recovery").expect("url"))
        .send()
        .await
        .expect("strip");
    assert_eq!(response.status(), 204);

    let device = h.client("device");
    match device
        .recover_account(USERNAME, &recovery, NEW_PASSWORD)
        .await
    {
        Err(err) => assert!(err.is_recovery_unavailable(), "{err:?}"),
        Ok(_) => panic!("recovery must be refused"),
    }
    // Nothing changed: sessions and the password still work, and the record
    // parses as one that cannot be recovered.
    assert!(first.list_sessions().await.is_ok());
    h.client("login")
        .login(USERNAME, PASSWORD)
        .await
        .expect("login");
    let record = first.account_record().await.expect("record");
    assert!(!record.supports_recovery());
    // A request that is not even well formed gets a plain 400.
    let response = raw_http()
        .post(h.url.join("/v1/account/recover").expect("url"))
        .body("{}")
        .header("content-type", "application/json")
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn malformed_recovery_input_is_rejected_before_anything_changes() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    let recovery = signup.recovery_key.to_display_string().to_string();
    let device = h.client("device");

    assert!(matches!(
        device
            .recover_account(USERNAME, "not a key", NEW_PASSWORD)
            .await,
        Err(SyncError::Input(_))
    ));
    assert!(matches!(
        device.recover_account(USERNAME, &recovery, "").await,
        Err(SyncError::Account(AccountError::EmptyPassword))
    ));
    assert!(matches!(
        device.recover_account("x", &recovery, NEW_PASSWORD).await,
        Err(SyncError::Input(_))
    ));
    // A bad shape on the wire is a 400, and the original account is untouched.
    let record: AccountRecordWire =
        serde_json::to_value(first.account_record().await.expect("record"))
            .and_then(serde_json::from_value)
            .expect("wire record");
    let body = RecoverRequest {
        username: USERNAME.into(),
        recovery_auth_key: "short".into(),
        new_auth_key: b64_encode(&[1; 32]),
        device_label: "raw".into(),
        record,
    };
    let response = raw_http()
        .post(h.url.join("/v1/account/recover").expect("url"))
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 400);
    assert!(first.list_sessions().await.is_ok());
    h.client("login")
        .login(USERNAME, PASSWORD)
        .await
        .expect("login");
}
