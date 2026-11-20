//! Outbox primitives, sync cursor and metadata, settings.

mod store_support;

use std::collections::HashSet;

use ne_core::tree::Position;
use ne_store::{DocRef, Error};
use store_support::*;

#[test]
fn outbox_is_ordered_durable_and_acknowledged_by_client_id() {
    let (dir, mut ws) = new_workspace();
    assert_eq!(ws.outbox_len().expect("len"), 0);
    let first = ws.outbox_enqueue(&DocRef::Tree, b"one").expect("enqueue");
    let second = ws
        .outbox_enqueue(&DocRef::Vault("vault/a".into()), b"two")
        .expect("enqueue");
    let third = ws.outbox_enqueue(&DocRef::Tree, b"three").expect("enqueue");
    assert_eq!(
        HashSet::from([&first, &second, &third]).len(),
        3,
        "ids are unique"
    );
    assert!(first.len() >= 32, "ids are random enough to dedupe by");

    let items = ws.outbox_peek(2).expect("peek");
    assert_eq!(
        items.iter().map(|i| i.bytes.clone()).collect::<Vec<_>>(),
        [b"one".to_vec(), b"two".to_vec()]
    );
    assert_eq!(items[1].doc, DocRef::Vault("vault/a".into()));
    assert!(items[0].seq < items[1].seq);
    assert_eq!(
        ws.outbox_peek(2).expect("peek again"),
        items,
        "peeking removes nothing"
    );
    assert_eq!(ws.outbox_peek(0).expect("peek none"), []);

    // The same ids are reported again after a restart.
    drop(ws);
    let mut ws = reopen(dir.path());
    assert_eq!(ws.outbox_peek(10).expect("peek").len(), 3);
    assert_eq!(ws.outbox_peek(10).expect("peek")[0].client_id, first);

    // Acknowledging out of order, with unknown and repeated ids.
    let removed = ws
        .outbox_acknowledge(&[third.clone(), "unknown".into(), third.clone()])
        .expect("ack");
    assert_eq!(removed, 1);
    assert_eq!(ws.outbox_len().expect("len"), 2);
    ws.outbox_acknowledge(&[first, second]).expect("ack rest");
    assert_eq!(ws.outbox_len().expect("len"), 0);
    assert!(ws.outbox_peek(10).expect("peek").is_empty());
}

#[test]
fn every_local_change_queues_exactly_its_update() {
    let (_dir, mut ws) = new_workspace();
    let folder = ws.create_folder(None, Position::End, "F").expect("folder");
    ws.rename(folder, "G").expect("rename");
    let page = ws
        .create_page(Some(folder), Position::End, "P")
        .expect("page");
    ws.edit_page(page, |d| d.insert_text(&[0], 0, "x"))
        .expect("edit");
    let queued: Vec<DocRef> = ws
        .outbox_peek(100)
        .expect("peek")
        .into_iter()
        .map(|i| i.doc)
        .collect();
    assert_eq!(
        queued,
        [
            DocRef::Tree,
            DocRef::Tree,
            DocRef::Tree,
            DocRef::Page(page),
            DocRef::Page(page)
        ]
    );
    // Reads and no-op edits queue nothing.
    ws.open_page(page).expect("open");
    ws.edit_page(page, |_| Ok(())).expect("no-op edit");
    assert_eq!(ws.outbox_len().expect("len"), 5);
}

#[test]
fn sync_cursor_and_metadata_round_trip() {
    let (dir, mut ws) = new_workspace();
    assert_eq!(ws.sync_cursor().expect("cursor"), None);
    ws.set_sync_cursor(b"cursor-1").expect("set");
    ws.set_sync_cursor(&[0, 255, 7]).expect("overwrite");
    assert_eq!(ws.sync_cursor().expect("cursor"), Some(vec![0, 255, 7]));
    ws.set_sync_meta("server.epoch", b"42").expect("meta");
    assert_eq!(
        ws.sync_meta("server.epoch").expect("meta"),
        Some(b"42".to_vec())
    );
    assert_eq!(ws.sync_meta("missing").expect("meta"), None);
    drop(ws);
    let ws = reopen(dir.path());
    assert_eq!(ws.sync_cursor().expect("cursor"), Some(vec![0, 255, 7]));
    assert_eq!(
        ws.sync_meta("server.epoch").expect("meta"),
        Some(b"42".to_vec())
    );
}

#[test]
fn settings_are_a_key_value_store_with_a_reserved_prefix() {
    let (dir, mut ws) = new_workspace();
    assert_eq!(ws.setting("theme").expect("get"), None);
    ws.set_setting("theme", "tối").expect("set");
    ws.set_setting("theme", "sáng").expect("overwrite");
    assert_eq!(ws.setting("theme").expect("get"), Some("sáng".to_owned()));
    assert!(matches!(
        ws.set_setting("device.peer_id", "1"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        ws.remove_setting("device.peer_id"),
        Err(Error::InvalidInput(_))
    ));
    drop(ws);
    let mut ws = reopen(dir.path());
    assert_eq!(ws.setting("theme").expect("get"), Some("sáng".to_owned()));
    ws.remove_setting("theme").expect("remove");
    assert_eq!(ws.setting("theme").expect("get"), None);
}
