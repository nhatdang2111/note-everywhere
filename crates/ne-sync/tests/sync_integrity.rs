//! A device must stop, not skip, when the server hands it a row that does not
//! authenticate. Skipping would silently lose data. Skipped without
//! `NE_SYNC_TEST_URL`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use ne_crypto::envelope::seal_update;
use ne_sync::{MemoryStore, SyncFailure, SyncStatus};
use ne_sync_protocol::{ClientFrame, ClientId, PushEntry, ServerFrame};

use common::{RawSocket, fresh, start_engine, wait_for};

#[tokio::test]
async fn a_row_that_fails_authentication_stops_sync() {
    let h = fresh(require_server!()).await;
    let (client, signup) = h.sign_up().await;

    // Someone with a valid session but not the workspace key pushes junk.
    let mut intruder = RawSocket::connect(&client, &h.url, "intruder").await;
    intruder.pull_all(1, 0).await;
    let junk = PushEntry {
        client_id: ClientId([1; 16]),
        doc_id: "doc-main".into(),
        payload: vec![9; 80],
    };
    intruder
        .send(&ClientFrame::Push {
            req_id: 2,
            entries: vec![junk],
        })
        .await;
    assert!(matches!(
        intruder.recv().await,
        Some(ServerFrame::Ack { .. })
    ));

    let store = Arc::new(MemoryStore::new());
    let engine = start_engine(&client, &signup.unlocked.secrets, "device-a", &store, None);
    wait_for("sync to stop", Duration::from_secs(10), || async {
        engine.status() == SyncStatus::Error(SyncFailure::CorruptData)
    })
    .await;
    assert!(
        store.applied().is_empty(),
        "nothing from the bad row was applied"
    );
}

#[tokio::test]
async fn the_server_cannot_relabel_an_update_as_a_snapshot() {
    let h = fresh(require_server!()).await;
    let (client, signup) = h.sign_up().await;
    let secrets = &signup.unlocked.secrets;
    let workspace = &signup.unlocked.session.workspace_id;

    // A genuine update, sealed correctly, then replayed as a snapshot.
    let mut raw = RawSocket::connect(&client, &h.url, "device-evil").await;
    raw.pull_all(1, 0).await;
    let genuine = seal_update(
        &secrets.workspace_key,
        workspace,
        "doc-main",
        "device-evil",
        &[0, 1, 2, 3],
    )
    .expect("seal");
    let entry = PushEntry {
        client_id: ClientId([1; 16]),
        doc_id: "doc-main".into(),
        payload: genuine.clone(),
    };
    raw.send(&ClientFrame::Push {
        req_id: 2,
        entries: vec![entry],
    })
    .await;
    assert!(matches!(raw.recv().await, Some(ServerFrame::Ack { .. })));
    let relabelled = ClientFrame::Snapshot {
        req_id: 3,
        client_id: ClientId([2; 16]),
        doc_id: "doc-main".into(),
        upto: 1,
        payload: genuine,
    };
    raw.send(&relabelled).await;
    assert!(matches!(
        raw.recv().await,
        Some(ServerFrame::SnapshotAck { .. })
    ));

    let store = Arc::new(MemoryStore::new());
    let engine = start_engine(&client, secrets, "device-a", &store, None);
    wait_for("sync to stop", Duration::from_secs(10), || async {
        engine.status() == SyncStatus::Error(SyncFailure::CorruptData)
    })
    .await;
}
