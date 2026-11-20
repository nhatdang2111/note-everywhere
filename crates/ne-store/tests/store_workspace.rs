//! Workspace lifecycle, tree persistence, page updates, remote sync,
//! compaction and cascading deletion.

mod store_support;

use ne_core::page::{Block, BlockKind};
use ne_core::tree::{NodeKind, Position};
use ne_store::{DocRef, Error, StoreConfig, Workspace};
use store_support::*;

#[test]
fn create_open_and_reject_duplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(matches!(
        Workspace::open(dir.path(), config()),
        Err(Error::NoWorkspace(_))
    ));
    let ws = Workspace::create(dir.path(), config()).expect("create");
    let peer = ws.peer_id();
    assert_eq!(
        ws.schema_version().expect("version"),
        ne_store::SCHEMA_VERSION
    );
    ws.check_integrity().expect("sound database");
    drop(ws);

    assert!(matches!(
        Workspace::create(dir.path(), config()),
        Err(Error::AlreadyExists(_))
    ));
    let again = Workspace::open(dir.path(), config()).expect("open");
    assert_eq!(
        again.peer_id(),
        peer,
        "the device id is stable across sessions"
    );
    assert!(dir.path().join(ne_store::DB_FILE).is_file());
    assert!(dir.path().join(ne_store::ATTACHMENTS_DIR).is_dir());
    let third = Workspace::open_or_create(dir.path(), config()).expect("open_or_create");
    assert_eq!(third.peer_id(), peer);
    let fresh = tempfile::tempdir().expect("tempdir");
    Workspace::open_or_create(fresh.path(), config()).expect("creates when missing");
}

#[test]
fn tree_edits_survive_a_restart_and_the_sidebar_matches_the_tree() {
    let (dir, mut ws) = new_workspace();
    let folder = ws
        .create_folder(None, Position::End, "Dự án")
        .expect("folder");
    let a = ws
        .create_page(Some(folder), Position::End, "Trang A")
        .expect("page");
    let b = ws
        .create_page(Some(folder), Position::End, "Trang B")
        .expect("page");
    let c = ws
        .create_page(None, Position::End, "Trang C")
        .expect("page");
    ws.rename(a, "Trang A (đã đổi tên)").expect("rename");
    ws.set_icon(a, Some("📝")).expect("icon");
    ws.set_favorite(c, true).expect("favorite");
    ws.reorder(b, Position::Index(0)).expect("reorder");
    ws.move_node(c, Some(folder), Position::Index(1))
        .expect("move");
    let gone = ws.create_page(None, Position::End, "Sẽ xoá").expect("page");
    ws.trash_node(gone).expect("trash");

    let check = |ws: &Workspace| {
        let tree = ws.tree().snapshot().expect("snapshot");
        let sidebar = ws.sidebar().expect("sidebar");
        assert_eq!(sidebar.len(), tree.len());
        for info in &tree {
            let row = sidebar
                .iter()
                .find(|r| r.id == info.id)
                .expect("row exists");
            assert_eq!(row.parent, info.parent);
            assert_eq!(row.title, info.title);
            assert_eq!(row.kind, info.kind);
            assert_eq!(row.icon, info.icon);
            assert_eq!(row.favorite, info.favorite);
            assert_eq!(row.trashed_at, info.trashed_at);
            assert!(!row.hidden);
        }
        for parent in [None, Some(folder)] {
            let expected: Vec<_> = ws
                .tree()
                .children(parent, false)
                .expect("children")
                .iter()
                .map(|n| n.id)
                .collect();
            let got: Vec<_> = ws
                .children(parent)
                .expect("children")
                .iter()
                .map(|r| r.id)
                .collect();
            assert_eq!(got, expected, "order under {parent:?}");
        }
        assert_eq!(ws.favorites().expect("favorites").len(), 1);
        assert_eq!(ws.trash().expect("trash").len(), 1);
        let trashed = ws.node(gone).expect("node").expect("row");
        assert!(trashed.hidden && trashed.trashed_at.is_some());
    };
    check(&ws);
    let before = ws.tree().snapshot().expect("snapshot");
    drop(ws);

    let ws = reopen(dir.path());
    assert_eq!(ws.tree().snapshot().expect("snapshot"), before);
    check(&ws);
    let titles: Vec<_> = ws
        .children(Some(folder))
        .expect("children")
        .into_iter()
        .map(|r| r.title)
        .collect();
    assert_eq!(titles, ["Trang B", "Trang C", "Trang A (đã đổi tên)"]);
}

#[test]
fn trashing_hides_the_whole_subtree_and_restoring_brings_it_back() {
    let (_dir, mut ws) = new_workspace();
    let folder = ws
        .create_folder(None, Position::End, "Thư mục")
        .expect("folder");
    let inner = ws
        .create_folder(Some(folder), Position::End, "Trong")
        .expect("folder");
    let page = ws
        .create_page(Some(inner), Position::End, "Sâu")
        .expect("page");
    ws.trash_node(folder).expect("trash");
    assert!(ws.sidebar().expect("sidebar").is_empty());
    assert!(ws.node(page).expect("node").expect("row").hidden);
    ws.restore(folder).expect("restore");
    assert_eq!(ws.sidebar().expect("sidebar").len(), 3);
    assert!(!ws.node(page).expect("node").expect("row").hidden);

    ws.trash_node(inner).expect("trash inner");
    assert_eq!(ws.sidebar().expect("sidebar").len(), 1);
    ws.move_node(page, None, Position::End)
        .expect_err("a page below a trashed folder cannot be moved");
}

#[test]
fn creating_a_page_stores_its_skeleton_and_queues_it_for_sync() {
    let (dir, mut ws) = new_workspace();
    let id = ws.create_page(None, Position::End, "Mới").expect("page");
    let queue = ws.outbox_peek(10).expect("peek");
    assert_eq!(queue.len(), 2, "tree update, then the page's first update");
    assert_eq!(queue[0].doc, DocRef::Tree);
    assert_eq!(queue[1].doc, DocRef::Page(id));
    assert_ne!(queue[0].client_id, queue[1].client_id);
    assert!(queue[0].seq < queue[1].seq);

    // The queued first update alone builds the page on another device.
    let replica = ne_core::page::PageDoc::with_peer_id(77).expect("page");
    replica.import(&queue[1].bytes).expect("import skeleton");
    assert_eq!(
        replica.blocks().expect("blocks"),
        [Block::new(BlockKind::Paragraph)]
    );

    let snapshot = ws.page_snapshot(id).expect("snapshot");
    let editor = ne_core::page::PageDoc::from_snapshot(&snapshot).expect("editor doc");
    assert_eq!(editor.blocks().expect("blocks").len(), 1);
    drop(ws);
    let mut ws = reopen(dir.path());
    assert_eq!(ws.outbox_len().expect("len"), 2, "the outbox is durable");
    assert_eq!(page_text(&mut ws, id), "");
}

#[test]
fn opening_a_folder_as_a_page_is_rejected() {
    let (_dir, mut ws) = new_workspace();
    let folder = ws.create_folder(None, Position::End, "F").expect("folder");
    assert!(matches!(ws.open_page(folder), Err(Error::InvalidInput(_))));
    let page = ws.create_page(None, Position::End, "P").expect("page");
    ws.delete_permanently(page).expect_err("not trashed");
    ws.trash_node(page).expect("trash");
    ws.delete_permanently(page).expect("delete");
    assert!(matches!(ws.open_page(page), Err(Error::Core(_))));
}

#[test]
fn editor_updates_are_stored_indexed_and_queued() {
    let (dir, mut ws) = new_workspace();
    let id = ws
        .create_page(None, Position::End, "Ghi chú")
        .expect("page");
    ws.outbox_acknowledge(
        &ws.outbox_peek(10)
            .expect("peek")
            .iter()
            .map(|i| i.client_id.clone())
            .collect::<Vec<_>>(),
    )
    .expect("ack");

    let editor = editor_replica(&mut ws, id);
    let before = editor.version();
    editor
        .insert_text(&[0], 0, "Xin chào thế giới")
        .expect("type");
    editor
        .append_block(&[], &Block::paragraph("Đoạn thứ hai"))
        .expect("append");
    let update = editor.export_updates_since(&before).expect("update");

    let change = ws.apply_local_update(id, &update).expect("apply");
    assert_eq!(change.page, Some(id));
    assert!(!change.missing_dependencies);
    assert_eq!(page_text(&mut ws, id), "Xin chào thế giới\nĐoạn thứ hai");
    assert_eq!(ws.word_count(id).expect("words"), Some(7));
    let hits = ws.search("the gioi", 10).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id);

    let queued = ws.outbox_peek(10).expect("peek");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].doc, DocRef::Page(id));
    assert_eq!(
        queued[0].bytes, update,
        "the editor's bytes go out unchanged"
    );

    // A second update builds on the first; garbage bytes are rejected and change nothing.
    editor.insert_text(&[1], 0, "→ ").expect("type");
    let second = editor
        .export_updates_since(&ws.open_page(id).expect("open").version())
        .expect("update");
    assert!(ws.apply_local_update(id, b"not an update").is_err());
    assert_eq!(ws.outbox_len().expect("len"), 1);
    ws.apply_local_update(id, &second).expect("apply second");
    drop(ws);

    let mut ws = reopen(dir.path());
    assert_eq!(page_text(&mut ws, id), "Xin chào thế giới\n→ Đoạn thứ hai");
    assert_eq!(ws.search("doan thu hai", 10).expect("search").len(), 1);
}

#[test]
fn devices_converge_through_the_outbox() {
    let (_da, mut a) = new_workspace();
    let (_db, mut b) = new_workspace();
    assert_ne!(a.peer_id(), b.peer_id());
    // B first learns A's workspace, then both work offline and exchange.
    deliver(&mut a, &mut b);

    let folder = a
        .create_folder(None, Position::End, "Từ A")
        .expect("folder");
    let page = page_in(
        &mut a,
        Some(folder),
        "Trang chung",
        &paragraphs(&["Dòng của A"]),
    );
    deliver(&mut a, &mut b);
    assert_eq!(
        b.tree().snapshot().expect("b tree"),
        a.tree().snapshot().expect("a tree")
    );
    assert_eq!(page_text(&mut b, page), "Dòng của A");
    assert_eq!(
        b.outbox_len().expect("len"),
        0,
        "applying remote updates queues nothing"
    );

    // Concurrent edits to the same paragraph and to the tree merge.
    a.edit_page(page, |d| d.insert_text(&[0], 0, "[A] "))
        .expect("a edits");
    b.edit_page(page, |d| d.insert_text(&[0], 10, " [B]"))
        .expect("b edits");
    a.rename(page, "Tên từ A").expect("rename");
    b.set_favorite(page, true).expect("favorite");
    deliver(&mut a, &mut b);
    deliver(&mut b, &mut a);
    assert_eq!(page_text(&mut a, page), page_text(&mut b, page));
    assert_eq!(page_text(&mut a, page), "[A] Dòng của A [B]");
    let (ta, tb) = (
        a.tree().snapshot().expect("a"),
        b.tree().snapshot().expect("b"),
    );
    assert_eq!(ta, tb);
    let node = ta.iter().find(|n| n.id == page).expect("page node");
    assert_eq!((node.title.as_str(), node.favorite), ("Tên từ A", true));
    assert_eq!(
        a.dump_derived_indexes().expect("dump a"),
        b.dump_derived_indexes().expect("dump b")
    );
    assert_eq!(b.search("ten tu a", 5).expect("search")[0].id, page);
}

#[test]
fn remote_updates_are_idempotent_and_report_changes() {
    let (_da, mut a) = new_workspace();
    let (_db, mut b) = new_workspace();
    let folder = a.create_folder(None, Position::End, "F").expect("folder");
    let queue = a.outbox_peek(10).expect("peek");
    let first = b
        .apply_remote_update(&queue[0].doc, &queue[0].bytes)
        .expect("apply");
    assert_eq!(first.nodes.into_iter().collect::<Vec<_>>(), [folder]);
    let again = b
        .apply_remote_update(&queue[0].doc, &queue[0].bytes)
        .expect("apply again");
    assert_eq!(again, Default::default());
    assert_eq!(b.outbox_len().expect("len"), 0);
    assert_eq!(b.sidebar().expect("sidebar").len(), 1);
}

#[test]
fn a_page_update_that_arrives_before_the_page_node_is_indexed_when_the_node_arrives() {
    let (_da, mut a) = new_workspace();
    let (_db, mut b) = new_workspace();
    let page = page_with(&mut a, "Đến muộn", &paragraphs(&["nội dung độc đáo xyz"]));
    let queue = a.outbox_peek(20).expect("peek");
    // Deliver page updates first, tree updates after.
    for item in queue.iter().filter(|i| matches!(i.doc, DocRef::Page(_))) {
        b.apply_remote_update(&item.doc, &item.bytes)
            .expect("page update");
    }
    assert!(
        b.search("doc dao", 5).expect("search").is_empty(),
        "no node yet, so not searchable"
    );
    for item in queue.iter().filter(|i| i.doc == DocRef::Tree) {
        b.apply_remote_update(&item.doc, &item.bytes)
            .expect("tree update");
    }
    let hits = b.search("doc dao xyz", 5).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, page);
    assert_eq!(
        b.dump_derived_indexes().expect("b"),
        a.dump_derived_indexes().expect("a")
    );
}

#[test]
fn compaction_folds_the_update_log_without_changing_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut config = config();
    config.compact_after_updates = 5;
    let mut ws = Workspace::create(dir.path(), config.clone()).expect("create");
    let id = ws
        .create_page(None, Position::End, "Nhiều lần sửa")
        .expect("page");
    for i in 0..23 {
        ws.edit_page(id, |d| {
            d.append_block(&[], &Block::paragraph(&format!("dòng {i}")))
        })
        .expect("edit");
        let (count, _) = ws.update_log_size(&DocRef::Page(id)).expect("log size");
        assert!(
            count < 5,
            "the log is folded as soon as it reaches the threshold ({count})"
        );
    }
    let expected = page_text(&mut ws, id);
    assert!(expected.ends_with("dòng 22"));
    drop(ws);
    let mut ws = Workspace::open(dir.path(), config).expect("open");
    assert_eq!(page_text(&mut ws, id), expected);

    // The tree log compacts too, and forcing a compaction empties the log.
    for i in 0..12 {
        ws.create_folder(None, Position::End, &format!("thư mục {i}"))
            .expect("folder");
    }
    assert!(ws.update_log_size(&DocRef::Tree).expect("size").0 < 5);
    ws.compact(&DocRef::Tree).expect("compact");
    ws.compact(&DocRef::Page(id)).expect("compact page");
    assert_eq!(ws.update_log_size(&DocRef::Tree).expect("size"), (0, 0));
    let before = ws.tree().snapshot().expect("snapshot");
    drop(ws);
    let ws = reopen(dir.path());
    assert_eq!(ws.tree().snapshot().expect("snapshot"), before);
}

#[test]
fn deleting_a_subtree_removes_documents_index_entries_and_links() {
    let (dir, mut ws) = new_workspace();
    let folder = ws
        .create_folder(None, Position::End, "Xoá cả nhánh")
        .expect("folder");
    let inner = page_in(
        &mut ws,
        Some(folder),
        "Trang trong",
        &paragraphs(&["từ khoá hiếm qwertyuiop"]),
    );
    let deep = page_in(&mut ws, Some(inner), "Trang sâu", &paragraphs(&["sâu hơn"]));
    let keeper = page_with(
        &mut ws,
        "Giữ lại",
        &[Block::new(BlockKind::PageLink {
            page_id: inner.to_string(),
            title: "x".into(),
        })],
    );
    assert_eq!(ws.backlinks(inner).expect("backlinks").len(), 1);
    assert_eq!(ws.search("qwertyuiop", 5).expect("search").len(), 1);

    ws.trash_node(folder).expect("trash");
    assert!(
        ws.search("qwertyuiop", 5).expect("search").is_empty(),
        "trashed pages are not found"
    );
    let removed = ws.delete_permanently(folder).expect("delete");
    assert_eq!(removed.len(), 3);
    assert!(removed.contains(&deep));

    assert!(ws.search("qwertyuiop", 5).expect("search").is_empty());
    assert!(ws.node(inner).expect("node").is_none());
    assert!(
        ws.update_log_size(&DocRef::Page(inner)).is_err(),
        "the document is gone"
    );
    assert!(ws.update_log_size(&DocRef::Page(deep)).is_err());
    assert!(ws.update_log_size(&DocRef::Page(keeper)).is_ok());
    drop(ws);
    let mut ws = reopen(dir.path());
    assert!(ws.open_page(inner).is_err());
    assert!(
        ws.sidebar()
            .expect("sidebar")
            .iter()
            .all(|r| r.id == keeper)
    );
}

#[test]
fn remote_permanent_deletion_cascades_to_documents() {
    let (_da, mut a) = new_workspace();
    let (_db, mut b) = new_workspace();
    let folder = a.create_folder(None, Position::End, "F").expect("folder");
    let page = page_in(
        &mut a,
        Some(folder),
        "P",
        &paragraphs(&["nội dung riêng biệt"]),
    );
    deliver(&mut a, &mut b);
    assert_eq!(b.search("rieng biet", 5).expect("search").len(), 1);

    a.trash_node(folder).expect("trash");
    a.delete_permanently(folder).expect("delete");
    let queue = a.outbox_peek(10).expect("peek");
    let mut removed = std::collections::BTreeSet::new();
    for item in &queue {
        removed.extend(
            b.apply_remote_update(&item.doc, &item.bytes)
                .expect("apply")
                .removed,
        );
    }
    assert!(
        removed.contains(&folder) && removed.contains(&page),
        "{removed:?}"
    );
    assert!(b.search("rieng biet", 5).expect("search").is_empty());
    assert!(b.update_log_size(&DocRef::Page(page)).is_err());
    assert!(b.sidebar().expect("sidebar").is_empty());
    assert_eq!(
        b.dump_derived_indexes().expect("b"),
        a.dump_derived_indexes().expect("a")
    );
}

#[test]
fn the_page_cache_is_bounded_and_reloads_from_storage() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut config = config();
    config.page_cache_capacity = 3;
    let mut ws = Workspace::create(dir.path(), config).expect("create");
    let ids: Vec<_> = (0..6)
        .map(|i| {
            page_with(
                &mut ws,
                &format!("Trang {i}"),
                &paragraphs(&[&format!("nội dung {i}")]),
            )
        })
        .collect();
    assert!(ws.cached_page_count() <= 3);
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(page_text(&mut ws, *id), format!("nội dung {i}"));
    }
    ws.evict_pages();
    assert_eq!(ws.cached_page_count(), 0);
    assert!(!ws.is_page_cached(ids[0]));
    assert_eq!(page_text(&mut ws, ids[0]), "nội dung 0");
    assert!(ws.is_page_cached(ids[0]));
}

#[test]
fn a_failed_edit_leaves_the_stored_page_untouched() {
    let (_dir, mut ws) = new_workspace();
    let id = page_with(&mut ws, "Trang", &paragraphs(&["giữ nguyên"]));
    let outbox_before = ws.outbox_len().expect("len");
    let result = ws.edit_page(id, |doc| {
        doc.append_block(&[], &Block::paragraph("nửa chừng"))?;
        doc.insert_text(&[99], 0, "lỗi")
    });
    assert!(result.is_err());
    assert_eq!(
        page_text(&mut ws, id),
        "giữ nguyên",
        "the partial edit was discarded"
    );
    assert_eq!(ws.outbox_len().expect("len"), outbox_before);
    assert_eq!(ws.search("nua chung", 5).expect("search").len(), 0);
}

#[test]
fn tree_operations_report_errors_without_side_effects() {
    let (_dir, mut ws) = new_workspace();
    let a = ws.create_folder(None, Position::End, "A").expect("folder");
    let b = ws
        .create_folder(Some(a), Position::End, "B")
        .expect("folder");
    let queued = ws.outbox_len().expect("len");
    assert!(ws.move_node(a, Some(b), Position::End).is_err(), "cycle");
    assert!(ws.set_icon(a, Some("")).is_err());
    assert!(ws.restore(a).is_err());
    assert_eq!(ws.outbox_len().expect("len"), queued);
    assert_eq!(ws.sidebar().expect("sidebar").len(), 2);
    assert_eq!(
        ws.node(a).expect("node").expect("row").kind,
        NodeKind::Folder
    );
}

#[test]
fn default_config_values_are_sane() {
    let config = StoreConfig::default();
    assert_eq!(config.durability, ne_store::Durability::Full);
    assert!(config.compact_after_updates >= 10);
    assert!(config.page_cache_capacity >= 4);
}

#[test]
fn the_workspace_can_move_between_threads() {
    // The shell keeps the workspace in shared state behind a lock, which needs
    // it to be `Send`.
    fn assert_send<T: Send>() {}
    assert_send::<Workspace>();
}
