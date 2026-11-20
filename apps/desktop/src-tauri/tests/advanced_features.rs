//! Databases, templates, page history, synced blocks, reminders and structured
//! import through `AppCore`, against a local-only workspace.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ne_core::database::{
    CellValue, FilterNode, GroupConfig, Operator, PropertyType, QuerySpec, SortKey, ViewKind,
};
use ne_core::page::{Block, PageDoc};
use note_everywhere_desktop_lib::app::CoreEvent;
use note_everywhere_desktop_lib::app::db_api::{
    DbPropertyAddArgs, DbPropertyChangeTypeArgs, DbQueryArgs, DbRowCreateArgs,
};
use note_everywhere_desktop_lib::app::reminder_api::ReminderAddArgs;
use note_everywhere_desktop_lib::app::types::{CreateNodeArgs, NodeKind};
use serde_json::json;
use support::Device;

async fn new_database(device: &Device) -> String {
    device
        .core
        .db_create(None, None, Some("Công việc"))
        .await
        .expect("database")
}

async fn add_property(device: &Device, db: &str, name: &str, ty: PropertyType) -> String {
    device
        .core
        .db_property_add(DbPropertyAddArgs {
            id: db.to_owned(),
            name: name.to_owned(),
            ty,
        })
        .await
        .expect("property")
}

async fn add_row(device: &Device, db: &str, title: &str) -> String {
    device
        .core
        .db_row_create(DbRowCreateArgs {
            id: db.to_owned(),
            title: Some(title.to_owned()),
            index: None,
            values: None,
        })
        .await
        .expect("row")
}

fn query(db: &str, spec: QuerySpec) -> DbQueryArgs {
    DbQueryArgs {
        id: db.to_owned(),
        view_id: None,
        spec: Some(spec),
        offset: None,
        limit: None,
    }
}

#[tokio::test]
async fn a_database_has_properties_rows_cells_and_views() {
    let device = Device::local().await;
    let db = new_database(&device).await;
    let amount = add_property(&device, &db, "Số lượng", PropertyType::Number).await;
    let status = add_property(&device, &db, "Trạng thái", PropertyType::Select).await;
    let todo = device
        .core
        .db_option_add(&db, &status, "Cần làm", Some("red"))
        .await
        .expect("option");
    let done = device
        .core
        .db_option_add(&db, &status, "Xong", None)
        .await
        .expect("option");
    let first = add_row(&device, &db, "Đào").await;
    let second = add_row(&device, &db, "Anh").await;
    device
        .core
        .db_cell_set(&db, &first, &amount, Some(CellValue::Number(3.0)))
        .await
        .expect("cell");
    device
        .core
        .db_cell_set(&db, &second, &amount, Some(CellValue::Number(10.0)))
        .await
        .expect("cell");
    device
        .core
        .db_cell_set(&db, &first, &status, Some(CellValue::Select(todo.clone())))
        .await
        .expect("cell");
    device
        .core
        .db_cell_set(&db, &second, &status, Some(CellValue::Select(done.clone())))
        .await
        .expect("cell");

    let result = device
        .core
        .db_query(query(&db, QuerySpec::default()))
        .await
        .expect("query");
    assert_eq!(result.total, 2);
    assert_eq!(result.rows[0].title, "Đào");
    assert_eq!(result.rows[0].values[&amount], CellValue::Number(3.0));

    // Sorted by title the Vietnamese way: Anh before Đào.
    let sorted = QuerySpec {
        sorts: vec![SortKey {
            property: "title".into(),
            descending: false,
        }],
        ..QuerySpec::default()
    };
    let rows = device
        .core
        .db_query(query(&db, sorted))
        .await
        .expect("query")
        .rows;
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Anh", "Đào"]
    );

    // A saved view drives a query by id, with paging overrides.
    let schema = device.core.db_schema(&db).await.expect("schema");
    assert_eq!(schema.properties.len(), 2);
    let mut view = schema.views[0].clone();
    view.filter = Some(FilterNode::Condition {
        property: amount.clone(),
        operator: Operator::Gt,
        value: Some(json!(5)),
    });
    device
        .core
        .db_view_save(&db, view.clone())
        .await
        .expect("save view");
    let by_view = device
        .core
        .db_query(DbQueryArgs {
            id: db.clone(),
            view_id: Some(view.id.clone()),
            spec: None,
            offset: None,
            limit: Some(10),
        })
        .await
        .expect("query");
    assert_eq!(
        by_view
            .rows
            .iter()
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        ["Anh"]
    );
    let board_id = device
        .core
        .db_view_add(&db, "Bảng", ViewKind::Board)
        .await
        .expect("board");
    device
        .core
        .db_view_move(&db, &board_id, 0)
        .await
        .expect("move");
    assert_eq!(
        device.core.db_schema(&db).await.expect("schema").views[0].id,
        board_id
    );
    device
        .core
        .db_view_delete(&db, &board_id)
        .await
        .expect("delete");

    // Cards move between groups and within them.
    let board = QuerySpec {
        group: Some(GroupConfig {
            property: status.clone(),
            hide_empty: false,
        }),
        ..QuerySpec::default()
    };
    device
        .core
        .db_card_move(&db, &first, &status, Some(&done), Some(&second))
        .await
        .expect("move card");
    let groups = device
        .core
        .db_query(query(&db, board))
        .await
        .expect("query")
        .groups;
    let titles: Vec<_> = groups
        .iter()
        .find(|g| g.key.as_deref() == Some(done.as_str()))
        .expect("done group")
        .rows
        .iter()
        .map(|r| r.title.as_str())
        .collect();
    assert_eq!(titles, ["Đào", "Anh"]);

    // Property operations.
    device
        .core
        .db_property_rename(&db, &amount, "Số")
        .await
        .expect("rename");
    device
        .core
        .db_property_move(&db, &amount, 1)
        .await
        .expect("move");
    device
        .core
        .db_option_rename(&db, &status, &todo, "Việc mới")
        .await
        .expect("rename option");
    device
        .core
        .db_option_color(&db, &status, &todo, "blue")
        .await
        .expect("colour");
    device
        .core
        .db_option_move(&db, &status, &done, 0)
        .await
        .expect("move option");
    device
        .core
        .db_option_delete(&db, &status, &todo)
        .await
        .expect("delete option");
    device
        .core
        .db_property_change_type(DbPropertyChangeTypeArgs {
            id: db.clone(),
            property_id: amount.clone(),
            ty: PropertyType::Text,
            relation_database_id: None,
        })
        .await
        .expect("retype");
    let schema = device.core.db_schema(&db).await.expect("schema");
    assert_eq!(schema.properties[1].name, "Số");
    assert_eq!(schema.properties[1].ty, PropertyType::Text);
    assert_eq!(schema.properties[0].options.len(), 1);
    device
        .core
        .db_property_delete(&db, &amount)
        .await
        .expect("delete");
    assert_eq!(
        device
            .core
            .db_schema(&db)
            .await
            .expect("schema")
            .properties
            .len(),
        1
    );

    // Every mutation told the UI.
    let changed = device.events.named("database:changed");
    assert!(changed.len() >= 15, "{} events", changed.len());
    assert!(changed.iter().all(
        |e| matches!(e, CoreEvent::DatabaseChanged { database_id, .. } if *database_id == db)
    ));
}

#[tokio::test]
async fn the_tree_lists_databases_but_not_their_rows() {
    let device = Device::local().await;
    let db = new_database(&device).await;
    let row = add_row(&device, &db, "Row").await;
    let tree = device.core.tree_snapshot().await.expect("tree");
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].kind, NodeKind::Database);
    // A row opens as an ordinary page, and a page can be moved in to become one.
    let opened = device.core.page_open(&row).await.expect("open row");
    assert!(!opened.snapshot.is_empty());
    let page = device
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Page,
            parent_id: None,
            index: None,
            title: Some("Loose".into()),
        })
        .await
        .expect("page");
    device
        .core
        .tree_move(note_everywhere_desktop_lib::app::types::MoveNodeArgs {
            id: page.clone(),
            parent_id: Some(db.clone()),
            index: None,
        })
        .await
        .expect("move in");
    assert_eq!(
        device
            .core
            .db_query(query(&db, QuerySpec::default()))
            .await
            .expect("query")
            .total,
        2
    );
    // A folder cannot go in.
    let folder = device
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Folder,
            parent_id: None,
            index: None,
            title: Some("F".into()),
        })
        .await
        .expect("folder");
    let err = device
        .core
        .tree_move(note_everywhere_desktop_lib::app::types::MoveNodeArgs {
            id: folder,
            parent_id: Some(db),
            index: None,
        })
        .await
        .expect_err("folder in database");
    assert_eq!(err.code, "invalid_input");
}

#[tokio::test]
async fn bad_requests_are_rejected_with_stable_codes() {
    let device = Device::local().await;
    let db = new_database(&device).await;
    let text = add_property(&device, &db, "Text", PropertyType::Text).await;
    let row = add_row(&device, &db, "R").await;
    let code = |e: note_everywhere_desktop_lib::app::AppError| e.code;
    assert_eq!(
        code(
            device
                .core
                .db_property_add(DbPropertyAddArgs {
                    id: db.clone(),
                    name: "r".into(),
                    ty: PropertyType::Relation
                })
                .await
                .expect_err("relation")
        ),
        "invalid_input"
    );
    assert_eq!(
        code(
            device
                .core
                .db_cell_set(&db, &row, &text, Some(CellValue::Number(1.0)))
                .await
                .expect_err("type")
        ),
        "invalid_input"
    );
    assert_eq!(
        code(device.core.db_schema("1@1").await.expect_err("unknown")),
        "not_found"
    );
    assert_eq!(
        code(device.core.db_schema("nonsense").await.expect_err("bad id")),
        "not_found"
    );
    // An inline request is checked strictly, a saved view is not.
    let bad = QuerySpec {
        filter: Some(FilterNode::Condition {
            property: "missing".into(),
            operator: Operator::IsEmpty,
            value: None,
        }),
        ..QuerySpec::default()
    };
    assert_eq!(
        code(
            device
                .core
                .db_query(query(&db, bad))
                .await
                .expect_err("unknown property")
        ),
        "not_found"
    );
    let wrong_operator = QuerySpec {
        filter: Some(FilterNode::Condition {
            property: text.clone(),
            operator: Operator::Gt,
            value: Some(json!(1)),
        }),
        ..QuerySpec::default()
    };
    assert_eq!(
        code(
            device
                .core
                .db_query(query(&db, wrong_operator))
                .await
                .expect_err("operator")
        ),
        "invalid_input"
    );
    assert_eq!(
        code(
            device
                .core
                .db_query(DbQueryArgs {
                    id: db.clone(),
                    view_id: Some("nope".into()),
                    spec: None,
                    offset: None,
                    limit: None
                })
                .await
                .expect_err("view")
        ),
        "not_found"
    );
}

#[tokio::test]
async fn results_serialise_the_way_the_contract_says() {
    let device = Device::local().await;
    let db = new_database(&device).await;
    let amount = add_property(&device, &db, "Amount", PropertyType::Number).await;
    let tags = add_property(&device, &db, "Tags", PropertyType::MultiSelect).await;
    let tag = device
        .core
        .db_option_add(&db, &tags, "a", Some("green"))
        .await
        .expect("option");
    let row = add_row(&device, &db, "Row").await;
    device
        .core
        .db_cell_set(&db, &row, &amount, Some(CellValue::Number(2.5)))
        .await
        .expect("cell");
    device
        .core
        .db_cell_set(
            &db,
            &row,
            &tags,
            Some(CellValue::MultiSelect(vec![tag.clone()])),
        )
        .await
        .expect("cell");
    let schema =
        serde_json::to_value(device.core.db_schema(&db).await.expect("schema")).expect("json");
    assert_eq!(schema["properties"][0]["type"], "number");
    assert_eq!(
        schema["properties"][1]["options"][0],
        json!({ "id": tag, "name": "a", "color": "green" })
    );
    assert_eq!(schema["views"][0]["kind"], "table");
    assert_eq!(schema["views"][0]["dateProperty"], json!(null));
    assert_eq!(schema["views"][0]["filter"], json!(null));
    let result = serde_json::to_value(
        device
            .core
            .db_query(query(&db, QuerySpec::default()))
            .await
            .expect("query"),
    )
    .expect("json");
    assert_eq!(result["total"], 1);
    assert_eq!(
        result["rows"][0]["values"][&amount],
        json!({ "type": "number", "value": 2.5 })
    );
    assert_eq!(
        result["rows"][0]["values"][&tags],
        json!({ "type": "multi_select", "value": [tag] })
    );
    assert_eq!(result["groups"], json!([]));
    assert_eq!(result["rows"][0]["icon"], json!(null));
    // The request side: camelCase fields and tagged values.
    let spec: QuerySpec = serde_json::from_value(json!({
        "filter": { "type": "group", "logic": "or", "children": [
            { "type": "condition", "property": "title", "operator": "contains", "value": "x" }
        ] },
        "sorts": [{ "property": "title", "descending": true }],
        "dateRange": { "from": "2026-01-01", "to": "2026-01-31" },
        "limit": 5
    }))
    .expect("spec");
    assert_eq!(spec.limit, 5);
    assert!(spec.date_range.is_some());
    let value: CellValue = serde_json::from_value(
        json!({ "type": "date", "value": { "start": "2026-11-21", "end": "2026-11-22" } }),
    )
    .expect("date");
    assert!(matches!(value, CellValue::Date(d) if d.end.as_deref() == Some("2026-11-22")));
}

#[tokio::test]
async fn templates_make_pages_and_rows() {
    let device = Device::local().await;
    let page = device
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Page,
            parent_id: None,
            index: None,
            title: Some("Mẫu".into()),
        })
        .await
        .expect("page");
    let opened = device.core.page_open(&page).await.expect("open");
    let doc = PageDoc::from_snapshot(&opened.snapshot).expect("doc");
    let (_, update) = doc
        .edit(|d| {
            d.append_blocks(&[], &[Block::paragraph("nội dung mẫu")])
                .map(drop)
        })
        .expect("edit");
    device
        .core
        .page_apply_update(&page, &update)
        .await
        .expect("apply");
    let template = device
        .core
        .template_create_from_page(&page, "Biên bản")
        .await
        .expect("template");
    let listed = device.core.template_list(None).await.expect("list");
    assert_eq!(listed.len(), 1);
    let json = serde_json::to_value(&listed[0]).expect("json");
    assert_eq!(json["name"], "Biên bản");
    assert_eq!(json["kind"], "page");
    assert!(json.get("blocks").is_none(), "listings carry no content");
    let copy = device
        .core
        .template_instantiate_page(&template, None, None, Some("Tuần 47"))
        .await
        .expect("copy");
    let text = device.core.page_open(&copy).await.expect("open copy");
    assert!(
        PageDoc::from_snapshot(&text.snapshot)
            .expect("doc")
            .plain_text()
            .expect("text")
            .contains("nội dung mẫu")
    );

    let db = new_database(&device).await;
    let amount = add_property(&device, &db, "Amount", PropertyType::Number).await;
    let model = add_row(&device, &db, "Model").await;
    device
        .core
        .db_cell_set(&db, &model, &amount, Some(CellValue::Number(9.0)))
        .await
        .expect("cell");
    let row_template = device
        .core
        .template_create_for_database(&db, "Hàng mẫu", Some(&model))
        .await
        .expect("template");
    assert_eq!(
        device
            .core
            .template_list(Some(&db))
            .await
            .expect("list")
            .len(),
        1
    );
    let row = device
        .core
        .template_instantiate_row(&row_template, Some("Mới"))
        .await
        .expect("row");
    let rows = device
        .core
        .db_query(query(&db, QuerySpec::default()))
        .await
        .expect("query")
        .rows;
    let made = rows.iter().find(|r| r.id == row).expect("made row");
    assert_eq!(made.values[&amount], CellValue::Number(9.0));
    let defaults: BTreeMap<String, CellValue> = [(amount, CellValue::Number(1.0))].into();
    device
        .core
        .template_set_defaults(&row_template, &defaults)
        .await
        .expect("defaults");
    device
        .core
        .template_rename(&row_template, "Đổi tên")
        .await
        .expect("rename");
    device
        .core
        .template_delete(&row_template)
        .await
        .expect("delete");
    assert!(
        device
            .core
            .template_list(Some(&db))
            .await
            .expect("list")
            .is_empty()
    );
    assert_eq!(
        device
            .core
            .template_instantiate_row("zz", None)
            .await
            .expect_err("unknown")
            .code,
        "not_found"
    );
}

#[tokio::test]
async fn restoring_a_version_reaches_the_open_editor() {
    let device = Device::local().await;
    let page = device
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Page,
            parent_id: None,
            index: None,
            title: Some("Notes".into()),
        })
        .await
        .expect("page");
    let editor =
        PageDoc::from_snapshot(&device.core.page_open(&page).await.expect("open").snapshot)
            .expect("doc");
    editor.set_peer_id(777).expect("peer");
    // The editor typed two paragraphs, a long time apart.
    for text in ["first", "second"] {
        let (_, update) = editor
            .edit(|d| d.append_blocks(&[], &[Block::paragraph(text)]).map(drop))
            .expect("edit");
        device
            .core
            .page_apply_update(&page, &update)
            .await
            .expect("apply");
    }
    let history = device.core.page_history(&page).await.expect("history");
    assert!(history.previewable && !history.trimmed);
    assert!(!history.versions.is_empty());
    let skeleton_version = history.versions.last().expect("oldest").id.clone();
    let preview = device
        .core
        .page_version_preview(&page, &skeleton_version)
        .await
        .expect("preview");
    assert!(
        preview.iter().all(|b| b.plain_text().is_empty()),
        "the first version is the empty page"
    );
    let json = serde_json::to_value(&preview).expect("json");
    assert_eq!(json[0]["kind"]["type"], "paragraph");
    device.events.clear();
    device
        .core
        .page_version_restore(&page, &skeleton_version)
        .await
        .expect("restore");
    let events = device.events.named("page:remote-update");
    assert_eq!(events.len(), 1, "the open editor is told");
    let CoreEvent::PageRemoteUpdate { update, page_id } = &events[0] else {
        panic!("unexpected event");
    };
    assert_eq!(*page_id, page);
    editor.import(update).expect("the editor takes the update");
    assert_eq!(
        editor.plain_text().expect("text").trim(),
        "",
        "the editor shows the restored page"
    );
    assert_eq!(
        device
            .core
            .page_version_restore(&page, "00")
            .await
            .expect_err("bad version")
            .code,
        "not_found"
    );
}

#[tokio::test]
async fn synced_blocks_are_shared_between_pages_through_the_core() {
    let device = Device::local().await;
    let make = |title: &'static str| {
        let core = device.core.clone();
        async move {
            core.tree_create(CreateNodeArgs {
                kind: NodeKind::Page,
                parent_id: None,
                index: None,
                title: Some(title.into()),
            })
            .await
            .expect("page")
        }
    };
    let (one, two) = (make("One").await, make("Two").await);
    let editor = PageDoc::from_snapshot(&device.core.page_open(&one).await.expect("open").snapshot)
        .expect("doc");
    editor.set_peer_id(555).expect("peer");
    let (_, update) = editor
        .edit(|d| {
            d.append_blocks(&[], &[Block::paragraph("keep"), Block::paragraph("shared")])
                .map(drop)
        })
        .expect("edit");
    device
        .core
        .page_apply_update(&one, &update)
        .await
        .expect("apply");
    let blocks = editor.blocks().expect("blocks").len();
    assert_eq!(blocks, 3, "the skeleton paragraph and two more");

    device.events.clear();
    let sync_id = device.core.synced_create(&one, 2, 3).await.expect("create");
    let pushed = device.events.named("page:remote-update");
    assert_eq!(pushed.len(), 1, "the open editor gets the page change");
    let CoreEvent::PageRemoteUpdate { update, .. } = &pushed[0] else {
        panic!()
    };
    editor.import(update).expect("import");
    assert!(matches!(
        editor.blocks().expect("blocks")[2].kind,
        ne_core::page::BlockKind::SyncedBlock { .. }
    ));

    device
        .core
        .synced_insert(&two, &[], 0, &sync_id)
        .await
        .expect("insert");
    let occurrences = device
        .core
        .synced_occurrences(&sync_id)
        .await
        .expect("occurrences");
    assert_eq!(
        occurrences
            .iter()
            .map(|o| o.title.as_str())
            .collect::<Vec<_>>(),
        ["One", "Two"]
    );
    let list = device.core.synced_list().await.expect("list");
    assert_eq!(
        (list.len(), list[0].occurrences, list[0].orphaned_at),
        (1, 2, None)
    );
    assert_eq!(
        serde_json::to_value(&list[0]).expect("json")["syncId"],
        json!(sync_id)
    );

    // The nested editor opens the shared document and edits it; the document is
    // marked open so remote updates would be forwarded.
    let shared = PageDoc::from_snapshot(
        &device
            .core
            .synced_open(&sync_id)
            .await
            .expect("open")
            .snapshot,
    )
    .expect("doc");
    shared.set_peer_id(556).expect("peer");
    let (_, edit) = shared
        .edit(|d| d.insert_text(&[0], 0, "edited "))
        .expect("edit");
    device
        .core
        .synced_apply_update(&sync_id, &edit)
        .await
        .expect("apply");
    let again = PageDoc::from_snapshot(
        &device
            .core
            .synced_open(&sync_id)
            .await
            .expect("open")
            .snapshot,
    )
    .expect("doc");
    assert_eq!(again.plain_text().expect("text"), "edited shared");
    device.core.synced_close(&sync_id).await.expect("close");
    assert_eq!(
        device
            .core
            .synced_open("nonsense")
            .await
            .expect_err("bad id")
            .code,
        "not_found"
    );
    assert_eq!(
        device
            .core
            .synced_apply_update(&sync_id, &[])
            .await
            .expect_err("empty")
            .code,
        "invalid_input"
    );

    device.core.synced_unsync(&two, &[0]).await.expect("unsync");
    assert_eq!(
        device
            .core
            .synced_occurrences(&sync_id)
            .await
            .expect("occurrences")
            .len(),
        1
    );
}

const MINUTE: i64 = 60_000;

#[tokio::test]
async fn reminders_fire_as_events_once_and_not_when_stale() {
    let device = Device::local().await;
    let page = device
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Page,
            parent_id: None,
            index: None,
            title: Some("Gọi điện".into()),
        })
        .await
        .expect("page");
    let base = 1_900_000_000_000;
    let reminder = device
        .core
        .reminder_add(ReminderAddArgs {
            page_id: page.clone(),
            property_id: None,
            at: Some(base + 5 * MINUTE),
            utc_offset_minutes: 420,
            note: Some("gọi bác sĩ".into()),
        })
        .await
        .expect("add");
    let stale = device
        .core
        .reminder_add(ReminderAddArgs {
            page_id: page.clone(),
            property_id: None,
            at: Some(base - 60 * MINUTE),
            utc_offset_minutes: 420,
            note: None,
        })
        .await
        .expect("add");
    assert!(device.events.named("reminders:changed").len() >= 2);
    assert!(
        device
            .core
            .reminders_check(base)
            .expect("check")
            .iter()
            .all(|d| d.reminder.id != reminder.id),
        "not due yet"
    );
    // The first look happens an hour after the stale one was due: it is skipped.
    assert!(device.core.reminders_check(base).expect("check").is_empty());
    device.events.clear();
    let fired = device
        .core
        .reminders_check(base + 6 * MINUTE)
        .expect("check");
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].reminder.id, reminder.id);
    assert_eq!(fired[0].page_title, "Gọi điện");
    let events = device.events.named("reminder:due");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].payload()["reminders"][0]["reminder"]["note"],
        json!("gọi bác sĩ")
    );
    assert_eq!(
        events[0].payload()["reminders"][0]["dueAt"],
        json!(base + 5 * MINUTE)
    );
    assert!(
        device
            .core
            .reminders_check(base + 7 * MINUTE)
            .expect("check")
            .is_empty(),
        "once"
    );

    let open = device.core.reminder_list(None).await.expect("list");
    assert_eq!(open.len(), 2, "the stale one still shows as overdue");
    device
        .core
        .reminder_set_done(&stale.id, true)
        .await
        .expect("done");
    assert_eq!(
        device.core.reminder_list(None).await.expect("list").len(),
        1
    );
    assert_eq!(
        device
            .core
            .reminder_list(Some(true))
            .await
            .expect("list")
            .len(),
        2
    );
    device
        .core
        .reminder_snooze(&reminder.id, base + 30 * MINUTE)
        .await
        .expect("snooze");
    assert_eq!(
        device
            .core
            .reminders_check(base + 31 * MINUTE)
            .expect("check")
            .len(),
        1,
        "a snooze fires again"
    );
    device
        .core
        .reminder_reschedule(&reminder.id, base + 90 * MINUTE)
        .await
        .expect("reschedule");
    device
        .core
        .reminder_set_note(&reminder.id, "new")
        .await
        .expect("note");
    device
        .core
        .reminder_delete(&reminder.id)
        .await
        .expect("delete");
    assert!(
        device
            .core
            .reminders_check(base + 100 * MINUTE)
            .expect("check")
            .is_empty()
    );
    assert_eq!(
        device
            .core
            .reminder_set_done("zz", true)
            .await
            .expect_err("unknown")
            .code,
        "not_found"
    );
    let reminder_json = serde_json::to_value(&open[0]).expect("json");
    assert!(reminder_json["reminder"]["utcOffsetMinutes"].is_number());
    assert!(reminder_json["pageVisible"].as_bool().unwrap_or(false));
}

#[tokio::test]
async fn a_reminder_on_a_date_property_follows_the_cell() {
    let device = Device::local().await;
    let db = new_database(&device).await;
    let due = add_property(&device, &db, "Hạn", PropertyType::Date).await;
    let row = add_row(&device, &db, "Nộp").await;
    assert!(
        device
            .core
            .reminder_add(ReminderAddArgs {
                page_id: row.clone(),
                property_id: Some(due.clone()),
                at: None,
                utc_offset_minutes: 0,
                note: None
            })
            .await
            .is_err(),
        "no date yet"
    );
    device
        .core
        .db_cell_set(
            &db,
            &row,
            &due,
            Some(CellValue::Date(ne_core::database::DateValue::new(
                "2026-12-01T10:00",
            ))),
        )
        .await
        .expect("date");
    let reminder = device
        .core
        .reminder_add(ReminderAddArgs {
            page_id: row.clone(),
            property_id: Some(due.clone()),
            at: None,
            utc_offset_minutes: 0,
            note: None,
        })
        .await
        .expect("add");
    let first = ne_core::database::to_epoch_ms("2026-12-01T10:00", 0, 0).expect("ms");
    assert_eq!(reminder.at, first);
    device
        .core
        .db_cell_set(
            &db,
            &row,
            &due,
            Some(CellValue::Date(ne_core::database::DateValue::new(
                "2026-12-05T08:00",
            ))),
        )
        .await
        .expect("move date");
    let listed = device.core.reminder_list(None).await.expect("list");
    assert_eq!(
        listed[0].due_at,
        ne_core::database::to_epoch_ms("2026-12-05T08:00", 0, 0).expect("ms")
    );
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    fs::write(path, content).expect("write");
}

#[tokio::test]
async fn a_structured_export_imports_through_the_folder_picker() {
    let device = Device::local().await;
    let export = tempfile::tempdir().expect("export");
    let (home, tasks, ann) = (
        format!("{:032x}", 1),
        format!("{:032x}", 2),
        format!("{:032x}", 3),
    );
    write(
        export.path(),
        &format!("Home {home}.md"),
        &format!("# Home\n\nSee [tasks](Tasks%20{tasks}.csv).\n"),
    );
    write(
        export.path(),
        &format!("Tasks {tasks}.csv"),
        "Name,Status\nWrite,Todo\nShip,Done\nTest,Todo\nDocs,Done\n",
    );
    write(
        export.path(),
        &format!("Tasks/Write {ann}.md"),
        "# Write\n\nBody\n",
    );
    // Cancelling the picker imports nothing.
    device.dialogs.queue_folder(None);
    assert!(
        device
            .core
            .import_structured(None)
            .await
            .expect("cancel")
            .is_none()
    );
    device
        .dialogs
        .queue_folder(Some(export.path().to_path_buf()));
    let report = device
        .core
        .import_structured(None)
        .await
        .expect("import")
        .expect("report");
    assert_eq!(report["databases"], 1);
    assert_eq!(report["rows"], 4);
    assert_eq!(report["pages"], 1);
    assert_eq!(report["linksRewritten"], 1);
    for key in ["folders", "attachments", "linksUnresolved", "imagesMissing"] {
        assert!(report[key].is_number(), "{key}");
    }
    assert!(report["skipped"].is_array());
    let tree = device.core.tree_snapshot().await.expect("tree");
    let database = tree
        .iter()
        .find(|n| n.kind == NodeKind::Database)
        .expect("database");
    assert_eq!(database.title, "Tasks");
    let result = device
        .core
        .db_query(query(&database.id, QuerySpec::default()))
        .await
        .expect("query");
    assert_eq!(result.total, 4);
    let schema = device.core.db_schema(&database.id).await.expect("schema");
    assert_eq!(schema.properties[0].ty, PropertyType::Select);
    assert!(!device.events.named("tree:changed").is_empty());
}
