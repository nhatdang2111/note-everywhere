//! Devices converging through the real server: offline edits, forced
//! disconnects mid-push, restarts, and paging. Skipped without `NE_SYNC_TEST_URL`.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ne_sync::{MemoryStore, SyncStatus};

use common::{
    Proxy, applied_count, fresh, server_addr, start_engine, state_of, update_of, wait_for,
    wait_synced,
};

const DOC: &str = "doc-main";

fn range(from: u32, to: u32) -> BTreeSet<u32> {
    (from..=to).collect()
}

#[tokio::test]
async fn two_devices_converge_after_offline_edits() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let addr = server_addr(&h.url).await;
    let (proxy_a, proxy_b) = (Proxy::start(addr).await, Proxy::start(addr).await);
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));

    // Both start with no network at all.
    proxy_a.block();
    proxy_b.block();
    let a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        Some(&proxy_a.url()),
    );
    let b = start_engine(
        &client_b,
        &unlocked_b.secrets,
        "device-b",
        &store_b,
        Some(&proxy_b.url()),
    );
    wait_for("both offline", Duration::from_secs(10), || async {
        a.status() == SyncStatus::Offline && b.status() == SyncStatus::Offline
    })
    .await;

    for n in 1..=20 {
        a.submit(DOC, &update_of(n)).expect("submit");
        a.submit("doc-other", &update_of(1000 + n)).expect("submit");
    }
    for n in 100..=120 {
        b.submit(DOC, &update_of(n)).expect("submit");
    }
    assert_eq!(
        store_a.outbox().len(),
        40,
        "edits wait in the outbox while offline"
    );
    assert_eq!(a.status(), SyncStatus::Offline);

    proxy_a.unblock();
    proxy_b.unblock();
    let want: BTreeSet<u32> = range(1, 20).union(&range(100, 120)).copied().collect();
    let (own_a, own_b) = (range(1, 20), range(100, 120));
    wait_for("devices to converge", Duration::from_secs(30), || async {
        state_of(&store_a, DOC, &own_a) == want && state_of(&store_b, DOC, &own_b) == want
    })
    .await;
    wait_synced(&a).await;
    wait_synced(&b).await;
    assert!(store_a.outbox().is_empty() && store_b.outbox().is_empty());

    // Documents are independent: B received A's other document too, once each.
    let other: BTreeSet<u32> = store_b
        .applied()
        .iter()
        .filter(|u| u.doc_id == "doc-other")
        .map(|u| u32::from_le_bytes(u.plaintext[..4].try_into().expect("four bytes")))
        .collect();
    assert_eq!(other, range(1001, 1020));
    assert_eq!(
        store_b
            .applied()
            .iter()
            .filter(|u| u.doc_id == "doc-other")
            .count(),
        20
    );
    // A device never re-applies what it wrote itself.
    assert!(store_a.applied().iter().all(|u| u.device_id == "device-b"));
}

#[tokio::test]
async fn forced_disconnects_mid_push_lose_nothing_and_duplicate_nothing() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let proxy_a = Proxy::start(server_addr(&h.url).await).await;
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));
    let mut a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        Some(&proxy_a.url()),
    );
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_synced(&a).await;
    wait_synced(&b).await;

    // The push reaches the server, but its acknowledgement is lost and the link dies.
    proxy_a.lose_next_ack();
    for n in 1..=5 {
        a.submit(DOC, &update_of(n)).expect("submit");
    }
    wait_for("the cut to fire", Duration::from_secs(10), || async {
        !proxy_a.cut_pending()
    })
    .await;
    wait_for(
        "retry to be acknowledged",
        Duration::from_secs(20),
        || async { store_a.outbox().is_empty() },
    )
    .await;
    assert!(proxy_a.connections() >= 2, "the engine reconnected");

    // Connections keep dying while more edits stream in.
    for n in 6..=40 {
        a.submit(DOC, &update_of(n)).expect("submit");
        if n % 7 == 0 {
            proxy_a.kill_connections();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A hard stop (the process dying) with edits still queued, then a restart on the same store.
    proxy_a.block();
    for n in 41..=50 {
        a.submit(DOC, &update_of(n)).expect("submit");
    }
    drop(a);
    proxy_a.unblock();
    a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        Some(&proxy_a.url()),
    );

    let want = range(1, 50);
    wait_for(
        "B to receive everything",
        Duration::from_secs(30),
        || async { state_of(&store_b, DOC, &BTreeSet::new()) == want },
    )
    .await;
    wait_synced(&a).await;
    assert!(store_a.outbox().is_empty());
    // No duplicates: B applied each of A's updates exactly once.
    assert_eq!(store_b.applied().len(), 50);

    // The server log itself holds each update once: a brand-new device sees 50 rows.
    let (client_c, unlocked_c) = h.login("device c").await;
    let store_c = Arc::new(MemoryStore::new());
    let c = start_engine(&client_c, &unlocked_c.secrets, "device-c", &store_c, None);
    wait_for("C to catch up", Duration::from_secs(20), || async {
        state_of(&store_c, DOC, &BTreeSet::new()) == want
    })
    .await;
    wait_synced(&c).await;
    assert_eq!(applied_count(&store_c, false), 50);
    drop(b);
}

#[tokio::test]
async fn a_new_device_pages_through_a_long_log() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let store_a = Arc::new(MemoryStore::new());
    let a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        None,
    );
    for n in 1..=450 {
        a.submit(DOC, &update_of(n)).expect("submit");
    }
    wait_for("A to push 450 updates", Duration::from_secs(60), || async {
        store_a.outbox().is_empty()
    })
    .await;
    wait_synced(&a).await;

    let (client_b, unlocked_b) = h.login("device b").await;
    let store_b = Arc::new(MemoryStore::new());
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_synced(&b).await;
    assert_eq!(
        store_b.applied().len(),
        450,
        "several pages, no gap, no repeat"
    );
    assert_eq!(state_of(&store_b, DOC, &BTreeSet::new()), range(1, 450));
    let seqs: Vec<u64> = store_b.applied().iter().map(|u| u.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "applied in log order");
    drop(a);
}
