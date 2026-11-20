//! Session `last_seen_at` tracking against a local server.
//!
//! Needs a server started with `SESSION_TOUCH_SECONDS` set to a small value
//! (2 is right) and `NE_SYNC_TEST_TOUCH_SECONDS` set to the same number for
//! this process; skipped otherwise, like every test here without
//! `NE_SYNC_TEST_URL`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use ne_sync::{MemoryStore, SyncClient};

use common::{fresh, start_engine, update_of, wait_for, wait_synced};

const DOC: &str = "doc-main";

fn touch_seconds() -> Option<u64> {
    std::env::var("NE_SYNC_TEST_TOUCH_SECONDS")
        .ok()?
        .parse()
        .ok()
        .filter(|secs| (1..=10).contains(secs))
}

/// `(created_at, last_seen_at)` of the caller's own session.
async fn own_times(client: &SyncClient) -> (u64, u64) {
    let sessions = client.list_sessions().await.expect("sessions");
    let current = sessions.iter().find(|s| s.current).expect("current");
    (current.created_at, current.last_seen_at)
}

/// Waits until the server acknowledged everything queued. (A status check
/// alone could still see the state from before the update was noticed.)
async fn sent(store: &MemoryStore) {
    wait_for(
        "the push to be acknowledged",
        Duration::from_secs(20),
        || async { store.outbox().is_empty() },
    )
    .await;
}

async fn rows_written(h: &common::Harness) -> u64 {
    h.stats().await["rows_written"].as_u64().expect("rows")
}

#[tokio::test]
async fn last_seen_follows_connects_and_writes_at_a_coarse_interval() {
    let url = require_server!();
    let Some(interval) = touch_seconds() else {
        eprintln!("skipped: set NE_SYNC_TEST_TOUCH_SECONDS to the server's SESSION_TOUCH_SECONDS");
        return;
    };
    let wait = Duration::from_millis(interval * 1000 + 500);
    let h = fresh(url).await;
    let (first, signup) = h.sign_up().await;

    // A new session starts out last seen at its creation.
    let (created, seen) = own_times(&first).await;
    assert_eq!(seen, created);

    // Connecting counts as activity once the interval has passed.
    tokio::time::sleep(wait).await;
    let store = Arc::new(MemoryStore::new());
    let engine = start_engine(&first, &signup.unlocked.secrets, "device-a", &store, None);
    wait_synced(&engine).await;
    let (_, after_connect) = own_times(&first).await;
    assert!(
        after_connect >= created + interval * 1000,
        "connect should advance last_seen_at ({created} -> {after_connect})"
    );

    // A push after the interval writes the new time; a push right after it,
    // within the interval, costs no extra row write and changes nothing.
    tokio::time::sleep(wait).await;
    let before = rows_written(&h).await;
    engine.submit(DOC, &update_of(1)).expect("submit");
    sent(&store).await;
    let (_, after_push) = own_times(&first).await;
    let first_push_rows = rows_written(&h).await - before;
    assert!(
        after_push >= after_connect + interval * 1000,
        "push advances last_seen_at"
    );

    let before = rows_written(&h).await;
    engine.submit(DOC, &update_of(2)).expect("submit");
    sent(&store).await;
    let second_push_rows = rows_written(&h).await - before;
    let (_, after_second) = own_times(&first).await;
    assert_eq!(after_second, after_push, "no write inside the interval");
    assert_eq!(
        first_push_rows,
        second_push_rows + 1,
        "the touch is exactly one row write per interval, not one per request"
    );

    // Reading the session list or logging in elsewhere does not move it.
    let (second, _) = h.login("second device").await;
    let (_, still) = own_times(&first).await;
    assert_eq!(still, after_push);
    let (second_created, second_seen) = own_times(&second).await;
    assert_eq!(second_seen, second_created);
    engine.shutdown().await;
}
