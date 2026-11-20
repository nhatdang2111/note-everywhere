//! Templates, page history, reminders, synced blocks and structured import on
//! the workspace.

mod store_support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ne_core::NodeId;
use ne_core::database::{CellValue, DateValue, PropertyType, QuerySpec};
use ne_core::page::{Block, BlockKind, PageDoc};
use ne_core::tree::{NodeKind, Position};
use ne_store::{DocRef, Workspace};
use store_support::*;

fn timed_workspace() -> (tempfile::TempDir, Workspace, TestClock) {
    let clock = TestClock::new(1_800_000_000_000);
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = Workspace::create(dir.path(), config_with(&clock)).expect("create");
    (dir, ws, clock)
}

fn texts(ws: &mut Workspace, page: NodeId) -> Vec<String> {
    ws.open_page(page)
        .expect("open")
        .blocks()
        .expect("blocks")
        .iter()
        .map(|b| match &b.kind {
            BlockKind::SyncedBlock { sync_id } => format!("[{sync_id}]"),
            _ => b.plain_text(),
        })
        .filter(|t| !t.is_empty())
        .collect()
}

// ------------------------------------------------------------- templates --

#[test]
fn a_page_template_makes_independent_copies() {
    let (_dir, mut ws) = new_workspace();
    let source = page_with(&mut ws, "Biên bản", &paragraphs(&["Mục 1", "Mục 2"]));
    let template = ws.template_from_page(source, "Họp tuần").expect("template");
    ws.edit_page(source, |doc| {
        doc.append_block(&[], &Block::paragraph("added later"))
            .map(drop)
    })
    .expect("edit source");
    let copy = ws
        .template_new_page(&template, None, Position::End, None)
        .expect("copy");
    assert_eq!(
        ws.node(copy).expect("node").expect("copy").title,
        "Họp tuần",
        "the template's name by default"
    );
    assert_eq!(
        texts(&mut ws, copy),
        ["Mục 1", "Mục 2"],
        "content as saved, not as the source is now"
    );
    let named = ws
        .template_new_page(&template, None, Position::End, Some("Tuần 47"))
        .expect("copy");
    assert_eq!(
        ws.node(named).expect("node").expect("copy").title,
        "Tuần 47"
    );
    // Editing a copy changes neither the template nor other copies.
    ws.edit_page(copy, |doc| {
        doc.append_block(&[], &Block::paragraph("only here"))
            .map(drop)
    })
    .expect("edit copy");
    assert_eq!(texts(&mut ws, named), ["Mục 1", "Mục 2"]);
    let listed = ws.templates(None).expect("list");
    assert_eq!(listed.len(), 1);
    ws.template_rename(&template, "Họp").expect("rename");
    assert_eq!(ws.templates(None).expect("list")[0].name, "Họp");
    ws.template_delete(&template).expect("delete");
    assert!(ws.templates(None).expect("list").is_empty());
    assert!(
        ws.template_new_page(&template, None, Position::End, None)
            .is_err()
    );
}

#[test]
fn a_row_template_carries_content_and_default_values() {
    let (_dir, mut ws) = new_workspace();
    let db = ws
        .create_database(None, Position::End, "Tasks")
        .expect("db");
    let (status, _) = ws
        .database_edit(db, |d| d.add_property("Status", PropertyType::Select, None))
        .expect("status");
    let (hours, _) = ws
        .database_edit(db, |d| d.add_property("Hours", PropertyType::Number, None))
        .expect("hours");
    let (gone, _) = ws
        .database_edit(db, |d| d.add_property("Gone", PropertyType::Text, None))
        .expect("gone");
    let (todo, _) = ws
        .database_edit(db, |d| d.add_option(&status, "Todo", None))
        .expect("option");
    let model = ws
        .database_create_row(db, "Model", Position::End, &BTreeMap::new())
        .expect("row");
    fill_page(&mut ws, model, &paragraphs(&["checklist"]));
    for (p, v) in [
        (&status, CellValue::Select(todo.clone())),
        (&hours, CellValue::Number(2.0)),
        (&gone, CellValue::Text("temp".into())),
    ] {
        ws.database_set_cell(db, model, p, Some(v)).expect("cell");
    }
    let template = ws
        .template_for_database(db, "Standard task", Some(model), None)
        .expect("template");
    assert_eq!(ws.templates(Some(db)).expect("list").len(), 1);
    assert!(
        ws.templates(None).expect("list").is_empty(),
        "row templates are not page templates"
    );
    // A property that disappears is skipped, the rest applies.
    ws.database_delete_property(db, &gone)
        .expect("delete property");
    let row = ws.template_new_row(&template, Some("Task A")).expect("row");
    assert_eq!(texts(&mut ws, row), ["checklist"]);
    let result = ws.database_query(db, &QuerySpec::default()).expect("query");
    let made = result
        .rows
        .iter()
        .find(|r| r.id == row.to_string())
        .expect("row");
    assert_eq!(made.title, "Task A");
    assert_eq!(made.values[&status], CellValue::Select(todo));
    assert_eq!(made.values[&hours], CellValue::Number(2.0));
    assert!(!made.values.contains_key(&gone));
    // A default that no longer fits (the property changed type) is skipped.
    ws.database_edit(db, |d| {
        d.change_property_type(&hours, PropertyType::Checkbox, None)
    })
    .expect("retype");
    let row = ws.template_new_row(&template, None).expect("row");
    let result = ws.database_query(db, &QuerySpec::default()).expect("query");
    let made = result
        .rows
        .iter()
        .find(|r| r.id == row.to_string())
        .expect("row");
    assert_eq!(made.title, "Standard task");
    assert!(!made.values.contains_key(&hours));
    // Page templates cannot make rows and vice versa.
    let page_template = ws.template_from_page(model, "p").expect("page template");
    assert!(ws.template_new_row(&page_template, None).is_err());
    assert!(
        ws.template_new_page(&template, None, Position::End, None)
            .is_err()
    );
}

#[test]
fn templates_sync_like_other_documents() {
    let (_da, mut a) = new_workspace();
    let (_db, mut b) = new_workspace();
    let source = page_with(&mut a, "Source", &paragraphs(&["shared body"]));
    let id = a.template_from_page(source, "Mẫu").expect("template");
    let b_source = page_with(&mut b, "B source", &paragraphs(&["b body"]));
    let other = b.template_from_page(b_source, "Mẫu B").expect("template");
    deliver(&mut a, &mut b);
    deliver(&mut b, &mut a);
    for ws in [&mut a, &mut b] {
        let names: Vec<String> = ws
            .templates(None)
            .expect("list")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }
    // The other device can make a page from a template created here.
    let page = b
        .template_new_page(&id, None, Position::End, None)
        .expect("page");
    assert_eq!(texts(&mut b, page), ["shared body"]);
    a.template_delete(&other).expect("delete");
    deliver(&mut a, &mut b);
    assert_eq!(b.templates(None).expect("list").len(), 1);
}

// --------------------------------------------------------------- history --

#[test]
fn history_lists_previews_and_restores_as_a_change_other_devices_get() {
    let (_da, mut a, clock) = timed_workspace();
    let page = a.create_page(None, Position::End, "Notes").expect("page");
    a.edit_page(page, |d| d.replace_blocks(&paragraphs(&["first draft"])))
        .expect("first");
    clock.advance(10 * 60_000);
    a.edit_page(page, |d| d.replace_blocks(&paragraphs(&["second draft"])))
        .expect("second");
    clock.advance(10 * 60_000);
    a.edit_page(page, |d| d.replace_blocks(&paragraphs(&["third draft"])))
        .expect("third");
    let history = a.page_history(page).expect("history");
    assert!(history.previewable && !history.trimmed);
    assert!(history.versions.len() >= 4, "{:?}", history.versions);
    let blocks_of = |ws: &mut Workspace, i: usize| -> Vec<String> {
        let id = ws.page_history(page).expect("history").versions[i]
            .id
            .clone();
        ws.page_version_blocks(page, &id)
            .expect("preview")
            .iter()
            .map(Block::plain_text)
            .collect()
    };
    assert_eq!(blocks_of(&mut a, 0), ["third draft"]);
    assert_eq!(blocks_of(&mut a, 1), ["second draft"]);
    assert_eq!(blocks_of(&mut a, 2), ["first draft"]);

    let (_db, mut b) = new_workspace();
    deliver(&mut a, &mut b);
    assert_eq!(texts(&mut b, page), ["third draft"]);

    let target = history.versions[1].id.clone();
    clock.advance(10 * 60_000);
    let (update, change) = a.restore_page_version(page, &target).expect("restore");
    assert!(!update.is_empty());
    assert_eq!(change.page, Some(page));
    assert_eq!(texts(&mut a, page), ["second draft"]);
    assert!(
        a.page_history(page).expect("history").versions.len() > history.versions.len(),
        "history grows"
    );
    // The restore is an ordinary update: the other device converges on it.
    deliver(&mut a, &mut b);
    assert_eq!(texts(&mut b, page), ["second draft"]);
    // The search index follows the restored text.
    assert_eq!(a.search("second", 5).expect("search").len(), 1);
    assert!(a.search("third", 5).expect("search").is_empty());
    assert!(a.page_version_blocks(page, "00").is_err());
}

// ------------------------------------------------------------- reminders --

const MINUTE: i64 = 60_000;
const GRACE: i64 = 10 * MINUTE;

#[test]
fn a_reminder_fires_once_when_due_and_not_when_done() {
    let (_dir, mut ws, clock) = timed_workspace();
    let page = page_with(&mut ws, "Gọi điện", &[]);
    let base = 1_800_000_000_000;
    let reminder = ws
        .reminder_add(page, None, Some(base + 5 * MINUTE), 420, "gọi bác sĩ")
        .expect("add");
    let later = ws
        .reminder_add(page, None, Some(base + 60 * MINUTE), 420, "")
        .expect("add");
    assert!(
        ws.reminders_take_due(base, GRACE).expect("take").is_empty(),
        "not due yet"
    );
    let upcoming = ws.reminders(false).expect("list");
    assert_eq!(
        upcoming
            .iter()
            .map(|e| e.reminder.id.clone())
            .collect::<Vec<_>>(),
        [reminder.id.clone(), later.id.clone()]
    );
    assert_eq!(upcoming[0].page_title, "Gọi điện");
    let fired = ws
        .reminders_take_due(base + 6 * MINUTE, GRACE)
        .expect("take");
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].reminder.note, "gọi bác sĩ");
    assert!(
        ws.reminders_take_due(base + 7 * MINUTE, GRACE)
            .expect("take")
            .is_empty(),
        "once"
    );
    // A restart does not fire it again.
    let dir = _dir.path().to_path_buf();
    drop(ws);
    let mut ws = Workspace::open(&dir, config_with(&clock)).expect("reopen");
    assert!(
        ws.reminders_take_due(base + 8 * MINUTE, GRACE)
            .expect("take")
            .is_empty()
    );
    // Snoozing gives a new due moment, which fires again; done never fires.
    ws.reminder_snooze(&reminder.id, base + 30 * MINUTE)
        .expect("snooze");
    assert!(
        ws.reminders_take_due(base + 20 * MINUTE, GRACE)
            .expect("take")
            .is_empty()
    );
    assert_eq!(
        ws.reminders_take_due(base + 31 * MINUTE, GRACE)
            .expect("take")
            .len(),
        1
    );
    ws.reminder_set_done(&later.id, true).expect("done");
    assert!(
        ws.reminders_take_due(base + 61 * MINUTE, GRACE)
            .expect("take")
            .is_empty()
    );
    assert_eq!(
        ws.reminders(false).expect("list").len(),
        1,
        "the snoozed one is still open"
    );
    assert_eq!(ws.reminders(true).expect("list").len(), 2);
    ws.reminder_reschedule(&later.id, base + 90 * MINUTE)
        .expect("reschedule");
    assert_eq!(
        ws.reminders_take_due(base + 91 * MINUTE, GRACE)
            .expect("take")
            .len(),
        1,
        "reopened and due"
    );
}

#[test]
fn reminders_that_are_long_overdue_when_the_device_looks_do_not_fire() {
    let (_dir, mut ws, _clock) = timed_workspace();
    let page = page_with(&mut ws, "Old", &[]);
    let base = 1_800_000_000_000;
    let missed = ws
        .reminder_add(page, None, Some(base + MINUTE), 0, "")
        .expect("add");
    let recent = ws
        .reminder_add(page, None, Some(base + 2 * MINUTE), 0, "")
        .expect("add");
    // The device was off; it looks 3 hours later for the first and a minute
    // after the second.
    let fired = ws
        .reminders_take_due(base + 3 * 60 * MINUTE, GRACE)
        .expect("take");
    assert!(fired.is_empty(), "both are older than the grace window");
    let overdue = ws.reminders(false).expect("list");
    assert_eq!(overdue.len(), 2, "they still show as overdue");
    assert!(overdue.iter().all(|e| e.due_at < base + 3 * 60 * MINUTE));
    let _ = (missed, recent);
    // Within the window a reminder fires even if the check was a little late.
    let fresh = ws
        .reminder_add(page, None, Some(base + 4 * 60 * MINUTE), 0, "")
        .expect("add");
    let fired = ws
        .reminders_take_due(base + 4 * 60 * MINUTE + 9 * MINUTE, GRACE)
        .expect("take");
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].reminder.id, fresh.id);
}

#[test]
fn a_reminder_on_a_date_property_follows_the_date() {
    let (_dir, mut ws, _clock) = timed_workspace();
    let db = ws.create_database(None, Position::End, "Hạn").expect("db");
    let (due, _) = ws
        .database_edit(db, |d| d.add_property("Due", PropertyType::Date, None))
        .expect("due");
    let (text, _) = ws
        .database_edit(db, |d| d.add_property("Text", PropertyType::Text, None))
        .expect("text");
    let row = ws
        .database_create_row(db, "Nộp thuế", Position::End, &BTreeMap::new())
        .expect("row");
    assert!(
        ws.reminder_add(row, Some(&due), None, 420, "").is_err(),
        "the cell holds no date yet"
    );
    assert!(
        ws.reminder_add(row, Some(&text), None, 420, "").is_err(),
        "not a date property"
    );
    ws.database_set_cell(
        db,
        row,
        &due,
        Some(CellValue::Date(DateValue::new("2026-11-30"))),
    )
    .expect("date");
    let reminder = ws
        .reminder_add(row, Some(&due), None, 420, "đừng quên")
        .expect("add");
    // 09:00 on the 30th in UTC+7 is 02:00 UTC.
    let nine = ne_core::database::to_epoch_ms("2026-11-30T02:00", 0, 0).expect("moment");
    assert_eq!(reminder.at, nine);
    assert_eq!(ws.reminders(false).expect("list")[0].due_at, nine);
    // Moving the date moves the reminder.
    ws.database_set_cell(
        db,
        row,
        &due,
        Some(CellValue::Date(DateValue::new("2026-12-02T18:30"))),
    )
    .expect("date");
    let moved = ne_core::database::to_epoch_ms("2026-12-02T11:30", 0, 0).expect("moment");
    assert_eq!(ws.reminders(false).expect("list")[0].due_at, moved);
    assert!(
        ws.reminders_take_due(nine + MINUTE, GRACE)
            .expect("take")
            .is_empty(),
        "not due at the old date"
    );
    assert_eq!(
        ws.reminders_take_due(moved + MINUTE, GRACE)
            .expect("take")
            .len(),
        1
    );
}

#[test]
fn reminders_sync_and_each_device_fires_its_own() {
    let (_da, mut a, _ca) = timed_workspace();
    let (_db, mut b, _cb) = timed_workspace();
    let page = page_with(&mut a, "Shared", &[]);
    let base = 1_800_000_000_000;
    let reminder = a
        .reminder_add(page, None, Some(base + MINUTE), 0, "hello")
        .expect("add");
    deliver(&mut a, &mut b);
    assert_eq!(b.reminders(false).expect("list").len(), 1);
    assert_eq!(b.reminders(false).expect("list")[0].reminder.note, "hello");
    // Both fire for themselves.
    assert_eq!(
        a.reminders_take_due(base + 2 * MINUTE, GRACE)
            .expect("take")
            .len(),
        1
    );
    assert_eq!(
        b.reminders_take_due(base + 2 * MINUTE, GRACE)
            .expect("take")
            .len(),
        1
    );
    // Done on one device reaches the other.
    b.reminder_set_done(&reminder.id, true).expect("done");
    deliver(&mut b, &mut a);
    assert!(a.reminders(false).expect("list").is_empty());
    assert_eq!(a.reminders(true).expect("list").len(), 1);
    // Deleting the page keeps the reminder out of the way: it is not visible.
    let other = a
        .reminder_add(page, None, Some(base + 10 * MINUTE), 0, "")
        .expect("add");
    a.trash_node(page).expect("trash");
    assert!(
        a.reminders_take_due(base + 11 * MINUTE, GRACE)
            .expect("take")
            .is_empty()
    );
    assert!(
        !a.reminders(false)
            .expect("list")
            .iter()
            .find(|e| e.reminder.id == other.id)
            .expect("entry")
            .page_visible
    );
}

// ---------------------------------------------------------- synced blocks --

#[test]
fn a_synced_block_is_one_document_shown_on_several_pages() {
    let (_dir, mut ws, clock) = timed_workspace();
    let one = page_with(
        &mut ws,
        "One",
        &paragraphs(&["intro", "shared a", "shared b", "outro"]),
    );
    let two = page_with(&mut ws, "Two", &paragraphs(&["other page"]));
    let (sync_id, update) = ws.synced_create(one, 1, 3).expect("create");
    assert!(!update.is_empty());
    assert_eq!(
        texts(&mut ws, one),
        ["intro", &format!("[{sync_id}]"), "outro"]
    );
    ws.synced_insert(two, &[], 1, sync_id).expect("insert");
    let occurrences: Vec<String> = ws
        .synced_occurrences(sync_id)
        .expect("occurrences")
        .into_iter()
        .map(|o| o.title)
        .collect();
    assert_eq!(occurrences, ["One", "Two"]);
    // The editor edits the shared document through its own replica; every
    // occurrence reads the same content.
    let snapshot = ws.synced_open(sync_id).expect("open");
    let replica = PageDoc::from_snapshot(&snapshot).expect("replica");
    replica.set_peer_id(999).expect("peer");
    assert_eq!(replica.plain_text().expect("text"), "shared a\nshared b");
    let (_, edit) = replica
        .edit(|d| d.insert_text(&[0], 0, "edited: "))
        .expect("edit in the editor");
    ws.synced_apply_update(sync_id, &edit).expect("apply");
    assert_eq!(
        PageDoc::from_snapshot(&ws.synced_open(sync_id).expect("open"))
            .expect("doc")
            .plain_text()
            .expect("text"),
        "edited: shared a\nshared b"
    );
    // Unsyncing one occurrence copies the content in place and leaves the rest.
    ws.synced_unsync(two, &[1]).expect("unsync");
    assert_eq!(
        texts(&mut ws, two),
        ["other page", "edited: shared a", "shared b"]
    );
    let info = ws.synced_list().expect("list");
    assert_eq!(
        (info.len(), info[0].occurrences, info[0].orphaned_at),
        (1, 1, None)
    );
    // Unsyncing the last occurrence deletes the shared document.
    ws.synced_unsync(one, &[1]).expect("unsync last");
    assert_eq!(
        texts(&mut ws, one),
        ["intro", "edited: shared a", "shared b", "outro"]
    );
    assert!(ws.synced_list().expect("list").is_empty());
    assert!(ws.synced_open(sync_id).is_err());
    let _ = clock;
}

#[test]
fn deleting_the_last_placeholder_leaves_an_orphan_that_is_collected_later() {
    let (_dir, mut ws, clock) = timed_workspace();
    let page = page_with(&mut ws, "Page", &paragraphs(&["keep", "shared"]));
    let (sync_id, _) = ws.synced_create(page, 1, 2).expect("create");
    ws.edit_page(page, |d| d.delete_block(&[1]))
        .expect("delete the placeholder");
    let info = ws.synced_list().expect("list");
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].occurrences, 0);
    let since = info[0].orphaned_at.expect("orphan clock running");
    // Not yet old enough.
    clock.advance(10 * 24 * 3_600_000);
    assert_eq!(ws.synced_gc(30 * 24 * 3_600_000).expect("gc"), 0);
    assert!(ws.synced_open(sync_id).is_ok(), "undo can still find it");
    // A page that shows it again stops the clock.
    ws.synced_insert(page, &[], 1, sync_id)
        .expect("insert again");
    assert_eq!(ws.synced_list().expect("list")[0].orphaned_at, None);
    ws.edit_page(page, |d| d.delete_block(&[1]))
        .expect("delete again");
    let again = ws.synced_list().expect("list")[0]
        .orphaned_at
        .expect("clock");
    assert!(again > since, "the clock restarted");
    clock.advance(31 * 24 * 3_600_000);
    assert_eq!(ws.synced_gc(30 * 24 * 3_600_000).expect("gc"), 1);
    assert!(ws.synced_list().expect("list").is_empty());
}

#[test]
fn a_page_in_the_trash_still_counts_as_an_occurrence() {
    let (_dir, mut ws, clock) = timed_workspace();
    let page = page_with(&mut ws, "Page", &paragraphs(&["a", "shared"]));
    let other = page_with(&mut ws, "Other", &paragraphs(&["b"]));
    let (sync_id, _) = ws.synced_create(page, 1, 2).expect("create");
    ws.synced_insert(other, &[], 1, sync_id).expect("insert");
    ws.trash_node(other).expect("trash");
    let occurrences = ws.synced_occurrences(sync_id).expect("occurrences");
    assert_eq!(occurrences.len(), 2);
    assert!(occurrences.iter().any(|o| o.trashed));
    ws.edit_page(page, |d| d.delete_block(&[1]))
        .expect("delete");
    clock.advance(40 * 24 * 3_600_000);
    assert_eq!(
        ws.synced_gc(30 * 24 * 3_600_000).expect("gc"),
        0,
        "the trashed page still shows it"
    );
    ws.restore(other).expect("restore");
    assert_eq!(texts(&mut ws, other).len(), 2);
}

#[test]
fn synced_blocks_cannot_nest_or_be_selected_twice() {
    let (_dir, mut ws, _clock) = timed_workspace();
    let page = page_with(&mut ws, "Page", &paragraphs(&["a", "b", "c"]));
    let (sync_id, _) = ws.synced_create(page, 0, 2).expect("create");
    assert!(
        ws.synced_create(page, 0, 2).is_err(),
        "the placeholder is in the selection"
    );
    assert!(ws.synced_create(page, 5, 9).is_err());
    assert!(
        ws.synced_insert(page, &[], 0, NodeId::fresh_document())
            .is_err(),
        "unknown shared document"
    );
    assert!(ws.synced_open(NodeId::fresh_document()).is_err());
    assert!(ws.synced_unsync(page, &[1]).is_err(), "not a placeholder");
    let _ = sync_id;
}

#[test]
fn synced_blocks_converge_across_devices() {
    let (_da, mut a, _ca) = timed_workspace();
    let (_db, mut b, _cb) = timed_workspace();
    let page = page_with(&mut a, "Page", &paragraphs(&["keep", "shared text"]));
    let (sync_id, _) = a.synced_create(page, 1, 2).expect("create");
    deliver(&mut a, &mut b);
    assert_eq!(
        b.synced_occurrences(sync_id).expect("occurrences").len(),
        1,
        "the reference index is built on arrival"
    );
    // Both devices edit the shared text concurrently; both edits survive.
    let edit = |ws: &mut Workspace, peer: u64, text: &str, offset: usize| {
        let replica =
            PageDoc::from_snapshot(&ws.synced_open(sync_id).expect("open")).expect("replica");
        replica.set_peer_id(peer).expect("peer");
        let (_, update) = replica
            .edit(|d| d.insert_text(&[0], offset, text))
            .expect("edit");
        ws.synced_apply_update(sync_id, &update).expect("apply");
    };
    edit(&mut a, 111, "A:", 0);
    edit(&mut b, 222, ":B", 11);
    deliver(&mut a, &mut b);
    deliver(&mut b, &mut a);
    let read = |ws: &mut Workspace| {
        PageDoc::from_snapshot(&ws.synced_open(sync_id).expect("open"))
            .expect("doc")
            .plain_text()
            .expect("text")
    };
    assert_eq!(read(&mut a), read(&mut b));
    assert_eq!(read(&mut a), "A:shared text:B");
}

// --------------------------------------------------------- structured import --

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    fs::write(path, content).expect("write");
}

fn id32(n: u8) -> String {
    format!("{n:032x}")
}

fn export_fixture(root: &Path) {
    let (home, tasks, spec, people, ann) = (id32(1), id32(2), id32(3), id32(4), id32(5));
    write(
        root,
        &format!("Home {home}.md"),
        &format!(
            "# Home\n\nSee [the tasks](Tasks%20{tasks}.csv) and [Ann](People/Ann%20{ann}.md).\n\n![pic](Home%20{home}/pic.png)\n"
        ),
    );
    write(root, &format!("Home {home}/pic.png"), "IMAGE");
    write(
        root,
        &format!("Tasks {tasks}.csv"),
        &format!(
            "Name,Status,Hours,Owner\nWrite spec,Todo,3,Ann (People/Ann%20{ann}.md)\nShip,Done,5,\nTest,Todo,2,\nDocs,Done,1,\n"
        ),
    );
    write(
        root,
        &format!("Tasks/Write spec {spec}.md"),
        "# Write spec\n\nOwner: Ann\n\nThe body with đường.\n",
    );
    write(root, &format!("People {people}.csv"), "Name\nAnn\n");
    write(root, &format!("People/Ann {ann}.md"), "# Ann\n\nBio.\n");
}

#[test]
fn a_structured_import_creates_pages_databases_and_syncs_them() {
    let (_dir, mut a) = new_workspace();
    let export = tempfile::tempdir().expect("export");
    export_fixture(export.path());
    let report = a
        .import_structured_dir(None, export.path())
        .expect("import");
    assert_eq!(
        (
            report.databases,
            report.rows,
            report.pages,
            report.attachments
        ),
        (2, 5, 1, 1)
    );
    let top: Vec<(String, NodeKind)> = a
        .children(None)
        .expect("top")
        .into_iter()
        .map(|n| (n.title, n.kind))
        .collect();
    assert!(top.contains(&("Tasks".to_owned(), NodeKind::Database)));
    assert!(top.contains(&("Home".to_owned(), NodeKind::Page)));
    let tasks = a
        .children(None)
        .expect("top")
        .into_iter()
        .find(|n| n.title == "Tasks")
        .expect("tasks")
        .id;
    let schema = a.database_schema(tasks).expect("schema");
    let types: Vec<_> = schema
        .properties
        .iter()
        .map(|p| (p.name.clone(), p.ty))
        .collect();
    assert_eq!(
        types,
        [
            ("Status".to_owned(), PropertyType::Select),
            ("Hours".to_owned(), PropertyType::Number),
            ("Owner".to_owned(), PropertyType::Relation),
        ]
    );
    let result = a
        .database_query(tasks, &QuerySpec::default())
        .expect("query");
    assert_eq!(result.total, 4);
    let spec = result
        .rows
        .iter()
        .find(|r| r.title == "Write spec")
        .expect("row");
    assert_eq!(
        spec.values[&schema.properties[1].id],
        CellValue::Number(3.0)
    );
    assert_eq!(result.titles.values().collect::<Vec<_>>(), ["Ann"]);
    // Row content and cell text are searchable; attachments are stored.
    assert_eq!(a.search("duong", 5).expect("search").len(), 1);
    assert_eq!(a.attachments().expect("attachments").len(), 1);
    let home = a
        .children(None)
        .expect("top")
        .into_iter()
        .find(|n| n.title == "Home")
        .expect("home")
        .id;
    let links = a
        .open_page(home)
        .expect("open")
        .summary()
        .expect("summary")
        .links;
    assert_eq!(links.len(), 2, "the links were rewritten to pages");
    // The index equals a rebuild, and another device receives everything.
    let incremental = a.dump_derived_indexes().expect("dump");
    a.rebuild_indexes().expect("rebuild");
    assert_eq!(incremental, a.dump_derived_indexes().expect("dump"));
    let (_db, mut b) = new_workspace();
    deliver(&mut a, &mut b);
    assert_eq!(
        b.database_query(tasks, &QuerySpec::default())
            .expect("query")
            .total,
        4
    );
    assert!(
        texts(&mut b, home).iter().any(|t| t.contains("See")),
        "the page content arrived"
    );
    assert!(
        a.synced_documents()
            .expect("docs")
            .contains(&DocRef::Page(tasks))
    );
}

#[test]
fn a_failed_structured_import_keeps_nothing() {
    let (_dir, mut ws) = new_workspace();
    let missing = tempfile::tempdir()
        .expect("tempdir")
        .path()
        .join("not here");
    assert!(ws.import_structured_dir(None, &missing).is_err());
    assert!(ws.children(None).expect("top").is_empty());
}

#[test]
fn a_markdown_export_writes_database_rows_as_pages_in_a_folder() {
    let (_dir, mut ws) = new_workspace();
    let db = ws
        .create_database(None, Position::End, "Tasks")
        .expect("db");
    let row = ws
        .database_create_row(db, "First row", Position::End, &BTreeMap::new())
        .expect("row");
    fill_page(&mut ws, row, &paragraphs(&["row body"]));
    let out = tempfile::tempdir().expect("out");
    let report = ws.export_markdown(None, out.path()).expect("export");
    assert_eq!(report.pages, 1);
    let written =
        fs::read_to_string(out.path().join("Tasks").join("First row.md")).expect("row file");
    assert!(written.contains("row body"), "{written}");
}
