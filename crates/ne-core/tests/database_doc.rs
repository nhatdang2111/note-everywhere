//! The database document: schema operations, cell values, conversions and the
//! convergence of concurrent edits between replicas.

use std::collections::HashSet;

use ne_core::database::{
    CellValue, DatabaseDoc, DateValue, FilterNode, GroupConfig, Logic, Operator, PropertyType,
    SortKey, ViewDef, ViewKind,
};
use serde_json::json;

fn replica(peer: u64, from: &DatabaseDoc) -> DatabaseDoc {
    let doc = DatabaseDoc::from_snapshot(&from.export_snapshot().expect("snapshot")).expect("load");
    doc.set_peer_id(peer).expect("peer id");
    doc
}

fn base() -> DatabaseDoc {
    let doc = DatabaseDoc::new().expect("new");
    doc.set_peer_id(1).expect("peer id");
    doc
}

/// Exchanges everything both ways until neither has anything new.
fn sync(a: &DatabaseDoc, b: &DatabaseDoc) {
    let to_b = a.export_updates_since(&b.version()).expect("a to b");
    let to_a = b.export_updates_since(&a.version()).expect("b to a");
    b.import(&to_b).expect("import into b");
    a.import(&to_a).expect("import into a");
}

fn text(s: &str) -> Option<CellValue> {
    Some(CellValue::Text(s.into()))
}

fn state(doc: &DatabaseDoc) -> String {
    let props = doc.properties().expect("properties");
    let mut rows: Vec<_> = doc.all_row_values(&props).into_iter().collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    format!("{:?}\n{:?}\n{:?}", props, doc.views().expect("views"), rows)
}

#[test]
fn a_new_database_has_one_table_view_and_no_properties() {
    let doc = base();
    let schema = doc.schema().expect("schema");
    assert!(schema.properties.is_empty());
    assert_eq!(schema.views.len(), 1);
    assert_eq!(schema.views[0].kind, ViewKind::Table);
    assert!(DatabaseDoc::is_database(
        &doc.export_snapshot().expect("snapshot")
    ));
    assert!(!DatabaseDoc::is_database(&[1, 2, 3]));
    let page = ne_core::page::PageDoc::new().expect("page");
    assert!(!DatabaseDoc::is_database(
        &page.export_snapshot().expect("snapshot")
    ));
}

#[test]
fn properties_can_be_added_renamed_reordered_and_deleted() {
    let doc = base();
    let a = doc
        .add_property("Ghi chú", PropertyType::Text, None)
        .expect("a");
    let b = doc
        .add_property("Số lượng", PropertyType::Number, None)
        .expect("b");
    let c = doc
        .add_property("Xong", PropertyType::Checkbox, None)
        .expect("c");
    let ids = |d: &DatabaseDoc| -> Vec<String> {
        d.properties()
            .expect("props")
            .into_iter()
            .map(|p| p.id)
            .collect()
    };
    assert_eq!(ids(&doc), [a.clone(), b.clone(), c.clone()]);
    doc.move_property(&c, 0).expect("move");
    assert_eq!(ids(&doc), [c.clone(), a.clone(), b.clone()]);
    doc.rename_property(&a, "Mô tả").expect("rename");
    assert_eq!(doc.property(&a).expect("a").name, "Mô tả");
    doc.delete_property(&b).expect("delete");
    assert_eq!(ids(&doc), [c, a]);
    assert!(doc.rename_property(&b, "x").is_err());
    assert!(doc.add_property("  ", PropertyType::Text, None).is_err());
    assert!(doc.add_property("r", PropertyType::Relation, None).is_err());
}

#[test]
fn a_database_has_at_most_500_properties() {
    let doc = base();
    for i in 0..500 {
        doc.add_property(&format!("p{i}"), PropertyType::Text, None)
            .expect("within the limit");
    }
    assert!(
        doc.add_property("one more", PropertyType::Text, None)
            .is_err()
    );
}

#[test]
fn options_have_stable_ids_names_and_colours() {
    let doc = base();
    let p = doc
        .add_property("Trạng thái", PropertyType::Select, None)
        .expect("p");
    let todo = doc.add_option(&p, "Cần làm", Some("red")).expect("todo");
    let done = doc.add_option(&p, "Xong", None).expect("done");
    assert!(
        doc.add_option(&p, "xong", None).is_err(),
        "names are unique ignoring case"
    );
    assert!(doc.add_option(&p, "Khác", Some("mauve")).is_err());
    doc.set_cell("r1", &p, Some(CellValue::Select(todo.clone())))
        .expect("cell");
    doc.rename_option(&p, &todo, "Việc mới").expect("rename");
    assert_eq!(
        doc.cell("r1", &p).expect("cell"),
        Some(CellValue::Select(todo.clone())),
        "renaming keeps the value"
    );
    assert_eq!(
        doc.property(&p)
            .expect("p")
            .option(&todo)
            .expect("option")
            .name,
        "Việc mới"
    );
    doc.move_option(&p, &done, 0).expect("move");
    let names: Vec<_> = doc
        .property(&p)
        .expect("p")
        .options
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(names, ["Xong", "Việc mới"]);
    doc.delete_option(&p, &todo).expect("delete");
    assert_eq!(
        doc.cell("r1", &p).expect("cell"),
        None,
        "deleting clears the value"
    );
    assert!(
        doc.set_cell("r1", &p, Some(CellValue::Select(todo)))
            .is_err()
    );
}

#[test]
fn cells_are_typed_and_validated() {
    let doc = base();
    let n = doc
        .add_property("N", PropertyType::Number, None)
        .expect("n");
    let d = doc.add_property("D", PropertyType::Date, None).expect("d");
    doc.set_cell("r", &n, Some(CellValue::Number(4.5)))
        .expect("number");
    assert!(doc.set_cell("r", &n, text("x")).is_err());
    assert!(
        doc.set_cell("r", &n, Some(CellValue::Number(f64::INFINITY)))
            .is_err()
    );
    assert!(doc.set_cell("r", "title", text("x")).is_err());
    assert!(doc.set_cell("r", "missing", text("x")).is_err());
    assert!(
        doc.set_cell("a/b", &n, Some(CellValue::Number(1.0)))
            .is_err()
    );
    let mut date = DateValue::new("2026-11-21T09:30");
    date.end = Some("2026-11-22".into());
    doc.set_cell("r", &d, Some(CellValue::Date(date.clone())))
        .expect("date");
    assert!(
        doc.set_cell("r", &d, Some(CellValue::Date(DateValue::new("21/11/2026"))))
            .is_err()
    );
    assert_eq!(
        doc.cell("r", &d).expect("date"),
        Some(CellValue::Date(date))
    );
    doc.set_cell("r", &n, None).expect("clear");
    assert_eq!(doc.cell("r", &n).expect("cleared"), None);
}

#[test]
fn writing_the_stored_value_again_makes_no_update() {
    let doc = base();
    let t = doc.add_property("T", PropertyType::Text, None).expect("t");
    doc.set_cell("r", &t, text("a")).expect("first");
    let ((), update) = doc.edit(|d| d.set_cell("r", &t, text("a"))).expect("edit");
    let ((), clear_absent) = doc.edit(|d| d.set_cell("other", &t, None)).expect("edit");
    let probe = base();
    // An update without operations imports as a no-op on a fresh replica.
    assert!(probe.import(&update).is_ok());
    assert!(probe.import(&clear_absent).is_ok());
    assert!(
        update.len() < 80 && clear_absent.len() < 80,
        "{} {}",
        update.len(),
        clear_absent.len()
    );
}

#[test]
fn changing_a_type_converts_on_read_and_changing_back_restores() {
    let doc = base();
    let p = doc.add_property("P", PropertyType::Text, None).expect("p");
    doc.set_cell("a", &p, text("12,5")).expect("a");
    doc.set_cell("b", &p, text("not a number")).expect("b");
    doc.change_property_type(&p, PropertyType::Number, None)
        .expect("to number");
    assert_eq!(doc.cell("a", &p).expect("a"), Some(CellValue::Number(12.5)));
    assert_eq!(
        doc.cell("b", &p).expect("b"),
        None,
        "unconvertible reads empty"
    );
    doc.set_cell("c", &p, Some(CellValue::Number(7.0)))
        .expect("c");
    doc.change_property_type(&p, PropertyType::Text, None)
        .expect("back to text");
    assert_eq!(
        doc.cell("a", &p).expect("a"),
        text("12,5"),
        "the original text is back"
    );
    assert_eq!(doc.cell("b", &p).expect("b"), text("not a number"));
    assert_eq!(doc.cell("c", &p).expect("c"), text("7"));
}

#[test]
fn text_to_select_creates_the_options_its_values_name() {
    let doc = base();
    let p = doc.add_property("P", PropertyType::Text, None).expect("p");
    for (row, value) in [("1", "Xong"), ("2", "Đang làm"), ("3", "xong"), ("4", "")] {
        doc.set_cell(row, &p, text(value)).expect("cell");
    }
    doc.change_property_type(&p, PropertyType::Select, None)
        .expect("select");
    let prop = doc.property(&p).expect("p");
    let names: Vec<&str> = prop.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let first = doc.cell("1", &p).expect("1");
    assert_eq!(
        first,
        doc.cell("3", &p).expect("3"),
        "same option ignoring case"
    );
    assert!(matches!(first, Some(CellValue::Select(_))));
    assert_eq!(doc.cell("4", &p).expect("4"), None);
    // Select to multi-select keeps the single value as a one-element list.
    doc.change_property_type(&p, PropertyType::MultiSelect, None)
        .expect("multi");
    assert!(
        matches!(doc.cell("2", &p).expect("2"), Some(CellValue::MultiSelect(v)) if v.len() == 1)
    );
}

#[test]
fn a_backlink_property_is_never_written_directly() {
    use ne_core::database::RelationConfig;
    let doc = base();
    let p = doc
        .add_property(
            "Backlink",
            PropertyType::Relation,
            Some(RelationConfig {
                database: "1@2".into(),
                reverse_property: None,
                backlink_of: Some("forward".into()),
            }),
        )
        .expect("backlink");
    assert!(
        doc.set_cell("r", &p, Some(CellValue::Relation(vec!["1@3".into()])))
            .is_err()
    );
    assert!(
        doc.change_property_type(&p, PropertyType::Text, None)
            .is_err()
    );
}

#[test]
fn concurrent_edits_to_different_cells_both_survive() {
    let a = base();
    let t = a.add_property("T", PropertyType::Text, None).expect("t");
    let n = a.add_property("N", PropertyType::Number, None).expect("n");
    let b = replica(2, &a);
    a.set_cell("row", &t, text("from a")).expect("a");
    b.set_cell("row", &n, Some(CellValue::Number(9.0)))
        .expect("b");
    sync(&a, &b);
    for doc in [&a, &b] {
        assert_eq!(doc.cell("row", &t).expect("t"), text("from a"));
        assert_eq!(
            doc.cell("row", &n).expect("n"),
            Some(CellValue::Number(9.0))
        );
    }
    assert_eq!(state(&a), state(&b));
}

#[test]
fn concurrent_edits_to_one_cell_converge_to_one_value() {
    let a = base();
    let t = a.add_property("T", PropertyType::Text, None).expect("t");
    let b = replica(2, &a);
    a.set_cell("row", &t, text("a")).expect("a");
    b.set_cell("row", &t, text("b")).expect("b");
    sync(&a, &b);
    let winner = a.cell("row", &t).expect("a");
    assert!(winner == text("a") || winner == text("b"));
    assert_eq!(winner, b.cell("row", &t).expect("b"));
}

#[test]
fn a_schema_change_racing_a_cell_edit_converges_without_corruption() {
    let a = base();
    let p = a.add_property("P", PropertyType::Text, None).expect("p");
    a.set_cell("row", &p, text("12")).expect("seed");
    let b = replica(2, &a);
    // a turns the property into a number while b rewrites the cell as text.
    a.change_property_type(&p, PropertyType::Number, None)
        .expect("type");
    b.set_cell("row", &p, text("34")).expect("cell");
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    assert_eq!(
        a.property(&p).expect("p").ty,
        PropertyType::Number,
        "the schema wins"
    );
    assert_eq!(
        a.cell("row", &p).expect("cell"),
        Some(CellValue::Number(34.0)),
        "the late text edit is read through the new type"
    );
}

#[test]
fn deleting_a_property_while_another_device_writes_to_it() {
    let a = base();
    let p = a.add_property("P", PropertyType::Text, None).expect("p");
    let keep = a.add_property("K", PropertyType::Text, None).expect("keep");
    let b = replica(2, &a);
    a.delete_property(&p).expect("delete");
    b.set_cell("row", &p, text("late write")).expect("write");
    b.set_cell("row", &keep, text("kept")).expect("write keep");
    b.rename_property(&p, "Renamed").expect("rename");
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    for doc in [&a, &b] {
        let props = doc.properties().expect("props");
        assert_eq!(props.len(), 1, "the delete wins");
        assert!(doc.property(&p).is_err());
        let values = doc.all_row_values(&props);
        assert_eq!(values["row"].len(), 1);
        assert_eq!(doc.cell("row", &keep).expect("keep"), text("kept"));
    }
    let live: HashSet<String> = ["row".to_owned()].into();
    assert_eq!(
        a.purge_orphans(&live).expect("purge"),
        1,
        "the stray cell is collected"
    );
}

#[test]
fn deleting_an_option_while_another_device_picks_it() {
    let a = base();
    let p = a.add_property("S", PropertyType::Select, None).expect("p");
    let o = a.add_option(&p, "One", None).expect("o");
    let other = a.add_option(&p, "Two", None).expect("other");
    let b = replica(2, &a);
    a.delete_option(&p, &o).expect("delete");
    b.set_cell("row", &p, Some(CellValue::Select(o)))
        .expect("pick");
    b.set_cell("row2", &p, Some(CellValue::Select(other.clone())))
        .expect("pick other");
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    assert_eq!(a.cell("row", &p).expect("row"), None);
    assert_eq!(
        a.cell("row2", &p).expect("row2"),
        Some(CellValue::Select(other))
    );
}

#[test]
fn concurrent_property_moves_converge() {
    let a = base();
    let ids: Vec<String> = (0..4)
        .map(|i| {
            a.add_property(&format!("p{i}"), PropertyType::Text, None)
                .expect("p")
        })
        .collect();
    let b = replica(2, &a);
    a.move_property(&ids[3], 0).expect("a");
    b.move_property(&ids[0], 3).expect("b");
    b.move_property(&ids[3], 1).expect("b again");
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    let order: Vec<_> = a
        .properties()
        .expect("props")
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(order.len(), 4);
}

#[test]
fn concurrent_option_creation_keeps_both_options() {
    let a = base();
    let p = a
        .add_property("S", PropertyType::MultiSelect, None)
        .expect("p");
    let b = replica(2, &a);
    a.add_option(&p, "From a", None).expect("a");
    b.add_option(&p, "From b", None).expect("b");
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    assert_eq!(a.property(&p).expect("p").options.len(), 2);
}

fn sample_view(doc: &DatabaseDoc, id: &str, p: &str) -> ViewDef {
    let mut view = doc.view(id).expect("view");
    view.filter = Some(FilterNode::Group {
        logic: Logic::And,
        children: vec![FilterNode::Condition {
            property: p.into(),
            operator: Operator::Contains,
            value: Some(json!("abc")),
        }],
    });
    view.sorts = vec![SortKey {
        property: "title".into(),
        descending: true,
    }];
    view
}

#[test]
fn views_keep_their_own_configuration() {
    let doc = base();
    let t = doc.add_property("T", PropertyType::Text, None).expect("t");
    let s = doc
        .add_property("S", PropertyType::Select, None)
        .expect("s");
    let table = doc.views().expect("views")[0].id.clone();
    let board = doc.add_view("Board", ViewKind::Board).expect("board");
    let mut def = sample_view(&doc, &table, &t);
    doc.save_view(&def).expect("save");
    let mut board_def = doc.view(&board).expect("board");
    board_def.group = Some(GroupConfig {
        property: s.clone(),
        hide_empty: true,
    });
    doc.save_view(&board_def).expect("save board");
    assert_eq!(doc.view(&table).expect("table"), def);
    assert!(
        doc.view(&board).expect("board").filter.is_none(),
        "views do not share filters"
    );
    def.filter = None;
    doc.save_view(&def).expect("clear filter");
    assert!(doc.view(&table).expect("table").filter.is_none());
    let mut bad = board_def.clone();
    bad.group = Some(GroupConfig {
        property: t,
        hide_empty: false,
    });
    assert!(
        doc.save_view(&bad).is_err(),
        "group needs a choice property"
    );
    doc.move_view(&board, 0).expect("move");
    assert_eq!(doc.views().expect("views")[0].id, board);
    doc.delete_view(&table).expect("delete");
    assert!(doc.delete_view(&board).is_err(), "the last view stays");
}

#[test]
fn concurrent_edits_of_different_view_fields_both_survive() {
    let a = base();
    let t = a.add_property("T", PropertyType::Text, None).expect("t");
    let view = a.views().expect("views")[0].id.clone();
    let b = replica(2, &a);
    let mut on_a = a.view(&view).expect("view");
    on_a.filter = sample_view(&a, &view, &t).filter;
    a.save_view(&on_a).expect("filter");
    let mut on_b = b.view(&view).expect("view");
    on_b.name = "Renamed".into();
    on_b.sorts = vec![SortKey {
        property: t,
        descending: false,
    }];
    b.save_view(&on_b).expect("name and sorts");
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    let merged = a.view(&view).expect("view");
    assert!(merged.filter.is_some());
    assert_eq!(merged.name, "Renamed");
    assert_eq!(merged.sorts.len(), 1);
}

#[test]
fn changes_report_the_rows_and_areas_that_moved() {
    let a = base();
    let t = a.add_property("T", PropertyType::Text, None).expect("t");
    let b = replica(2, &a);
    a.set_cell("r1", &t, text("x")).expect("a");
    a.set_cell("r2", &t, text("y")).expect("a");
    let update = a.export_updates_since(&b.version()).expect("update");
    b.take_changes();
    b.import(&update).expect("import");
    let changes = b.take_changes();
    assert_eq!(changes.rows, ["r1".to_owned(), "r2".to_owned()].into());
    assert!(!changes.schema);
    a.rename_property(&t, "Renamed").expect("rename");
    let update = a.export_updates_since(&b.version()).expect("update");
    b.import(&update).expect("import");
    assert!(b.take_changes().schema);
    assert!(b.take_changes().is_empty());
}

mod random {
    use super::*;
    use proptest::prelude::*;

    #[derive(Debug, Clone)]
    enum Op {
        SetCell { row: u8, prop: u8, text: u8 },
        ClearCell { row: u8, prop: u8 },
        Rename { prop: u8, name: u8 },
        Retype { prop: u8, number: bool },
        Delete { prop: u8 },
        Move { prop: u8, to: u8 },
        AddProperty,
        AddOption { prop: u8, name: u8 },
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0..3u8, 0..4u8, 0..5u8).prop_map(|(row, prop, text)| Op::SetCell { row, prop, text }),
            (0..3u8, 0..4u8).prop_map(|(row, prop)| Op::ClearCell { row, prop }),
            (0..4u8, 0..5u8).prop_map(|(prop, name)| Op::Rename { prop, name }),
            (0..4u8, any::<bool>()).prop_map(|(prop, number)| Op::Retype { prop, number }),
            (0..4u8).prop_map(|prop| Op::Delete { prop }),
            (0..4u8, 0..4u8).prop_map(|(prop, to)| Op::Move { prop, to }),
            Just(Op::AddProperty),
            (0..4u8, 0..5u8).prop_map(|(prop, name)| Op::AddOption { prop, name }),
        ]
    }

    fn apply(doc: &DatabaseDoc, op: &Op) {
        let props = doc.properties().expect("props");
        let pick = |i: u8| {
            props
                .get(usize::from(i) % props.len().max(1))
                .map(|p| p.id.clone())
        };
        // Operations that do not apply (deleted property, bad value) are
        // skipped; what matters is that whatever happens converges.
        let _ = match op {
            Op::SetCell { row, prop, text } => pick(*prop).map(|p| {
                let value = match props.iter().find(|x| x.id == p).map(|x| x.ty) {
                    Some(PropertyType::Number) => CellValue::Number(f64::from(*text)),
                    _ => CellValue::Text(format!("v{text}")),
                };
                doc.set_cell(&format!("r{row}"), &p, Some(value))
            }),
            Op::ClearCell { row, prop } => {
                pick(*prop).map(|p| doc.set_cell(&format!("r{row}"), &p, None))
            }
            Op::Rename { prop, name } => {
                pick(*prop).map(|p| doc.rename_property(&p, &format!("n{name}")))
            }
            Op::Retype { prop, number } => pick(*prop).map(|p| {
                let ty = if *number {
                    PropertyType::Number
                } else {
                    PropertyType::Text
                };
                doc.change_property_type(&p, ty, None)
            }),
            Op::Delete { prop } => pick(*prop).map(|p| doc.delete_property(&p)),
            Op::Move { prop, to } => pick(*prop).map(|p| doc.move_property(&p, usize::from(*to))),
            Op::AddProperty => Some(doc.add_property("new", PropertyType::Text, None).map(drop)),
            Op::AddOption { prop, name } => {
                pick(*prop).map(|p| doc.add_option(&p, &format!("o{name}"), None).map(drop))
            }
        };
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn replicas_converge_after_any_interleaving(
            left in prop::collection::vec(op(), 0..14),
            right in prop::collection::vec(op(), 0..14),
        ) {
            let a = base();
            for _ in 0..3 {
                a.add_property("seed", PropertyType::Text, None).expect("seed");
            }
            let b = replica(2, &a);
            for op in &left { apply(&a, op); }
            for op in &right { apply(&b, op); }
            sync(&a, &b);
            prop_assert_eq!(state(&a), state(&b));
            // A reload from a snapshot reads the same state.
            let reloaded = DatabaseDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("load");
            prop_assert_eq!(state(&a), state(&reloaded));
        }
    }
}
