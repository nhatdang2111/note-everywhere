//! Databases across devices: documents travel through the generic document
//! path, and concurrent edits converge.

mod store_support;

use std::collections::BTreeMap;

use ne_core::NodeId;
use ne_core::database::{CellValue, GroupConfig, PropertyType, QuerySpec};
use ne_core::tree::Position;
use ne_store::{DocRef, Workspace};
use store_support::*;

struct Pair {
    a: Workspace,
    b: Workspace,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    db: NodeId,
    text: String,
    number: String,
    status: String,
    rows: Vec<NodeId>,
}

/// Two devices that both hold a database of four rows.
fn pair() -> Pair {
    let (dir_a, mut a) = new_workspace();
    let (dir_b, mut b) = new_workspace();
    let db = a.create_database(None, Position::End, "DB").expect("db");
    let (text, _) = a
        .database_edit(db, |d| d.add_property("Text", PropertyType::Text, None))
        .expect("text");
    let (number, _) = a
        .database_edit(db, |d| d.add_property("Number", PropertyType::Number, None))
        .expect("number");
    let (status, _) = a
        .database_edit(db, |d| d.add_property("Status", PropertyType::Select, None))
        .expect("status");
    for name in ["One", "Two"] {
        a.database_edit(db, |d| d.add_option(&status, name, None))
            .expect("option");
    }
    let rows: Vec<NodeId> = (1..=4)
        .map(|i| {
            a.database_create_row(db, &format!("Row {i}"), Position::End, &BTreeMap::new())
                .expect("row")
        })
        .collect();
    deliver(&mut a, &mut b);
    Pair {
        a,
        b,
        _dirs: (dir_a, dir_b),
        db,
        text,
        number,
        status,
        rows,
    }
}

fn titles(ws: &mut Workspace, db: NodeId, spec: &QuerySpec) -> Vec<String> {
    ws.database_query(db, spec)
        .expect("query")
        .rows
        .into_iter()
        .map(|r| r.title)
        .collect()
}

fn exchange(p: &mut Pair) {
    deliver(&mut p.a, &mut p.b);
    deliver(&mut p.b, &mut p.a);
}

fn same_everything(p: &mut Pair) {
    let spec = QuerySpec::default();
    let a = p.a.database_query(p.db, &spec).expect("a");
    let b = p.b.database_query(p.db, &spec).expect("b");
    assert_eq!(a, b);
    assert_eq!(
        p.a.database_schema(p.db).expect("a"),
        p.b.database_schema(p.db).expect("b")
    );
    // The derived index is equal to one rebuilt from the documents on both.
    let incremental_a = p.a.dump_derived_indexes().expect("dump");
    let incremental_b = p.b.dump_derived_indexes().expect("dump");
    p.a.rebuild_indexes().expect("rebuild");
    p.b.rebuild_indexes().expect("rebuild");
    assert_eq!(incremental_a, p.a.dump_derived_indexes().expect("dump"));
    assert_eq!(incremental_b, p.b.dump_derived_indexes().expect("dump"));
}

#[test]
fn a_database_arrives_on_another_device_with_rows_and_a_working_index() {
    let mut p = pair();
    assert_eq!(
        p.b.node(p.db).expect("node").expect("db").kind,
        ne_core::tree::NodeKind::Database
    );
    let result =
        p.b.database_query(p.db, &QuerySpec::default())
            .expect("query");
    assert_eq!(result.total, 4);
    assert_eq!(
        p.b.database_schema(p.db).expect("schema").properties.len(),
        3
    );
    same_everything(&mut p);
}

#[test]
fn edits_of_different_cells_both_survive() {
    let mut p = pair();
    let (r0, r1) = (p.rows[0], p.rows[1]);
    p.a.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("from a".into())))
        .expect("a");
    p.b.database_set_cell(p.db, r0, &p.number, Some(CellValue::Number(5.0)))
        .expect("b same row");
    p.b.database_set_cell(p.db, r1, &p.text, Some(CellValue::Text("from b".into())))
        .expect("b other row");
    exchange(&mut p);
    let result =
        p.a.database_query(p.db, &QuerySpec::default())
            .expect("query");
    assert_eq!(
        result.rows[0].values[&p.text],
        CellValue::Text("from a".into())
    );
    assert_eq!(result.rows[0].values[&p.number], CellValue::Number(5.0));
    assert_eq!(
        result.rows[1].values[&p.text],
        CellValue::Text("from b".into())
    );
    same_everything(&mut p);
}

#[test]
fn the_same_cell_edited_on_two_devices_converges_to_one_value() {
    let mut p = pair();
    let r0 = p.rows[0];
    p.a.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("a".into())))
        .expect("a");
    p.b.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("b".into())))
        .expect("b");
    exchange(&mut p);
    let a =
        p.a.database_query(p.db, &QuerySpec::default())
            .expect("a")
            .rows[0]
            .values[&p.text]
            .clone();
    let b =
        p.b.database_query(p.db, &QuerySpec::default())
            .expect("b")
            .rows[0]
            .values[&p.text]
            .clone();
    assert_eq!(a, b);
    assert!(a == CellValue::Text("a".into()) || a == CellValue::Text("b".into()));
    same_everything(&mut p);
}

#[test]
fn a_schema_change_racing_a_cell_edit_converges_and_the_schema_wins() {
    let mut p = pair();
    let r0 = p.rows[0];
    p.a.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("12".into())))
        .expect("seed");
    exchange(&mut p);
    p.a.database_edit(p.db, |d| {
        d.change_property_type(&p.text, PropertyType::Number, None)
    })
    .expect("retype");
    p.b.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("34".into())))
        .expect("edit");
    exchange(&mut p);
    same_everything(&mut p);
    let result =
        p.a.database_query(p.db, &QuerySpec::default())
            .expect("query");
    assert_eq!(result.rows[0].values[&p.text], CellValue::Number(34.0));
    assert_eq!(
        p.b.database_schema(p.db)
            .expect("schema")
            .property(&p.text)
            .expect("property")
            .ty,
        PropertyType::Number
    );
}

#[test]
fn deleting_a_property_while_another_device_writes_to_it() {
    let mut p = pair();
    let r0 = p.rows[0];
    p.a.database_delete_property(p.db, &p.number)
        .expect("delete");
    p.b.database_set_cell(p.db, r0, &p.number, Some(CellValue::Number(1.0)))
        .expect("write");
    p.b.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("kept".into())))
        .expect("write other");
    exchange(&mut p);
    same_everything(&mut p);
    for ws in [&mut p.a, &mut p.b] {
        let schema = ws.database_schema(p.db).expect("schema");
        assert!(schema.property(&p.number).is_none(), "the delete wins");
        let result = ws
            .database_query(p.db, &QuerySpec::default())
            .expect("query");
        assert_eq!(result.rows[0].values.len(), 1);
        assert_eq!(
            result.rows[0].values[&p.text],
            CellValue::Text("kept".into())
        );
    }
}

#[test]
fn trashing_a_row_on_one_device_while_the_other_edits_it() {
    let mut p = pair();
    let r0 = p.rows[0];
    p.a.trash_node(r0).expect("trash");
    p.b.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("edited".into())))
        .expect("edit");
    exchange(&mut p);
    for ws in [&mut p.a, &mut p.b] {
        assert_eq!(
            ws.database_query(p.db, &QuerySpec::default())
                .expect("query")
                .total,
            3,
            "hidden on both"
        );
    }
    p.b.restore(r0).expect("restore");
    exchange(&mut p);
    for ws in [&mut p.a, &mut p.b] {
        let result = ws
            .database_query(p.db, &QuerySpec::default())
            .expect("query");
        assert_eq!(result.total, 4);
        assert_eq!(
            result.rows[0].values[&p.text],
            CellValue::Text("edited".into()),
            "the edit was kept"
        );
    }
    same_everything(&mut p);
}

#[test]
fn concurrent_reordering_of_cards_converges() {
    let mut p = pair();
    let (one, two, three, four) = (p.rows[0], p.rows[1], p.rows[2], p.rows[3]);
    let option =
        p.a.database_schema(p.db)
            .expect("schema")
            .property(&p.status)
            .expect("p")
            .options[0]
            .id
            .clone();
    // Different cards moved on each device.
    p.a.database_move_card(p.db, one, &p.status, Some(&option), None)
        .expect("a moves card one to the end");
    p.b.database_move_card(p.db, four, &p.status, Some(&option), Some(two))
        .expect("b moves card four up");
    exchange(&mut p);
    let order_a = titles(&mut p.a, p.db, &QuerySpec::default());
    let order_b = titles(&mut p.b, p.db, &QuerySpec::default());
    assert_eq!(order_a, order_b);
    assert_eq!(order_a.len(), 4);
    // The same card moved to different places: one place wins everywhere.
    p.a.database_move_card(p.db, three, &p.status, Some(&option), Some(one))
        .expect("a");
    p.b.database_move_card(p.db, three, &p.status, Some(&option), None)
        .expect("b");
    exchange(&mut p);
    assert_eq!(
        titles(&mut p.a, p.db, &QuerySpec::default()),
        titles(&mut p.b, p.db, &QuerySpec::default())
    );
    // Grouped, both devices see the same columns.
    let grouped = QuerySpec {
        group: Some(GroupConfig {
            property: p.status.clone(),
            hide_empty: false,
        }),
        ..QuerySpec::default()
    };
    assert_eq!(
        p.a.database_query(p.db, &grouped).expect("a"),
        p.b.database_query(p.db, &grouped).expect("b")
    );
    same_everything(&mut p);
}

#[test]
fn option_and_property_edits_on_both_devices_merge() {
    let mut p = pair();
    p.a.database_edit(p.db, |d| d.add_option(&p.status, "From a", None))
        .expect("a");
    p.b.database_edit(p.db, |d| d.add_option(&p.status, "From b", None))
        .expect("b");
    p.a.database_edit(p.db, |d| {
        d.add_property("Only a", PropertyType::Checkbox, None)
    })
    .expect("a");
    p.b.database_edit(p.db, |d| d.rename_property(&p.text, "Renamed"))
        .expect("b");
    exchange(&mut p);
    same_everything(&mut p);
    let schema = p.a.database_schema(p.db).expect("schema");
    assert_eq!(schema.property(&p.status).expect("status").options.len(), 4);
    assert_eq!(schema.properties.len(), 4);
    assert_eq!(schema.property(&p.text).expect("text").name, "Renamed");
}

#[test]
fn the_state_is_the_same_after_restarting_both_devices() {
    let mut p = pair();
    let r0 = p.rows[0];
    p.a.database_set_cell(p.db, r0, &p.text, Some(CellValue::Text("x".into())))
        .expect("a");
    p.b.database_set_cell(p.db, p.rows[1], &p.number, Some(CellValue::Number(2.0)))
        .expect("b");
    exchange(&mut p);
    let before =
        p.a.database_query(p.db, &QuerySpec::default())
            .expect("query");
    let (dir_a, dir_b) = (
        p._dirs.0.path().to_path_buf(),
        p._dirs.1.path().to_path_buf(),
    );
    drop(p.a);
    drop(p.b);
    let mut a = reopen(&dir_a);
    let mut b = reopen(&dir_b);
    assert_eq!(
        a.database_query(p.db, &QuerySpec::default()).expect("a"),
        before
    );
    assert_eq!(
        b.database_query(p.db, &QuerySpec::default()).expect("b"),
        before
    );
}

#[test]
fn seeding_a_server_queues_databases_through_the_explicit_document_list() {
    let mut p = pair();
    // Everything was delivered; simulate "the outbox is dropped and the server
    // is seeded from this device" the way a first sign-in does.
    loop {
        let queued = p.a.outbox_peek(100).expect("peek");
        if queued.is_empty() {
            break;
        }
        let ids: Vec<String> = queued.into_iter().map(|i| i.client_id).collect();
        p.a.outbox_acknowledge(&ids).expect("ack");
    }
    assert_eq!(p.a.outbox_len().expect("len"), 0);
    // Moving the cursor never queues anything by itself.
    p.a.set_sync_cursor(&0u64.to_be_bytes()).expect("cursor");
    assert_eq!(p.a.outbox_len().expect("len"), 0);
    // The caller queues every document that is not a page of the tree.
    for doc in p.a.synced_documents().expect("docs") {
        let DocRef::Page(id) = doc else { continue };
        let snapshot = p.a.document_snapshot(id).expect("snapshot");
        p.a.outbox_enqueue(&DocRef::Page(id), &snapshot)
            .expect("enqueue");
    }
    let queued = p.a.outbox_peek(100).expect("peek");
    assert!(
        queued.iter().any(|i| i.doc == DocRef::Page(p.db)),
        "the database document is queued in full"
    );
    // A fresh device that receives the tree and the queued documents has the
    // database.
    let (_dir, mut fresh) = new_workspace();
    let tree = p.a.tree().export_snapshot().expect("tree");
    fresh
        .apply_remote_update(&DocRef::Tree, &tree)
        .expect("tree");
    for item in &queued {
        fresh
            .apply_remote_update(&item.doc, &item.bytes)
            .expect("apply");
    }
    let result = fresh
        .database_query(p.db, &QuerySpec::default())
        .expect("query");
    assert_eq!(result.total, 4);
}

#[test]
fn a_database_whose_document_has_not_arrived_is_not_found_instead_of_empty() {
    let p = pair();
    let (_dir, mut fresh) = new_workspace();
    let tree = p.a.tree().export_snapshot().expect("tree");
    fresh
        .apply_remote_update(&DocRef::Tree, &tree)
        .expect("tree");
    assert!(fresh.database_schema(p.db).is_err());
    assert!(fresh.database_query(p.db, &QuerySpec::default()).is_err());
}
