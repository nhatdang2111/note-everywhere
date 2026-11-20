//! Wire-level behaviour (idempotency, ordering, strict parsing) and snapshot
//! compaction against a local server. Skipped without `NE_SYNC_TEST_URL`.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ne_sync::{MemoryStore, SnapshotOutcome};
use ne_sync_protocol::{ClientFrame, ClientId, EntryKind, ErrorCode, PushEntry, ServerFrame};

use common::{
    RawSocket, applied_count, fresh, snapshot_of, start_engine, state_of, update_of, wait_for,
    wait_synced,
};

const DOC: &str = "doc-main";

fn id(n: u8) -> ClientId {
    ClientId([n; 16])
}

fn entry(n: u8, doc: &str) -> PushEntry {
    PushEntry {
        client_id: id(n),
        doc_id: doc.into(),
        payload: vec![n; 64],
    }
}

fn push(req_id: u32, entries: Vec<PushEntry>) -> ClientFrame {
    ClientFrame::Push { req_id, entries }
}

async fn ack_seqs(socket: &mut RawSocket, expected_req: u32) -> Vec<u64> {
    match socket.recv().await.expect("ack") {
        ServerFrame::Ack { req_id, seqs, .. } => {
            assert_eq!(req_id, expected_req);
            seqs
        }
        other => panic!("expected an ack, got {other:?}"),
    }
}

#[tokio::test]
async fn duplicate_and_out_of_order_pushes_are_stored_once() {
    let h = fresh(require_server!()).await;
    let (client, _signup) = h.sign_up().await;
    let mut sender = RawSocket::connect(&client, &h.url, "raw-sender").await;
    let mut watcher = RawSocket::connect(&client, &h.url, "raw-watcher").await;
    // The keepalive is answered by the runtime itself.
    assert_eq!(sender.ping().await, "pong");
    // Both catch up first; live fan-out only starts after that.
    assert!(sender.pull_all(1, 0).await.is_empty());
    assert!(watcher.pull_all(1, 0).await.is_empty());

    // Request 10 is sent before request 9: both are stored in arrival order.
    sender
        .send(&push(10, vec![entry(3, "d1"), entry(4, "d1")]))
        .await;
    sender
        .send(&push(9, vec![entry(1, "d1"), entry(2, "d2")]))
        .await;
    assert_eq!(ack_seqs(&mut sender, 10).await, vec![1, 2]);
    assert_eq!(ack_seqs(&mut sender, 9).await, vec![3, 4]);

    // A retry of already-stored ids acknowledges with 0 and stores nothing.
    sender
        .send(&push(
            11,
            vec![entry(1, "d1"), entry(3, "d1"), entry(5, "d1")],
        ))
        .await;
    assert_eq!(ack_seqs(&mut sender, 11).await, vec![0, 0, 5]);
    // The same id twice in one push.
    sender
        .send(&push(12, vec![entry(6, "d1"), entry(6, "d1")]))
        .await;
    assert_eq!(ack_seqs(&mut sender, 12).await, vec![6, 0]);

    // The watcher got every stored row exactly once, in order, from the other device.
    let mut live = Vec::new();
    while live.len() < 6 {
        match watcher.recv().await.expect("live frame") {
            ServerFrame::Updates {
                req_id: 0, entries, ..
            } => live.extend(entries),
            other => panic!("unexpected {other:?}"),
        }
    }
    let seqs: Vec<u64> = live.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6]);
    assert!(
        live.iter()
            .all(|e| e.device_id == "raw-sender" && e.kind == EntryKind::Update)
    );
    // A fresh pull shows the same six rows and nothing else.
    let pulled = watcher.pull_all(2, 0).await;
    assert_eq!(pulled.iter().map(|e| e.seq).collect::<Vec<_>>(), seqs);
    // Pulling from the middle returns only what is newer.
    assert_eq!(watcher.pull_all(3, 4).await.len(), 2);
    // The sender does not get its own rows echoed live.
    sender.send(&push(13, vec![entry(7, "d1")])).await;
    assert_eq!(ack_seqs(&mut sender, 13).await, vec![7]);
}

#[tokio::test]
async fn malformed_and_oversized_frames_are_rejected() {
    let h = fresh(require_server!()).await;
    let (client, _signup) = h.sign_up().await;

    let mut socket = RawSocket::connect(&client, &h.url, "raw-1").await;
    socket.send_raw(vec![1, 0x77, 0, 0]).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::Error {
            code: ErrorCode::Malformed,
            ..
        })
    ));
    assert!(
        socket.recv().await.is_none(),
        "the connection is closed after a violation"
    );

    let mut socket = RawSocket::connect(&client, &h.url, "raw-2").await;
    socket.send_raw(vec![0u8; 1024 * 1024 + 1]).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::Error {
            code: ErrorCode::TooLarge,
            ..
        }) | None
    ));

    // A snapshot that claims entries the log does not have is rejected.
    let mut socket = RawSocket::connect(&client, &h.url, "raw-3").await;
    assert!(socket.pull_all(1, 0).await.is_empty());
    let claim = ClientFrame::Snapshot {
        req_id: 2,
        client_id: id(1),
        doc_id: DOC.into(),
        upto: 5,
        payload: vec![1; 32],
    };
    socket.send(&claim).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::SnapshotAck {
            outcome: SnapshotOutcome::Rejected,
            ..
        })
    ));
}

#[tokio::test]
async fn snapshot_compaction_lets_a_fresh_device_bootstrap_to_identical_content() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));
    let a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        None,
    );
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_synced(&a).await;
    wait_synced(&b).await;

    let (own_a, own_b): (BTreeSet<u32>, BTreeSet<u32>) = ((1..=30).collect(), (31..=40).collect());
    for n in &own_a {
        a.submit(DOC, &update_of(*n)).expect("submit");
        a.submit("doc-untouched", &update_of(500 + n))
            .expect("submit");
    }
    for n in &own_b {
        b.submit(DOC, &update_of(*n)).expect("submit");
    }
    let everything: BTreeSet<u32> = (1..=40).collect();
    wait_for("A and B to converge", Duration::from_secs(30), || async {
        state_of(&store_a, DOC, &own_a) == everything
            && state_of(&store_b, DOC, &own_b) == everything
    })
    .await;
    wait_synced(&a).await;
    wait_synced(&b).await;

    // B goes away; A compacts the document and the work continues without B.
    let b_cursor = ne_sync::SyncStore::cursor(&*store_b).expect("cursor");
    b.shutdown().await;
    let upto = a.snapshot_point().expect("point");
    let state = state_of(&store_a, DOC, &own_a);
    let outcome = a
        .upload_snapshot(DOC, upto, &snapshot_of(&state))
        .await
        .expect("snapshot");
    assert_eq!(outcome, SnapshotOutcome::Accepted { dropped: 40 });
    // Re-sending the same position, or going backwards, is stale; claiming the future is rejected.
    assert_eq!(
        a.upload_snapshot(DOC, upto, &snapshot_of(&state))
            .await
            .expect("stale"),
        SnapshotOutcome::Stale
    );
    assert_eq!(
        a.upload_snapshot(DOC, 1, &snapshot_of(&state))
            .await
            .expect("stale"),
        SnapshotOutcome::Stale
    );
    assert_eq!(
        a.upload_snapshot(DOC, upto + 1000, &snapshot_of(&state))
            .await
            .expect("rejected"),
        SnapshotOutcome::Rejected
    );
    let own_a_later: BTreeSet<u32> = own_a.union(&(41..=45).collect()).copied().collect();
    for n in 41..=45 {
        a.submit(DOC, &update_of(n)).expect("submit");
    }
    wait_synced(&a).await;
    wait_for(
        "later edits to be acknowledged",
        Duration::from_secs(10),
        || async { store_a.outbox().is_empty() },
    )
    .await;

    // A brand-new device bootstraps from the snapshot plus the remaining log.
    let (client_c, unlocked_c) = h.login("device c").await;
    let store_c = Arc::new(MemoryStore::new());
    let c = start_engine(&client_c, &unlocked_c.secrets, "device-c", &store_c, None);
    wait_synced(&c).await;
    let expected: BTreeSet<u32> = (1..=45).collect();
    assert_eq!(state_of(&store_c, DOC, &BTreeSet::new()), expected);
    assert_eq!(applied_count(&store_c, true), 1, "one snapshot");
    let later_updates = store_c
        .applied()
        .iter()
        .filter(|u| u.doc_id == DOC && matches!(u.kind, ne_sync::IncomingKind::Update))
        .count();
    assert_eq!(
        later_updates, 5,
        "only the updates after the snapshot remain in the log"
    );
    // The untouched document was not compacted and arrives whole.
    let untouched = store_c
        .applied()
        .iter()
        .filter(|u| u.doc_id == "doc-untouched")
        .count();
    assert_eq!(untouched, 30);

    // B, which went offline before the compaction, catches up from its old position too.
    assert!(b_cursor < upto + 1);
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_for("B to catch up", Duration::from_secs(20), || async {
        state_of(&store_b, DOC, &own_b) == expected
    })
    .await;
    wait_synced(&b).await;
    assert_eq!(state_of(&store_a, DOC, &own_a_later), expected);
    drop((a, b, c));
}

#[tokio::test]
async fn a_device_that_was_offline_during_compaction_never_misses_content() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));
    let a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        None,
    );
    let mut b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_synced(&a).await;
    wait_synced(&b).await;

    // B sees the first batch, then drops out.
    let mut own_a = BTreeSet::new();
    for n in 1..=10 {
        a.submit(DOC, &update_of(n)).expect("submit");
        own_a.insert(n);
    }
    wait_for(
        "B to see the first batch",
        Duration::from_secs(20),
        || async { state_of(&store_b, DOC, &BTreeSet::new()).len() == 10 },
    )
    .await;
    b.shutdown().await;

    // While B is away: more edits, a compaction that covers all of it, and one more edit.
    for n in 11..=25 {
        a.submit(DOC, &update_of(n)).expect("submit");
        own_a.insert(n);
    }
    wait_for("A to push", Duration::from_secs(20), || async {
        store_a.outbox().is_empty()
    })
    .await;
    wait_synced(&a).await;
    let upto = a.snapshot_point().expect("point");
    let outcome = a
        .upload_snapshot(DOC, upto, &snapshot_of(&own_a))
        .await
        .expect("snapshot");
    assert!(matches!(outcome, SnapshotOutcome::Accepted { dropped } if dropped >= 15));
    a.submit(DOC, &update_of(26)).expect("submit");
    own_a.insert(26);
    wait_for("A to push", Duration::from_secs(20), || async {
        store_a.outbox().is_empty()
    })
    .await;

    // B returns with a cursor older than the compaction and still ends identical.
    b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    let want: BTreeSet<u32> = (1..=26).collect();
    wait_for("B to catch up", Duration::from_secs(20), || async {
        state_of(&store_b, DOC, &BTreeSet::new()) == want
    })
    .await;
    wait_synced(&b).await;
    assert_eq!(
        applied_count(&store_b, true),
        1,
        "the snapshot filled the gap"
    );
    drop(a);
}

#[tokio::test]
async fn live_connections_survive_the_object_hibernating() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));
    let a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        None,
    );
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_synced(&a).await;
    wait_synced(&b).await;
    a.submit(DOC, &update_of(1)).expect("submit");
    wait_for(
        "first update to arrive",
        Duration::from_secs(10),
        || async { store_b.applied().len() == 1 },
    )
    .await;
    let wakes_before = h.stats().await["wakes"].as_u64().expect("wakes");

    // Idle long enough for the runtime to evict the object from memory.
    tokio::time::sleep(Duration::from_secs(14)).await;
    a.submit(DOC, &update_of(2)).expect("submit");
    wait_for(
        "update after the idle period",
        Duration::from_secs(10),
        || async { store_b.applied().len() == 2 },
    )
    .await;
    wait_synced(&a).await;
    let wakes_after = h.stats().await["wakes"].as_u64().expect("wakes");
    assert!(
        wakes_after > wakes_before,
        "the object was reconstructed ({wakes_before} -> {wakes_after})"
    );
    drop((a, b));
}

#[tokio::test]
async fn a_retry_after_compaction_is_recognised_until_the_tombstones_expire() {
    let h = fresh(require_server!()).await;
    let (client, _signup) = h.sign_up().await;
    let mut socket = RawSocket::connect(&client, &h.url, "raw-sender").await;
    assert!(socket.pull_all(1, 0).await.is_empty());

    socket
        .send(&push(2, vec![entry(1, "d1"), entry(2, "d1")]))
        .await;
    assert_eq!(ack_seqs(&mut socket, 2).await, vec![1, 2]);
    socket.send(&push(3, vec![entry(3, "d2")])).await;
    assert_eq!(ack_seqs(&mut socket, 3).await, vec![3]);
    let compact = |req_id, client_byte, doc: &str, upto| ClientFrame::Snapshot {
        req_id,
        client_id: id(client_byte),
        doc_id: doc.into(),
        upto,
        payload: vec![7; 40],
    };
    socket.send(&compact(4, 100, "d1", 3)).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::SnapshotAck {
            outcome: SnapshotOutcome::Accepted { dropped: 2 },
            ..
        })
    ));

    // The two updates were compacted away, yet their ids are still remembered.
    socket
        .send(&push(5, vec![entry(1, "d1"), entry(2, "d1")]))
        .await;
    assert_eq!(ack_seqs(&mut socket, 5).await, vec![0, 0]);
    let rows = socket.pull_all(6, 0).await;
    assert_eq!(
        rows.iter().filter(|e| e.doc_id == "d1").count(),
        1,
        "only the snapshot of d1 remains"
    );

    // Once the tombstones are older than their retention, the next compaction
    // forgets them, and a retry is stored again (harmless for CRDT updates).
    let aged = common::raw_http()
        .post(h.url.join("/v1/_test/age-tombstones").expect("url"))
        .send()
        .await;
    assert_eq!(aged.expect("send").status(), 204);
    socket.send(&push(7, vec![entry(4, "d2")])).await;
    let fourth = ack_seqs(&mut socket, 7).await[0];
    socket.send(&compact(8, 101, "d2", fourth)).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::SnapshotAck {
            outcome: SnapshotOutcome::Accepted { .. },
            ..
        })
    ));
    socket.send(&push(9, vec![entry(1, "d1")])).await;
    assert_ne!(
        ack_seqs(&mut socket, 9).await,
        vec![0],
        "the expired tombstone no longer dedupes"
    );
}
