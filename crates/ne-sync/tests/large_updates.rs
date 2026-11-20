//! Updates and snapshots above the row limit, against a local server: they
//! travel as staged parts and are published atomically. Skipped without
//! `NE_SYNC_TEST_URL`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ne_sync::{IncomingKind, MemoryStore, SnapshotOutcome, SyncFailure, SyncStatus};
use ne_sync_protocol::ids::hex_encode;
use ne_sync_protocol::limits::{MAX_PART_PAYLOAD, MAX_PARTS, PART_CHUNK};
use ne_sync_protocol::{
    ClientFrame, ClientId, EntryKind, ErrorCode, LargeKind, Manifest, ServerFrame,
};

use common::{
    Harness, Proxy, RawSocket, fresh, raw_http, server_addr, start_engine, update_of, wait_for,
    wait_synced,
};

const DOC: &str = "doc-main";
const MIB: usize = 1024 * 1024;

/// Incompressible, reproducible bytes.
fn synthetic(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

/// Parts needed for a payload of `len` bytes once the kind byte (and, for a
/// snapshot, the covered position) is in front of it.
fn parts_for(len: usize, header: usize) -> u64 {
    (len + header).div_ceil(PART_CHUNK) as u64
}

async fn staged(h: &Harness) -> (u64, u64) {
    let body: serde_json::Value = h.get("/v1/_test/parts").await.json().await.expect("json");
    (
        body["rows"].as_u64().expect("rows"),
        body["bytes"].as_u64().expect("bytes"),
    )
}

async fn rows_written(h: &Harness) -> u64 {
    h.stats().await["rows_written"].as_u64().expect("rows")
}

fn updates_of(store: &MemoryStore, snapshots: bool) -> Vec<ne_sync::IncomingUpdate> {
    store
        .applied()
        .into_iter()
        .filter(|u| matches!(u.kind, IncomingKind::Snapshot { .. }) == snapshots)
        .collect()
}

#[tokio::test]
async fn a_five_mib_update_syncs_and_compaction_replaces_it_with_a_large_snapshot() {
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

    let big = synthetic(5 * MIB, 1);
    let rows_before = rows_written(&h).await;
    a.submit(DOC, &big).expect("submit");
    a.submit(DOC, &update_of(7)).expect("submit");
    wait_for("B to receive both", Duration::from_secs(60), || async {
        updates_of(&store_b, false).len() == 2
    })
    .await;
    wait_synced(&a).await;
    assert!(store_a.outbox().is_empty());
    let received = updates_of(&store_b, false);
    assert_eq!(received[0].plaintext, big, "applied whole and unchanged");
    assert_eq!(received[1].plaintext, update_of(7));
    assert_eq!(staged(&h).await.0, parts_for(big.len(), 1));
    // Row budget: one row write per part, a few for the log row (insert and two
    // indexes), a few for the small update, at most one session touch.
    let spent = rows_written(&h).await - rows_before;
    let parts = parts_for(big.len(), 1);
    println!("5 MiB update: {parts} parts, {spent} row writes in total");
    assert!(
        (parts + 3..=parts + 10).contains(&spent),
        "{spent} writes for {parts} parts"
    );

    // Compaction: the large update is replaced by a snapshot that is itself
    // above the row limit, and the old parts go with the old row.
    let mut state = big.clone();
    state.extend_from_slice(&update_of(7));
    let upto = a.snapshot_point().expect("point");
    match a
        .upload_snapshot(DOC, upto, &state)
        .await
        .expect("snapshot")
    {
        SnapshotOutcome::Accepted { dropped } => assert_eq!(dropped, 2),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(staged(&h).await.0, parts_for(state.len(), 9));

    // A second snapshot replaces the first one, parts included.
    a.submit(DOC, &update_of(8)).expect("submit");
    // The update is row 4 (row 3 is the first snapshot); its ack moves the cursor there.
    wait_for(
        "the ack of the new update",
        Duration::from_secs(20),
        || async { a.snapshot_point().expect("point") >= 4 },
    )
    .await;
    let mut newer = state.clone();
    newer.extend_from_slice(&update_of(8));
    let upto = a.snapshot_point().expect("point");
    let outcome = a
        .upload_snapshot(DOC, upto, &newer)
        .await
        .expect("snapshot");
    assert!(
        matches!(outcome, SnapshotOutcome::Accepted { .. }),
        "{outcome:?} at {upto}"
    );
    assert_eq!(staged(&h).await.0, parts_for(newer.len(), 9));

    // A device that joins now gets exactly the newest state and nothing else.
    let (client_c, unlocked_c) = h.login("device c").await;
    let store_c = Arc::new(MemoryStore::new());
    let c = start_engine(&client_c, &unlocked_c.secrets, "device-c", &store_c, None);
    wait_for("C to bootstrap", Duration::from_secs(60), || async {
        !store_c.applied().is_empty()
    })
    .await;
    wait_synced(&c).await;
    let applied = store_c.applied();
    assert_eq!(applied.len(), 1, "one snapshot replaces the whole history");
    assert!(matches!(applied[0].kind, IncomingKind::Snapshot { .. }));
    assert_eq!(applied[0].plaintext, newer);
}

#[tokio::test]
async fn a_thirty_mib_update_survives_disconnects_and_bootstraps_a_new_device() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let addr = server_addr(&h.url).await;
    let (proxy_a, proxy_b) = (
        Arc::new(Proxy::start(addr).await),
        Arc::new(Proxy::start(addr).await),
    );
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));
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
    wait_synced(&a).await;
    wait_synced(&b).await;

    // Cut both links over and over while the object is on its way, on the
    // sending side (mid-upload) and the receiving side (mid-download).
    let done = Arc::new(AtomicBool::new(false));
    let killer = {
        let (done, pa, pb) = (done.clone(), proxy_a.clone(), proxy_b.clone());
        tokio::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(400)).await;
                if done.load(Ordering::SeqCst) {
                    return;
                }
                pa.kill_connections();
                pb.kill_connections();
            }
        })
    };

    let big = synthetic(30 * MIB, 2);
    a.submit(DOC, &big).expect("submit");
    wait_for(
        "B to receive the object",
        Duration::from_secs(240),
        || async { !updates_of(&store_b, false).is_empty() },
    )
    .await;
    done.store(true, Ordering::SeqCst);
    killer.await.expect("killer");
    wait_synced(&a).await;
    assert!(store_a.outbox().is_empty());
    assert!(
        proxy_a.connections() >= 2,
        "the upload was interrupted at least once"
    );

    let received = updates_of(&store_b, false);
    assert_eq!(
        received.len(),
        1,
        "applied exactly once despite the retries"
    );
    assert!(received[0].plaintext == big, "applied whole and unchanged");
    // Retried uploads never stored a part twice.
    assert_eq!(staged(&h).await.0, parts_for(big.len(), 1));

    // Compaction with the same large state, then a brand new device.
    let upto = a.snapshot_point().expect("point");
    assert!(matches!(
        a.upload_snapshot(DOC, upto, &big).await.expect("snapshot"),
        SnapshotOutcome::Accepted { dropped: 1 }
    ));
    assert_eq!(staged(&h).await.0, parts_for(big.len(), 9));

    let (client_c, unlocked_c) = h.login("device c").await;
    let store_c = Arc::new(MemoryStore::new());
    let c = start_engine(&client_c, &unlocked_c.secrets, "device-c", &store_c, None);
    wait_for("C to bootstrap", Duration::from_secs(240), || async {
        !store_c.applied().is_empty()
    })
    .await;
    wait_synced(&c).await;
    let applied = store_c.applied();
    assert_eq!(applied.len(), 1);
    assert!(matches!(applied[0].kind, IncomingKind::Snapshot { .. }));
    assert!(applied[0].plaintext == big, "C holds identical content");
}

/// Raw access to the staging endpoint and the commit frame.
struct Raw {
    token: String,
    base: url::Url,
}

impl Raw {
    fn new(h: &Harness, client: &ne_sync::SyncClient) -> Raw {
        let session = client.session().expect("session");
        Raw {
            token: session.token.expose().to_owned(),
            base: h.url.clone(),
        }
    }

    fn part_url(&self, group: &[u8; 16], index: &str) -> url::Url {
        self.base
            .join(&format!("/v1/parts/{}/{index}", hex_encode(group)))
            .expect("url")
    }

    async fn put(&self, group: &[u8; 16], index: &str, body: Vec<u8>) -> reqwest::Response {
        raw_http()
            .put(self.part_url(group, index))
            .bearer_auth(&self.token)
            .body(body)
            .send()
            .await
            .expect("put")
    }
}

fn commit(req_id: u32, group: [u8; 16], parts: u16) -> ClientFrame {
    ClientFrame::Commit {
        req_id,
        client_id: ClientId(group),
        doc_id: DOC.into(),
        kind: LargeKind::Update,
        parts,
    }
}

#[tokio::test]
async fn commit_publishes_only_complete_objects_and_is_idempotent() {
    let h = fresh(require_server!()).await;
    let (client, _signup) = h.sign_up().await;
    let raw = Raw::new(&h, &client);
    let mut socket = RawSocket::connect(&client, &h.url, "raw-device").await;
    let group = [7u8; 16];

    // Nothing staged: refused, and no row appears.
    socket.send(&commit(1, group, 2)).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::Error {
            code: ErrorCode::Unavailable,
            ..
        })
    ));
    assert!(socket.pull_all(2, 0).await.is_empty());

    // One of two parts: still refused. Storing is idempotent.
    let first = raw.put(&group, "0", vec![1; 100]).await;
    assert_eq!(first.status(), 201);
    let again = raw.put(&group, "0", vec![1; 100]).await;
    assert_eq!(again.status(), 200);
    let listing: serde_json::Value = raw_http()
        .get(
            raw.base
                .join(&format!("/v1/parts/{}", hex_encode(&group)))
                .expect("url"),
        )
        .bearer_auth(&raw.token)
        .send()
        .await
        .expect("get")
        .json()
        .await
        .expect("json");
    assert_eq!(listing["have"], serde_json::json!([0]));
    socket.send(&commit(3, group, 2)).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::Error {
            code: ErrorCode::Unavailable,
            ..
        })
    ));
    assert!(socket.pull_all(4, 0).await.is_empty());

    // Complete: published once; a retry is acknowledged with seq 0.
    assert_eq!(raw.put(&group, "1", vec![2; 100]).await.status(), 201);
    socket.send(&commit(5, group, 2)).await;
    let seq = match socket.recv().await {
        Some(ServerFrame::Ack { seqs, .. }) => {
            assert_eq!(seqs.len(), 1);
            assert!(seqs[0] > 0);
            seqs[0]
        }
        other => panic!("unexpected {other:?}"),
    };
    socket.send(&commit(6, group, 2)).await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::Ack { seqs, .. }) if seqs == [0]
    ));
    let entries = socket.pull_all(7, 0).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].seq, entries[0].kind),
        (seq, EntryKind::LargeUpdate)
    );
    assert_eq!(entries[0].payload, Manifest { group, parts: 2 }.encode());
}

#[tokio::test]
async fn the_staging_endpoint_enforces_its_limits_and_needs_a_session() {
    let h = fresh(require_server!()).await;
    let (client, _signup) = h.sign_up().await;
    let raw = Raw::new(&h, &client);
    let group = [9u8; 16];

    assert_eq!(raw.put(&group, "0", vec![]).await.status(), 400);
    assert_eq!(
        raw.put(&group, "0", vec![0; MAX_PART_PAYLOAD + 1])
            .await
            .status(),
        413
    );
    assert_eq!(
        raw.put(&group, &MAX_PARTS.to_string(), vec![1])
            .await
            .status(),
        404
    );
    assert_eq!(raw.put(&group, "x", vec![1]).await.status(), 404);
    let bad_group = raw_http()
        .put(raw.base.join("/v1/parts/zz/0").expect("url"))
        .bearer_auth(&raw.token)
        .body(vec![1])
        .send()
        .await
        .expect("put");
    assert_eq!(bad_group.status(), 404);
    // A maximum-size part is accepted.
    assert_eq!(
        raw.put(&group, "0", vec![3; MAX_PART_PAYLOAD])
            .await
            .status(),
        201
    );
    // Missing parts are 404; no token is 401 for every verb.
    let missing = raw_http()
        .get(raw.part_url(&group, "5"))
        .bearer_auth(&raw.token)
        .send()
        .await
        .expect("get");
    assert_eq!(missing.status(), 404);
    let stored = raw_http()
        .get(raw.part_url(&group, "0"))
        .bearer_auth(&raw.token)
        .send()
        .await
        .expect("get");
    assert_eq!(stored.bytes().await.expect("bytes").len(), MAX_PART_PAYLOAD);
    for request in [
        raw_http().get(raw.part_url(&group, "0")),
        raw_http().put(raw.part_url(&group, "1")).body(vec![1]),
    ] {
        assert_eq!(request.send().await.expect("request").status(), 401);
    }
}

#[tokio::test]
async fn orphaned_parts_are_swept_and_compacted_ones_are_deleted() {
    let h = fresh(require_server!()).await;
    let (client, _signup) = h.sign_up().await;
    let raw = Raw::new(&h, &client);
    let mut socket = RawSocket::connect(&client, &h.url, "raw-device").await;

    // An upload that is never committed...
    let orphan = [1u8; 16];
    raw.put(&orphan, "0", vec![1; 500]).await;
    raw.put(&orphan, "1", vec![1; 500]).await;
    assert_eq!(staged(&h).await.0, 2);
    // ...survives while young, and goes when old and another upload starts.
    let young = [2u8; 16];
    raw.put(&young, "0", vec![1; 500]).await;
    assert_eq!(staged(&h).await.0, 3);
    assert_eq!(
        h.get("/v1/_test/age-parts").await.status(),
        404,
        "POST only"
    );
    let aged = raw_http()
        .post(h.url.join("/v1/_test/age-parts").expect("url"))
        .send()
        .await
        .expect("age");
    assert_eq!(aged.status(), 204);
    let fresh_group = [3u8; 16];
    raw.put(&fresh_group, "0", vec![1; 500]).await;
    assert_eq!(staged(&h).await.0, 1, "only the new upload remains");

    // A committed object is kept while its row lives, and a snapshot that
    // covers the row deletes the parts.
    raw.put(&fresh_group, "1", vec![1; 500]).await;
    socket.send(&commit(1, fresh_group, 2)).await;
    let Some(ServerFrame::Ack { head, .. }) = socket.recv().await else {
        panic!("commit not acknowledged");
    };
    let aged = raw_http()
        .post(h.url.join("/v1/_test/age-parts").expect("url"))
        .send()
        .await
        .expect("age");
    assert_eq!(aged.status(), 204);
    raw.put(&[4u8; 16], "0", vec![1; 10]).await;
    assert_eq!(
        staged(&h).await.0,
        3,
        "the committed group is not an orphan"
    );
    socket
        .send(&ClientFrame::Snapshot {
            req_id: 2,
            client_id: ClientId([5; 16]),
            doc_id: DOC.into(),
            upto: head,
            payload: vec![9; 64],
        })
        .await;
    assert!(matches!(
        socket.recv().await,
        Some(ServerFrame::SnapshotAck {
            outcome: SnapshotOutcome::Accepted { dropped: 1 },
            ..
        })
    ));
    assert_eq!(
        staged(&h).await.0,
        1,
        "the compacted object's parts are gone"
    );
}

#[tokio::test]
async fn a_part_that_fails_authentication_stops_the_receiver() {
    let h = fresh(require_server!()).await;
    let (client_a, _signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let raw = Raw::new(&h, &client_a);
    let mut socket = RawSocket::connect(&client_a, &h.url, "raw-device").await;

    // Garbage in place of sealed parts, published as if it were an update.
    let group = [6u8; 16];
    raw.put(&group, "0", vec![5; 200]).await;
    socket.send(&commit(1, group, 1)).await;
    assert!(matches!(socket.recv().await, Some(ServerFrame::Ack { .. })));

    let store_b = Arc::new(MemoryStore::new());
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_for("B to stop", Duration::from_secs(20), || async {
        b.status() == SyncStatus::Error(SyncFailure::CorruptData)
    })
    .await;
    assert!(store_b.applied().is_empty(), "nothing was applied");
    assert_eq!(store_b.outbox().len(), 0);
}
