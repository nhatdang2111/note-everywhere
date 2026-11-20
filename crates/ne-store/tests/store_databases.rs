//! Databases in the store: rows, cells, views queries, the cell index and its
//! incremental maintenance, relations, cards and search.

mod store_support;

use std::collections::BTreeMap;

use ne_core::NodeId;
use ne_core::database::{
    CellValue, ColumnConfig, DateRange, DateValue, FilterNode, GroupConfig, Logic, Operator,
    PropertyType, QueryResult, QuerySpec, SortKey, ViewKind,
};
use ne_core::tree::Position;
use ne_store::Workspace;
use serde_json::json;
use store_support::*;

struct Fixture {
    ws: Workspace,
    _dir: tempfile::TempDir,
    db: NodeId,
    name: String,
    amount: String,
    status: String,
    tags: String,
    due: String,
    done: String,
}

fn fixture() -> Fixture {
    let (dir, mut ws) = new_workspace();
    let db = ws
        .create_database(None, Position::End, "Công việc")
        .expect("db");
    let add = |ws: &mut Workspace, name: &str, ty| {
        ws.database_edit(db, |d| d.add_property(name, ty, None))
            .expect("property")
            .0
    };
    let name = add(&mut ws, "Ghi chú", PropertyType::Text);
    let amount = add(&mut ws, "Số lượng", PropertyType::Number);
    let status = add(&mut ws, "Trạng thái", PropertyType::Select);
    let tags = add(&mut ws, "Nhãn", PropertyType::MultiSelect);
    let due = add(&mut ws, "Hạn", PropertyType::Date);
    let done = add(&mut ws, "Xong", PropertyType::Checkbox);
    for option in ["Cần làm", "Đang làm", "Xong"] {
        ws.database_edit(db, |d| d.add_option(&status, option, None))
            .expect("option");
    }
    for option in ["a", "b", "c"] {
        ws.database_edit(db, |d| d.add_option(&tags, option, None))
            .expect("option");
    }
    Fixture {
        ws,
        _dir: dir,
        db,
        name,
        amount,
        status,
        tags,
        due,
        done,
    }
}

impl Fixture {
    fn option(&mut self, property: &str, name: &str) -> String {
        self.ws
            .database_schema(self.db)
            .expect("schema")
            .property(property)
            .expect("property")
            .options
            .iter()
            .find(|o| o.name == name)
            .expect("option")
            .id
            .clone()
    }

    fn row(&mut self, title: &str) -> NodeId {
        self.ws
            .database_create_row(self.db, title, Position::End, &BTreeMap::new())
            .expect("row")
    }

    fn set(&mut self, row: NodeId, property: &str, value: Option<CellValue>) {
        self.ws
            .database_set_cell(self.db, row, property, value)
            .expect("set cell");
    }

    fn query(&mut self, spec: &QuerySpec) -> QueryResult {
        self.ws.database_query(self.db, spec).expect("query")
    }

    fn titles(&mut self, spec: &QuerySpec) -> Vec<String> {
        self.query(spec).rows.into_iter().map(|r| r.title).collect()
    }
}

fn cond(property: &str, operator: Operator, value: Option<serde_json::Value>) -> FilterNode {
    FilterNode::Condition {
        property: property.into(),
        operator,
        value,
    }
}

fn filter(node: FilterNode) -> QuerySpec {
    QuerySpec {
        filter: Some(node),
        ..QuerySpec::default()
    }
}

fn sorted_by(keys: &[(&str, bool)]) -> QuerySpec {
    QuerySpec {
        sorts: keys
            .iter()
            .map(|(p, d)| SortKey {
                property: (*p).into(),
                descending: *d,
            })
            .collect(),
        ..QuerySpec::default()
    }
}

/// Five rows with a bit of everything.
fn populated() -> Fixture {
    let mut f = fixture();
    let (todo, doing, done) = (
        f.option(&f.status.clone(), "Cần làm"),
        f.option(&f.status.clone(), "Đang làm"),
        f.option(&f.status.clone(), "Xong"),
    );
    let (a, b) = (
        f.option(&f.tags.clone(), "a"),
        f.option(&f.tags.clone(), "b"),
    );
    let rows = [
        (
            "Đào",
            "Tìm kiếm đường đi",
            Some(3.0),
            Some(&todo),
            vec![&a],
            Some("2026-11-20"),
            true,
        ),
        (
            "Anh",
            "ghi chú thứ hai",
            Some(10.0),
            Some(&doing),
            vec![&a, &b],
            Some("2026-11-22T09:00"),
            false,
        ),
        ("Ân", "", Some(-1.5), Some(&done), vec![&b], None, false),
        (
            "Bình",
            "Ghi chu khac",
            None,
            None,
            vec![],
            Some("2026-11-21"),
            true,
        ),
        ("Đức", "zzz", Some(3.0), Some(&todo), vec![], None, false),
    ];
    let (name, amount, status, tags, due, checkbox) = (
        f.name.clone(),
        f.amount.clone(),
        f.status.clone(),
        f.tags.clone(),
        f.due.clone(),
        f.done.clone(),
    );
    for (title, note, number, option, labels, date, ticked) in rows {
        let row = f.row(title);
        if !note.is_empty() {
            f.set(row, &name, Some(CellValue::Text(note.into())));
        }
        if let Some(n) = number {
            f.set(row, &amount, Some(CellValue::Number(n)));
        }
        if let Some(o) = option {
            f.set(row, &status, Some(CellValue::Select(o.clone())));
        }
        if !labels.is_empty() {
            f.set(
                row,
                &tags,
                Some(CellValue::MultiSelect(
                    labels.into_iter().cloned().collect(),
                )),
            );
        }
        if let Some(d) = date {
            f.set(row, &due, Some(CellValue::Date(DateValue::new(d))));
        }
        if ticked {
            f.set(row, &checkbox, Some(CellValue::Checkbox(true)));
        }
    }
    f
}

#[test]
fn rows_are_pages_with_values_a_query_returns() {
    let mut f = populated();
    let result = f.query(&QuerySpec::default());
    assert_eq!(result.total, 5);
    let titles: Vec<_> = result.rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Đào", "Anh", "Ân", "Bình", "Đức"],
        "manual order is creation order"
    );
    let first = &result.rows[0];
    assert_eq!(first.values[&f.amount], CellValue::Number(3.0));
    assert_eq!(
        first.values[&f.name],
        CellValue::Text("Tìm kiếm đường đi".into())
    );
    assert!(
        !result.rows[3].values.contains_key(&f.tags),
        "empty cells are absent"
    );
    // Rows are real pages with their own content.
    let row = result.rows[1].id.parse::<NodeId>().expect("id");
    fill_page(&mut f.ws, row, &paragraphs(&["body of the row"]));
    assert_eq!(page_text(&mut f.ws, row), "body of the row");
    // Rows stay out of the sidebar.
    let sidebar = f.ws.sidebar().expect("sidebar");
    assert_eq!(sidebar.len(), 1, "only the database itself");
    assert_eq!(f.ws.children(Some(f.db)).expect("rows").len(), 5);
}

#[test]
fn the_title_property_is_the_page_title_and_stays_in_step() {
    let mut f = fixture();
    let row = f.row("Before");
    f.set(row, "title", Some(CellValue::Text("After".into())));
    assert_eq!(f.ws.node(row).expect("node").expect("row").title, "After");
    assert_eq!(f.titles(&QuerySpec::default()), ["After"]);
    f.ws.rename(row, "Renamed in the tree").expect("rename");
    assert_eq!(f.titles(&QuerySpec::default()), ["Renamed in the tree"]);
    assert!(
        f.ws.database_set_cell(f.db, row, "title", Some(CellValue::Number(1.0)))
            .is_err()
    );
}

#[test]
fn create_row_applies_default_values_and_rejects_wrong_ones() {
    let mut f = fixture();
    let defaults: BTreeMap<String, CellValue> = [(f.amount.clone(), CellValue::Number(7.0))].into();
    let row =
        f.ws.database_create_row(f.db, "With defaults", Position::End, &defaults)
            .expect("row");
    let result = f.query(&QuerySpec::default());
    assert_eq!(result.rows[0].id, row.to_string());
    assert_eq!(result.rows[0].values[&f.amount], CellValue::Number(7.0));
    let wrong: BTreeMap<String, CellValue> =
        [(f.amount.clone(), CellValue::Text("x".into()))].into();
    assert!(
        f.ws.database_create_row(f.db, "Bad", Position::End, &wrong)
            .is_err()
    );
    let other = f.row("Other");
    let err =
        f.ws.database_set_cell(f.db, other, &f.amount, Some(CellValue::Text("x".into())))
            .expect_err("type mismatch");
    assert!(err.to_string().contains("holds"));
    // A page that is not a row of this database cannot be written to.
    let stray = page_with(&mut f.ws, "stray", &[]);
    assert!(
        f.ws.database_set_cell(f.db, stray, &f.amount, Some(CellValue::Number(1.0)))
            .is_err()
    );
}

#[test]
fn filters_use_operators_of_the_property_type() {
    let mut f = populated();
    let (todo, doing) = (
        f.option(&f.status.clone(), "Cần làm"),
        f.option(&f.status.clone(), "Đang làm"),
    );
    let a = f.option(&f.tags.clone(), "a");
    let (name, amount, status, tags, due, done) = (
        f.name.clone(),
        f.amount.clone(),
        f.status.clone(),
        f.tags.clone(),
        f.due.clone(),
        f.done.clone(),
    );
    let mut ask = |node: FilterNode| {
        let mut titles = f.titles(&filter(node));
        titles.sort_by_key(|t| ne_core::collation::sort_key(t));
        titles
    };
    // Text ignores case and accents.
    assert_eq!(
        ask(cond(&name, Operator::Contains, Some(json!("GHI CHU")))),
        ["Anh", "Bình"]
    );
    assert_eq!(
        ask(cond(&name, Operator::Contains, Some(json!("duong")))),
        ["Đào"]
    );
    assert_eq!(
        ask(cond(&name, Operator::NotContains, Some(json!("ghi chu")))),
        ["Ân", "Đào", "Đức"]
    );
    assert_eq!(ask(cond(&name, Operator::Is, Some(json!("ZZZ")))), ["Đức"]);
    assert_eq!(
        ask(cond(&name, Operator::IsNot, Some(json!("zzz")))),
        ["Anh", "Ân", "Bình", "Đào"]
    );
    assert_eq!(
        ask(cond(&name, Operator::StartsWith, Some(json!("ghi")))),
        ["Anh", "Bình"]
    );
    assert_eq!(
        ask(cond(&name, Operator::EndsWith, Some(json!("DI")))),
        ["Đào"]
    );
    assert_eq!(ask(cond(&name, Operator::IsEmpty, None)), ["Ân"]);
    assert_eq!(ask(cond(&name, Operator::IsNotEmpty, None)).len(), 4);
    // The title.
    assert_eq!(
        ask(cond("title", Operator::StartsWith, Some(json!("an")))),
        ["Anh", "Ân"]
    );
    assert_eq!(
        ask(cond("title", Operator::Contains, Some(json!("dao")))),
        ["Đào"]
    );
    // Numbers.
    assert_eq!(ask(cond(&amount, Operator::Gt, Some(json!(3)))), ["Anh"]);
    assert_eq!(
        ask(cond(&amount, Operator::Gte, Some(json!(3)))),
        ["Anh", "Đào", "Đức"]
    );
    assert_eq!(ask(cond(&amount, Operator::Lt, Some(json!(0)))), ["Ân"]);
    assert_eq!(
        ask(cond(&amount, Operator::Is, Some(json!(3)))),
        ["Đào", "Đức"]
    );
    assert_eq!(
        ask(cond(&amount, Operator::IsNot, Some(json!(3)))),
        ["Anh", "Ân", "Bình"]
    );
    assert_eq!(ask(cond(&amount, Operator::IsEmpty, None)), ["Bình"]);
    // Select and multi-select.
    assert_eq!(
        ask(cond(&status, Operator::Is, Some(json!(todo)))),
        ["Đào", "Đức"]
    );
    assert_eq!(
        ask(cond(&status, Operator::IsNot, Some(json!(todo)))),
        ["Anh", "Ân", "Bình"]
    );
    assert_eq!(ask(cond(&status, Operator::IsEmpty, None)), ["Bình"]);
    assert_eq!(
        ask(cond(&tags, Operator::Contains, Some(json!(a)))),
        ["Anh", "Đào"]
    );
    assert_eq!(
        ask(cond(&tags, Operator::NotContains, Some(json!(a)))),
        ["Ân", "Bình", "Đức"]
    );
    assert_eq!(ask(cond(&tags, Operator::IsEmpty, None)), ["Bình", "Đức"]);
    // Dates compare by day; a time of day is ignored.
    assert_eq!(
        ask(cond(&due, Operator::Is, Some(json!("2026-11-22")))),
        ["Anh"]
    );
    assert_eq!(
        ask(cond(&due, Operator::Before, Some(json!("2026-11-21")))),
        ["Đào"]
    );
    assert_eq!(
        ask(cond(&due, Operator::After, Some(json!("2026-11-20")))),
        ["Anh", "Bình"]
    );
    assert_eq!(
        ask(cond(&due, Operator::OnOrBefore, Some(json!("2026-11-21")))),
        ["Bình", "Đào"]
    );
    assert_eq!(
        ask(cond(&due, Operator::OnOrAfter, Some(json!("2026-11-21")))),
        ["Anh", "Bình"]
    );
    assert_eq!(ask(cond(&due, Operator::IsEmpty, None)), ["Ân", "Đức"]);
    // Checkboxes: a missing cell is unticked.
    assert_eq!(ask(cond(&done, Operator::IsChecked, None)), ["Bình", "Đào"]);
    assert_eq!(
        ask(cond(&done, Operator::IsUnchecked, None)),
        ["Anh", "Ân", "Đức"]
    );
    // Nested groups.
    let nested = FilterNode::Group {
        logic: Logic::Or,
        children: vec![
            FilterNode::Group {
                logic: Logic::And,
                children: vec![
                    cond(&status, Operator::Is, Some(json!(todo))),
                    cond(&amount, Operator::Is, Some(json!(3))),
                    cond("title", Operator::Contains, Some(json!("duc"))),
                ],
            },
            cond(&status, Operator::Is, Some(json!(doing))),
        ],
    };
    assert_eq!(ask(nested), ["Anh", "Đức"]);
    assert_eq!(
        ask(FilterNode::Group {
            logic: Logic::And,
            children: vec![]
        })
        .len(),
        5
    );
}

#[test]
fn a_filter_on_a_deleted_property_is_ignored_instead_of_failing() {
    let mut f = populated();
    let spec = filter(cond(&f.amount.clone(), Operator::Gt, Some(json!(5))));
    assert_eq!(f.titles(&spec), ["Anh"]);
    f.ws.database_delete_property(f.db, &f.amount.clone())
        .expect("delete");
    assert_eq!(
        f.titles(&spec).len(),
        5,
        "the stale condition no longer applies"
    );
    // So does a condition whose operator no longer fits after a type change.
    let spec = filter(cond(
        &f.name.clone(),
        Operator::Contains,
        Some(json!("ghi")),
    ));
    assert_eq!(f.titles(&spec).len(), 2);
    f.ws.database_edit(f.db, |d| {
        d.change_property_type(&f.name, PropertyType::Checkbox, None)
    })
    .expect("retype");
    assert_eq!(f.titles(&spec).len(), 5);
}

#[test]
fn sorting_follows_the_vietnamese_alphabet_and_the_type_of_the_property() {
    let mut f = populated();
    assert_eq!(
        f.titles(&sorted_by(&[("title", false)])),
        ["Anh", "Ân", "Bình", "Đào", "Đức"],
        "a, â, b, d, đ"
    );
    assert_eq!(
        f.titles(&sorted_by(&[("title", true)])),
        ["Đức", "Đào", "Bình", "Ân", "Anh"]
    );
    // Numbers by value, rows without a value last in either direction.
    let amount = f.amount.clone();
    assert_eq!(
        f.titles(&sorted_by(&[(&amount, false)])),
        ["Ân", "Đào", "Đức", "Anh", "Bình"]
    );
    assert_eq!(
        f.titles(&sorted_by(&[(&amount, true)])),
        ["Anh", "Đào", "Đức", "Ân", "Bình"]
    );
    // Select by the order of its options, then a second key breaks ties.
    let status = f.status.clone();
    assert_eq!(
        f.titles(&sorted_by(&[(&status, false), ("title", true)])),
        ["Đức", "Đào", "Anh", "Ân", "Bình"]
    );
    // Dates by day; text by the Vietnamese order of its content.
    let due = f.due.clone();
    assert_eq!(
        f.titles(&sorted_by(&[(&due, false)])),
        ["Đào", "Bình", "Anh", "Ân", "Đức"]
    );
    let name = f.name.clone();
    assert_eq!(
        f.titles(&sorted_by(&[(&name, false)])),
        ["Bình", "Anh", "Đào", "Đức", "Ân"]
    );
    // Ties fall back to the manual order.
    assert_eq!(
        f.titles(&sorted_by(&[(&f.done.clone(), true)]))[..2],
        ["Đào", "Bình"]
    );
}

#[test]
fn queries_page_through_the_matching_rows() {
    let mut f = fixture();
    for i in 0..25 {
        f.row(&format!("Row {i:02}"));
    }
    let page = |f: &mut Fixture, offset, limit| {
        let spec = QuerySpec {
            offset,
            limit,
            ..sorted_by(&[("title", false)])
        };
        f.query(&spec)
    };
    let first = page(&mut f, 0, 10);
    assert_eq!((first.total, first.rows.len()), (25, 10));
    assert_eq!(first.rows[0].title, "Row 00");
    let third = page(&mut f, 20, 10);
    assert_eq!(
        third
            .rows
            .iter()
            .map(|r| r.title.clone())
            .collect::<Vec<_>>(),
        ["Row 20", "Row 21", "Row 22", "Row 23", "Row 24"]
    );
    assert!(page(&mut f, 99, 10).rows.is_empty());
}

#[test]
fn boards_group_rows_by_option_with_counts_and_a_group_for_no_value() {
    let mut f = populated();
    let (todo, doing, done) = (
        f.option(&f.status.clone(), "Cần làm"),
        f.option(&f.status.clone(), "Đang làm"),
        f.option(&f.status.clone(), "Xong"),
    );
    let spec = QuerySpec {
        group: Some(GroupConfig {
            property: f.status.clone(),
            hide_empty: false,
        }),
        limit: 1,
        ..QuerySpec::default()
    };
    let result = f.query(&spec);
    let keys: Vec<_> = result
        .groups
        .iter()
        .map(|g| (g.key.clone(), g.count))
        .collect();
    assert_eq!(
        keys,
        [
            (Some(todo.clone()), 2),
            (Some(doing), 1),
            (Some(done), 1),
            (None, 1)
        ]
    );
    assert_eq!(
        result.groups[0].rows.len(),
        1,
        "the limit applies per group"
    );
    assert_eq!(result.groups[0].rows[0].title, "Đào");
    assert!(
        !result.groups[0].rows[0].values.is_empty(),
        "group rows carry their values"
    );
    assert!(result.rows.is_empty());
    // Hiding empty groups; a multi-select row sits in each of its groups.
    let tags = QuerySpec {
        group: Some(GroupConfig {
            property: f.tags.clone(),
            hide_empty: true,
        }),
        ..QuerySpec::default()
    };
    let groups = f.query(&tags).groups;
    let counts: Vec<_> = groups.iter().map(|g| g.count).collect();
    assert_eq!(
        counts,
        [2, 2, 2],
        "a: two rows, b: two rows, no value: two rows"
    );
    // A checkbox groups into unticked and ticked.
    let ticks = QuerySpec {
        group: Some(GroupConfig {
            property: f.done.clone(),
            hide_empty: false,
        }),
        ..QuerySpec::default()
    };
    let groups = f.query(&ticks).groups;
    assert_eq!(
        groups
            .iter()
            .map(|g| (g.key.clone().unwrap_or_default(), g.count))
            .collect::<Vec<_>>(),
        [("false".to_owned(), 3), ("true".to_owned(), 2)]
    );
}

#[test]
fn a_calendar_asks_for_the_rows_whose_date_touches_the_days() {
    let mut f = populated();
    let due = f.due.clone();
    let range = |from: &str, to: &str| QuerySpec {
        date_property: Some(due.clone()),
        date_range: Some(DateRange {
            from: from.into(),
            to: to.into(),
        }),
        ..QuerySpec::default()
    };
    let mut titles = f.titles(&range("2026-11-21", "2026-11-21"));
    titles.sort_by_key(|t| ne_core::collation::sort_key(t));
    assert_eq!(titles, ["Bình"]);
    let mut titles = f.titles(&range("2026-11-20", "2026-11-22"));
    titles.sort_by_key(|t| ne_core::collation::sort_key(t));
    assert_eq!(titles, ["Anh", "Bình", "Đào"]);
    // A ranged date appears on every day it covers.
    let anh = f.query(&QuerySpec::default()).rows[1]
        .id
        .parse::<NodeId>()
        .expect("id");
    let mut d = DateValue::new("2026-12-01");
    d.end = Some("2026-12-05".into());
    f.set(anh, &due, Some(CellValue::Date(d)));
    assert_eq!(f.titles(&range("2026-12-03", "2026-12-03")), ["Anh"]);
    assert!(f.titles(&range("2026-12-06", "2026-12-09")).is_empty());
    let bad = QuerySpec {
        date_property: Some(f.name.clone()),
        date_range: Some(DateRange {
            from: "2026-11-01".into(),
            to: "2026-11-30".into(),
        }),
        ..QuerySpec::default()
    };
    assert!(f.ws.database_query(f.db, &bad).is_err());
}

#[test]
fn saved_views_drive_queries() {
    let mut f = populated();
    let (_, _) =
        f.ws.database_edit(f.db, |d| {
            let id = d.views()?[0].id.clone();
            let mut view = d.view(&id)?;
            view.kind = ViewKind::Table;
            view.sorts = vec![SortKey {
                property: "title".into(),
                descending: true,
            }];
            view.filter = Some(cond(&f.done.clone(), Operator::IsChecked, None));
            view.columns = vec![ColumnConfig {
                property: f.amount.clone(),
                visible: true,
                width: Some(120),
            }];
            d.save_view(&view)
        })
        .expect("save view");
    let view = f.ws.database_schema(f.db).expect("schema").views[0].clone();
    assert_eq!(f.titles(&QuerySpec::from_view(&view)), ["Đào", "Bình"]);
}

#[test]
fn trashing_a_row_hides_it_from_queries_and_restoring_brings_it_back() {
    let mut f = populated();
    let row = f.query(&QuerySpec::default()).rows[1]
        .id
        .parse::<NodeId>()
        .expect("id");
    f.ws.trash_node(row).expect("trash");
    let result = f.query(&QuerySpec::default());
    assert_eq!(result.total, 4);
    assert!(result.rows.iter().all(|r| r.id != row.to_string()));
    assert!(
        f.titles(&filter(cond(
            "title",
            Operator::Contains,
            Some(json!("anh"))
        )))
        .is_empty()
    );
    f.ws.restore(row).expect("restore");
    let result = f.query(&QuerySpec::default());
    assert_eq!(result.total, 5);
    assert_eq!(
        result.rows[1].title, "Anh",
        "back in its old place with its values"
    );
    assert_eq!(result.rows[1].values[&f.amount], CellValue::Number(10.0));
}

#[test]
fn trashing_the_database_hides_everything_and_deleting_it_cleans_the_index() {
    let mut f = populated();
    f.ws.trash_node(f.db).expect("trash");
    assert!(f.ws.search("anh", 10).expect("search").is_empty());
    f.ws.delete_permanently(f.db).expect("delete");
    for table in ["db_cells", "db_rows", "db_links"] {
        let count: i64 =
            f.ws.dump_derived_indexes()
                .expect("dump")
                .lines()
                .skip_while(|l| *l != format!("== {table}"))
                .skip(1)
                .take_while(|l| !l.starts_with("=="))
                .count() as i64;
        assert_eq!(count, 0, "{table} still has rows");
    }
    assert!(f.ws.database_schema(f.db).is_err());
}

#[test]
fn deleting_a_row_for_good_clears_its_cells_from_the_document() {
    let mut f = populated();
    let row = f.query(&QuerySpec::default()).rows[0]
        .id
        .parse::<NodeId>()
        .expect("id");
    f.ws.trash_node(row).expect("trash");
    f.ws.delete_permanently(row).expect("delete");
    let props = f.ws.database_schema(f.db).expect("schema").properties;
    let doc_rows =
        f.ws.database_edit(f.db, |d| Ok(d.all_row_values(&props).len()))
            .expect("values")
            .0;
    assert_eq!(
        doc_rows, 4,
        "the deleted row's cells are gone from the document"
    );
    assert_eq!(f.query(&QuerySpec::default()).total, 4);
}

#[test]
fn rows_are_found_by_title_and_by_text_property_values() {
    let mut f = populated();
    let by_title = f.ws.search("binh", 10).expect("search");
    assert_eq!(by_title.len(), 1);
    assert_eq!(by_title[0].title, "Bình");
    // "duong" appears only in the text cell of the first row.
    let by_cell = f.ws.search("tim kiem duong", 10).expect("search");
    assert_eq!(by_cell.len(), 1);
    assert_eq!(by_cell[0].title, "Đào");
    assert!(
        by_cell[0].snippet.text.contains("đường"),
        "{:?}",
        by_cell[0].snippet
    );
    // Option names are searchable too, and clearing the cell removes the hit.
    assert_eq!(f.ws.search("dang lam", 10).expect("search").len(), 1);
    let anh = f
        .query(&filter(cond("title", Operator::Is, Some(json!("anh")))))
        .rows[0]
        .id
        .parse::<NodeId>()
        .expect("id");
    f.set(anh, &f.status.clone(), None);
    assert!(f.ws.search("dang lam", 10).expect("search").is_empty());
    // Editing the cell is reflected at once.
    f.set(
        anh,
        &f.name.clone(),
        Some(CellValue::Text("hoàn toàn mới".into())),
    );
    assert_eq!(
        f.ws.search("hoan toan", 10).expect("search")[0].title,
        "Anh"
    );
    assert!(f.ws.search("thu hai", 10).expect("search").is_empty());
}

#[test]
fn the_incremental_index_equals_a_rebuild_after_edits_of_every_kind() {
    let mut f = populated();
    let rows: Vec<NodeId> = f
        .query(&QuerySpec::default())
        .rows
        .iter()
        .map(|r| r.id.parse().expect("id"))
        .collect();
    let (name, status, tags) = (f.name.clone(), f.status.clone(), f.tags.clone());
    let doing = f.option(&status, "Đang làm");
    f.set(rows[0], &name, Some(CellValue::Text("sửa".into())));
    f.set(rows[1], &status, None);
    let c = f.option(&tags, "c");
    f.set(rows[2], &tags, Some(CellValue::MultiSelect(vec![c])));
    f.ws.rename(rows[3], "Bình mới").expect("rename");
    f.ws.trash_node(rows[4]).expect("trash");
    f.ws.database_edit(f.db, |d| d.rename_option(&status, &doing, "Đang xử lý"))
        .expect("rename option");
    f.ws.database_edit(f.db, |d| {
        d.change_property_type(&name, PropertyType::Url, None)
    })
    .expect("retype");
    f.ws.database_edit(f.db, |d| d.delete_property(&tags))
        .expect("delete property");
    let extra = f.row("Thêm");
    f.set(extra, &f.amount.clone(), Some(CellValue::Number(42.0)));
    f.ws.move_node(rows[3], None, Position::End)
        .expect("move out");
    let incremental = f.ws.dump_derived_indexes().expect("dump");
    f.ws.rebuild_indexes().expect("rebuild");
    let rebuilt = f.ws.dump_derived_indexes().expect("dump");
    assert_eq!(incremental, rebuilt);
}

#[test]
fn the_index_survives_reopening() {
    let mut f = populated();
    let before = f.query(&sorted_by(&[("title", false)]));
    let dir = f._dir.path().to_path_buf();
    drop(f.ws);
    let mut ws = reopen(&dir);
    let after = ws
        .database_query(f.db, &sorted_by(&[("title", false)]))
        .expect("query");
    assert_eq!(before, after);
    ws.check_integrity().expect("sound");
}

fn relation_pair() -> (Fixture, NodeId, String, String) {
    let mut f = fixture();
    let people =
        f.ws.create_database(None, Position::End, "Người")
            .expect("people");
    let owner =
        f.ws.database_add_relation(f.db, "Phụ trách", people, Some("Việc"))
            .expect("relation");
    let backlink =
        f.ws.database_schema(people)
            .expect("schema")
            .properties
            .iter()
            .find(|p| p.name == "Việc")
            .expect("backlink")
            .id
            .clone();
    (f, people, owner, backlink)
}

#[test]
fn a_two_way_relation_stores_links_once_and_computes_the_other_side() {
    let (mut f, people, owner, backlink) = relation_pair();
    let task = f.row("Task");
    let other = f.row("Other task");
    let ann =
        f.ws.database_create_row(people, "Ann", Position::End, &BTreeMap::new())
            .expect("ann");
    let bob =
        f.ws.database_create_row(people, "Bob", Position::End, &BTreeMap::new())
            .expect("bob");
    f.set(
        task,
        &owner,
        Some(CellValue::Relation(vec![ann.to_string(), bob.to_string()])),
    );
    // The forward side shows the links with the titles of the rows.
    let result = f.query(&QuerySpec::default());
    assert_eq!(
        result
            .rows
            .iter()
            .find(|r| r.id == task.to_string())
            .expect("task")
            .values[&owner],
        CellValue::Relation(vec![ann.to_string(), bob.to_string()])
    );
    assert_eq!(result.titles[&ann.to_string()], "Ann");
    // The backlink side is computed from them.
    let on_people =
        f.ws.database_query(people, &QuerySpec::default())
            .expect("query");
    let ann_row = on_people
        .rows
        .iter()
        .find(|r| r.id == ann.to_string())
        .expect("ann");
    assert_eq!(
        ann_row.values[&backlink],
        CellValue::Relation(vec![task.to_string()])
    );
    // Filtering on both sides.
    let linked =
        f.ws.database_query(
            people,
            &filter(cond(
                &backlink,
                Operator::Contains,
                Some(json!(task.to_string())),
            )),
        )
        .expect("query");
    assert_eq!(linked.total, 2);
    let empty =
        f.ws.database_query(people, &filter(cond(&backlink, Operator::IsEmpty, None)))
            .expect("query");
    assert_eq!(empty.total, 0);
    assert_eq!(
        f.titles(&filter(cond(&owner, Operator::IsEmpty, None))),
        ["Other task"]
    );
    assert_eq!(
        f.titles(&filter(cond(
            &owner,
            Operator::Contains,
            Some(json!(bob.to_string()))
        ))),
        ["Task"]
    );
    // Editing the backlink edits the forward cells of the other database.
    f.ws.database_set_cell(
        people,
        ann,
        &backlink,
        Some(CellValue::Relation(vec![other.to_string()])),
    )
    .expect("edit backlink");
    let result = f.query(&QuerySpec::default());
    let value = |id: NodeId| {
        result
            .rows
            .iter()
            .find(|r| r.id == id.to_string())
            .expect("row")
            .values
            .get(&owner)
            .cloned()
    };
    assert_eq!(
        value(task),
        Some(CellValue::Relation(vec![bob.to_string()]))
    );
    assert_eq!(
        value(other),
        Some(CellValue::Relation(vec![ann.to_string()]))
    );
    // A target must be a row of the linked database.
    assert!(
        f.ws.database_set_cell(
            f.db,
            task,
            &owner,
            Some(CellValue::Relation(vec![task.to_string()]))
        )
        .is_err()
    );
}

#[test]
fn trashed_targets_drop_out_of_relations_and_deleting_the_property_removes_the_backlink() {
    let (mut f, people, owner, backlink) = relation_pair();
    let task = f.row("Task");
    let ann =
        f.ws.database_create_row(people, "Ann", Position::End, &BTreeMap::new())
            .expect("ann");
    f.set(
        task,
        &owner,
        Some(CellValue::Relation(vec![ann.to_string()])),
    );
    f.ws.trash_node(ann).expect("trash");
    let result = f.query(&QuerySpec::default());
    assert!(
        !result.rows[0].values.contains_key(&owner),
        "a trashed target is not shown"
    );
    assert_eq!(
        f.titles(&filter(cond(&owner, Operator::IsEmpty, None))),
        ["Task"]
    );
    f.ws.restore(ann).expect("restore");
    assert!(
        f.query(&QuerySpec::default()).rows[0]
            .values
            .contains_key(&owner),
        "restoring brings the link back"
    );
    f.ws.database_delete_property(f.db, &owner)
        .expect("delete forward");
    assert!(
        f.ws.database_schema(people)
            .expect("schema")
            .property(&backlink)
            .is_none(),
        "the backlink goes with it"
    );
}

#[test]
fn a_relation_can_point_at_its_own_database() {
    let mut f = fixture();
    let parent =
        f.ws.database_add_relation(f.db, "Cha", f.db, Some("Con"))
            .expect("relation");
    let a = f.row("A");
    let b = f.row("B");
    f.set(b, &parent, Some(CellValue::Relation(vec![a.to_string()])));
    let result = f.query(&QuerySpec::default());
    let child_prop =
        f.ws.database_schema(f.db)
            .expect("schema")
            .properties
            .iter()
            .find(|p| p.name == "Con")
            .expect("child")
            .id
            .clone();
    let a_row = result
        .rows
        .iter()
        .find(|r| r.id == a.to_string())
        .expect("a");
    assert_eq!(
        a_row.values[&child_prop],
        CellValue::Relation(vec![b.to_string()])
    );
}

#[test]
fn moving_a_card_changes_its_group_and_its_place() {
    let mut f = populated();
    let rows: Vec<NodeId> = f
        .query(&QuerySpec::default())
        .rows
        .iter()
        .map(|r| r.id.parse().expect("id"))
        .collect();
    let (doing, done) = (
        f.option(&f.status.clone(), "Đang làm"),
        f.option(&f.status.clone(), "Xong"),
    );
    // Move "Đào" (todo) into "doing", before "Ân"... positions are in the whole manual order.
    f.ws.database_move_card(
        f.db,
        rows[0],
        &f.status.clone(),
        Some(&doing),
        Some(rows[2]),
    )
    .expect("move");
    let titles = f.titles(&QuerySpec::default());
    assert_eq!(titles, ["Anh", "Đào", "Ân", "Bình", "Đức"]);
    let result = f.query(&QuerySpec {
        group: Some(GroupConfig {
            property: f.status.clone(),
            hide_empty: false,
        }),
        ..QuerySpec::default()
    });
    let doing_titles: Vec<_> = result.groups[1]
        .rows
        .iter()
        .map(|r| r.title.as_str())
        .collect();
    assert_eq!(
        doing_titles,
        ["Anh", "Đào"],
        "cards of a group keep the manual order"
    );
    // To the end, and out of every group.
    f.ws.database_move_card(f.db, rows[1], &f.status.clone(), Some(&done), None)
        .expect("to the end");
    assert_eq!(f.titles(&QuerySpec::default()).last().expect("last"), "Anh");
    f.ws.database_move_card(f.db, rows[3], &f.status.clone(), None, Some(rows[4]))
        .expect("clear");
    assert_eq!(
        f.query(&QuerySpec {
            group: Some(GroupConfig {
                property: f.status.clone(),
                hide_empty: false
            }),
            ..QuerySpec::default()
        })
        .groups
        .last()
        .expect("none group")
        .count,
        1
    );
    assert!(
        f.ws.database_move_card(f.db, rows[0], &f.name.clone(), Some("x"), None)
            .is_err(),
        "text is not a grouping"
    );
}

#[test]
fn queries_see_a_database_the_same_after_every_edit_of_the_schema() {
    let mut f = populated();
    let amount = f.amount.clone();
    f.ws.database_edit(f.db, |d| {
        d.change_property_type(&amount, PropertyType::Text, None)
    })
    .expect("retype");
    let result = f.query(&QuerySpec::default());
    assert_eq!(result.rows[0].values[&amount], CellValue::Text("3".into()));
    let spec = filter(cond(&amount, Operator::Contains, Some(json!("10"))));
    assert_eq!(f.titles(&spec), ["Anh"]);
    f.ws.database_edit(f.db, |d| {
        d.change_property_type(&amount, PropertyType::Number, None)
    })
    .expect("back");
    assert_eq!(
        f.query(&QuerySpec::default()).rows[0].values[&amount],
        CellValue::Number(3.0)
    );
}

#[test]
fn purging_removes_cells_of_deleted_properties() {
    let mut f = populated();
    f.ws.database_delete_property(f.db, &f.amount.clone())
        .expect("delete");
    assert!(f.ws.database_purge_orphans(f.db).expect("purge") >= 4);
    assert_eq!(f.ws.database_purge_orphans(f.db).expect("again"), 0);
    let _ = (
        Position::End,
        DateRange {
            from: String::new(),
            to: String::new(),
        },
    );
}
