//! Account, login, throttling, registration and session behaviour against a
//! local server. Skipped without `NE_SYNC_TEST_URL`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use ne_crypto::account::{create_vault, unlock_vault};
use ne_crypto::keys::SecretKey;
use ne_crypto::password::KdfPolicy;
use ne_sync::{MemoryStore, SyncError, SyncFailure, SyncStatus};

use common::{PASSWORD, USERNAME, fresh, raw_http, setup_code, start_engine, wait_for};

#[tokio::test]
async fn full_account_lifecycle() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    assert!(!signup.recovery_key.to_display_string().is_empty());
    let original_key = *signup.unlocked.secrets.workspace_key.as_bytes();

    // A second device signs in with only the username and password.
    let (second, unlocked) = h.login("second device").await;
    assert_eq!(unlocked.secrets.workspace_key.as_bytes(), &original_key);
    assert_eq!(
        unlocked.session.workspace_id,
        signup.unlocked.session.workspace_id
    );

    // Wrong password and unknown wrapped data are refused.
    let stranger = h.client("stranger");
    assert!(matches!(
        stranger.login(USERNAME, "not the password").await,
        Err(SyncError::InvalidCredentials)
    ));
    assert!(matches!(
        stranger.list_sessions().await,
        Err(SyncError::Unauthorized)
    ));

    // Sessions are listed with their labels and the caller is marked.
    let sessions = first.list_sessions().await.expect("sessions");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions.iter().filter(|s| s.current).count(), 1);
    assert!(sessions.iter().any(|s| s.label == "second device"));

    // The vault record and the wrapped workspace key round-trip.
    assert!(first.vault_record().await.expect("vault").is_none());
    let vault =
        create_vault(b"second password", common::TEST_KDF, &KdfPolicy::LENIENT).expect("vault");
    first
        .put_vault_record(&vault.record)
        .await
        .expect("put vault");
    let stored = second
        .vault_record()
        .await
        .expect("vault")
        .expect("present");
    assert_eq!(stored, vault.record);
    let vault_key = unlock_vault(b"second password", &stored, &KdfPolicy::LENIENT).expect("unlock");
    assert_eq!(vault_key.as_bytes(), vault.vault_key.as_bytes());
    let record = first.account_record().await.expect("record");
    assert_eq!(
        first.workspace_key().await.expect("wrapped key"),
        record.workspace_key
    );
    first
        .put_workspace_key(&record.workspace_key)
        .await
        .expect("put key");

    // Changing the password ends every other session and keeps the data key.
    second
        .change_password(PASSWORD, "a brand new passphrase")
        .await
        .expect("change");
    assert!(matches!(
        first.list_sessions().await,
        Err(SyncError::Unauthorized)
    ));
    assert_eq!(
        second.list_sessions().await.expect("still signed in").len(),
        1
    );
    assert!(matches!(
        h.client("late").login(USERNAME, PASSWORD).await,
        Err(SyncError::InvalidCredentials)
    ));
    let relogin = h
        .client("after change")
        .login(USERNAME, "a brand new passphrase")
        .await
        .expect("login");
    assert_eq!(relogin.secrets.workspace_key.as_bytes(), &original_key);
    assert!(matches!(
        second
            .change_password("wrong current", "whatever else")
            .await,
        Err(SyncError::InvalidCredentials)
    ));

    // Logout ends the session for good.
    second.logout().await.expect("logout");
    assert!(matches!(
        second.list_sessions().await,
        Err(SyncError::Unauthorized)
    ));
}

#[tokio::test]
async fn wrong_passwords_lock_the_account_with_growing_delays() {
    let h = fresh(require_server!()).await;
    h.sign_up().await;
    let client = h.client("attacker");
    for _ in 0..5 {
        assert!(matches!(
            client.login(USERNAME, "guess").await,
            Err(SyncError::InvalidCredentials)
        ));
    }
    // Locked: even the right password is refused until the delay passes.
    match client.login(USERNAME, PASSWORD).await {
        Err(SyncError::Locked(secs)) => assert!((1..=2).contains(&secs), "first lock {secs}s"),
        other => panic!("expected a lockout, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(2300)).await;
    // One more failure doubles the lock.
    assert!(matches!(
        client.login(USERNAME, "guess").await,
        Err(SyncError::InvalidCredentials)
    ));
    match client.login(USERNAME, PASSWORD).await {
        Err(SyncError::Locked(secs)) => assert!((3..=4).contains(&secs), "second lock {secs}s"),
        other => panic!("expected a longer lockout, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(4300)).await;
    client
        .login(USERNAME, PASSWORD)
        .await
        .expect("login after the lock expires");
    // A success clears the record: failures start again from zero.
    assert!(matches!(
        client.login(USERNAME, "guess").await,
        Err(SyncError::InvalidCredentials)
    ));
}

#[tokio::test]
async fn unknown_usernames_look_like_wrong_passwords() {
    let h = fresh(require_server!()).await;
    h.sign_up().await;
    let salt_url = h.url.join("/v1/account/salt").expect("url");
    let salt_of = |name: &'static str| {
        let url = salt_url.clone();
        async move {
            let response = raw_http()
                .post(url)
                .json(&serde_json::json!({ "username": name }))
                .send()
                .await
                .expect("salt");
            assert_eq!(response.status(), 200);
            response.json::<serde_json::Value>().await.expect("json")
        }
    };
    let real = salt_of(USERNAME).await;
    let ghost = salt_of("ghost@example.test").await;
    assert_eq!(
        ghost,
        salt_of("ghost@example.test").await,
        "fake salt is stable"
    );
    assert_ne!(
        ghost["kdf_salt"],
        salt_of("other@example.test").await["kdf_salt"]
    );
    assert_ne!(ghost["kdf_salt"], real["kdf_salt"]);
    assert_eq!(
        ghost["kdf_params"], real["kdf_params"],
        "same parameters as the real account"
    );
    assert_eq!(
        ghost["kdf_salt"].as_str().map(str::len),
        real["kdf_salt"].as_str().map(str::len)
    );

    let client = h.client("probe");
    let known = client
        .login(USERNAME, "wrong")
        .await
        .expect_err("wrong password");
    let unknown = client
        .login("ghost@example.test", "wrong")
        .await
        .expect_err("unknown user");
    assert_eq!(known.to_string(), unknown.to_string());
    assert!(matches!(unknown, SyncError::InvalidCredentials));
}

#[tokio::test]
async fn unknown_usernames_are_throttled_like_real_ones() {
    let h = fresh(require_server!()).await;
    h.sign_up().await;
    let client = h.client("probe");
    for _ in 0..5 {
        assert!(matches!(
            client.login("ghost@example.test", "guess").await,
            Err(SyncError::InvalidCredentials)
        ));
    }
    assert!(matches!(
        client.login("ghost@example.test", "guess").await,
        Err(SyncError::Locked(_))
    ));
}

#[tokio::test]
async fn registration_needs_the_setup_code_and_closes_after_one_account() {
    let h = fresh(require_server!()).await;
    let client = h.client("first device");
    assert!(matches!(
        client
            .sign_up("not the setup code at all", USERNAME, PASSWORD)
            .await,
        Err(SyncError::Forbidden)
    ));
    assert!(client.session().is_none());
    client
        .sign_up(&setup_code(), USERNAME, PASSWORD)
        .await
        .expect("first account");

    let other = h.client("intruder");
    assert!(matches!(
        other
            .sign_up(
                &setup_code(),
                "second@example.test",
                "another long password"
            )
            .await,
        Err(SyncError::RegistrationClosed)
    ));
    assert!(matches!(
        other
            .sign_up("wrong code", "third@example.test", "another long password")
            .await,
        Err(SyncError::RegistrationClosed)
    ));
    // The one account is untouched.
    h.client("check")
        .login(USERNAME, PASSWORD)
        .await
        .expect("login");
}

#[tokio::test]
async fn revoking_a_session_cuts_its_sync_connection() {
    let h = fresh(require_server!()).await;
    let (first, signup) = h.sign_up().await;
    let (second, unlocked) = h.login("second device").await;
    let store = Arc::new(MemoryStore::new());
    let engine = start_engine(&second, &unlocked.secrets, "device-two", &store, None);
    common::wait_synced(&engine).await;

    first
        .revoke_session(&unlocked.session.session_id)
        .await
        .expect("revoke");
    wait_for(
        "revoked engine to stop",
        Duration::from_secs(10),
        || async { engine.status() == SyncStatus::Error(SyncFailure::Unauthorized) },
    )
    .await;
    assert!(matches!(
        second.list_sessions().await,
        Err(SyncError::Unauthorized)
    ));
    // Revoking something that is gone, or that is not an id, is a plain not-found.
    assert!(matches!(
        first.revoke_session(&unlocked.session.session_id).await,
        Err(SyncError::NotFound)
    ));
    assert!(matches!(
        first.revoke_session("not-an-id").await,
        Err(SyncError::NotFound)
    ));
    drop(signup);
}

#[tokio::test]
async fn requests_are_checked_at_the_edge() {
    let h = fresh(require_server!()).await;
    let url = |path: &str| h.url.join(path).expect("url");
    let http = raw_http();
    let plain = reqwest::Client::builder()
        .user_agent("python-requests/2.0")
        .build()
        .expect("client");
    assert_eq!(
        plain
            .get(url("/v1/health"))
            .send()
            .await
            .expect("send")
            .status(),
        403
    );

    assert_eq!(
        http.get(url("/v1/health"))
            .send()
            .await
            .expect("send")
            .status(),
        200
    );
    let browser = http
        .get(url("/v1/health"))
        .header("Origin", "https://evil.example")
        .send()
        .await;
    assert_eq!(browser.expect("send").status(), 403);

    let unsized_body = reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(&b"{}"[..]));
    let probe = http
        .post(url("/v1/session/login"))
        .body(unsized_body)
        .send()
        .await
        .expect("send");
    assert_eq!(probe.status(), 411, "a body without a length is refused");
    let junk = http
        .post(url("/v1/session/login"))
        .header("Content-Type", "application/json")
        .body("{not json")
        .send()
        .await
        .expect("send");
    assert_eq!(junk.status(), 400);
    assert!(junk.headers().get("access-control-allow-origin").is_none());
    assert_eq!(
        junk.json::<serde_json::Value>().await.expect("json"),
        serde_json::json!({"error": "bad_request"})
    );

    let huge = vec![b'x'; 70 * 1024];
    let big = http
        .post(url("/v1/session/login"))
        .body(huge)
        .send()
        .await
        .expect("send");
    assert_eq!(big.status(), 413);

    assert_eq!(
        http.get(url("/v1/nope"))
            .send()
            .await
            .expect("send")
            .status(),
        404
    );
    assert_eq!(
        http.get(url("/elsewhere"))
            .send()
            .await
            .expect("send")
            .status(),
        404
    );
    assert_eq!(
        http.get(url("/v1/sessions"))
            .send()
            .await
            .expect("send")
            .status(),
        401
    );
    let options = http
        .request(reqwest::Method::OPTIONS, url("/v1/sessions"))
        .send()
        .await
        .expect("send");
    assert!(
        options
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}
