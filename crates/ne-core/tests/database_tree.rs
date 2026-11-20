//! Databases as nodes of the workspace tree.

use std::sync::Arc;

use ne_core::NodeId;
use ne_core::tree::{NodeKind, Position, SystemClock, WorkspaceTree};

fn tree() -> WorkspaceTree {
    WorkspaceTree::with_peer_id(1, Arc::new(SystemClock)).expect("tree")
}

#[test]
fn a_database_holds_pages_as_rows_and_nothing_else() {
    let t = tree();
    let db = t.create_database(None, Position::End, "Tasks").expect("db");
    assert_eq!(t.get(db).expect("db").kind, NodeKind::Database);
    let row = t.create_page(Some(db), Position::End, "Row").expect("row");
    assert_eq!(t.get(row).expect("row").parent, Some(db));
    assert!(t.create_folder(Some(db), Position::End, "no").is_err());
    assert!(t.create_database(Some(db), Position::End, "no").is_err());
    // Rows can have child pages of their own (children of the row page).
    t.create_page(Some(row), Position::End, "Sub")
        .expect("sub page");
    // A folder or database cannot be moved in; a page can.
    let folder = t.create_folder(None, Position::End, "F").expect("folder");
    let other = t
        .create_database(None, Position::End, "Other")
        .expect("other");
    let page = t.create_page(None, Position::End, "P").expect("page");
    assert!(t.move_node(folder, Some(db), Position::End).is_err());
    assert!(t.move_node(other, Some(db), Position::End).is_err());
    t.move_node(page, Some(db), Position::End)
        .expect("page becomes a row");
    assert_eq!(t.children(Some(db), false).expect("rows").len(), 2);
    // A database can sit in a folder or under a page.
    t.move_node(db, Some(folder), Position::End)
        .expect("move db");
}

#[test]
fn trashing_a_database_hides_its_rows_and_restoring_brings_them_back() {
    let t = tree();
    let db = t.create_database(None, Position::End, "Tasks").expect("db");
    let row = t.create_page(Some(db), Position::End, "Row").expect("row");
    t.trash_node(db).expect("trash");
    assert!(t.snapshot().expect("snapshot").is_empty());
    assert!(t.get(row).is_ok(), "the row still exists, hidden");
    t.restore(db).expect("restore");
    assert_eq!(t.snapshot().expect("snapshot").len(), 2);
    assert!(
        t.delete_permanently(db).is_err(),
        "only trashed nodes can be deleted"
    );
}

#[test]
fn the_kind_survives_a_snapshot_round_trip() {
    let t = tree();
    let db = t.create_database(None, Position::End, "Tasks").expect("db");
    let loaded = WorkspaceTree::from_snapshot(
        &t.export_snapshot().expect("snapshot"),
        Arc::new(SystemClock),
    )
    .expect("load");
    assert_eq!(loaded.get(db).expect("db").kind, NodeKind::Database);
}

#[test]
fn well_known_and_fresh_document_ids_have_the_shape_of_node_ids() {
    let library = NodeId::library();
    let reminders = NodeId::reminders();
    assert_ne!(library, reminders);
    assert_eq!(
        library.to_string().parse::<NodeId>().expect("parse"),
        library
    );
    let fresh = NodeId::fresh_document();
    assert_eq!(fresh.to_string().parse::<NodeId>().expect("parse"), fresh);
    assert_ne!(fresh, NodeId::fresh_document());
    let t = tree();
    let node = t.create_page(None, Position::End, "p").expect("page");
    assert_ne!(node, library);
    assert_ne!(node, fresh);
    assert!(!t.contains(library) && !t.contains(fresh));
}
