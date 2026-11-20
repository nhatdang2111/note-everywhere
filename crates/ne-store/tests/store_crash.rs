//! Crash safety: killing the process mid-write, and transactions that fail
//! half-way, must neither corrupt the database nor lose acknowledged data.

mod store_support;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use ne_core::tree::Position;
use ne_store::{Durability, StoreConfig, Workspace};
use rusqlite::Connection;
use store_support::*;

const CHILD_DIR_VAR: &str = "NE_CRASH_CHILD_DIR";
const CHILD_START_VAR: &str = "NE_CRASH_CHILD_START";

fn durable_config() -> StoreConfig {
    StoreConfig {
        durability: Durability::Full,
        clock: Arc::new(ne_core::tree::SystemClock),
        ..StoreConfig::default()
    }
}

/// The child half of the kill test. It only does something when the parent
/// started it with the directory in the environment; as an ignored test it
/// never runs by itself.
#[test]
#[ignore = "run by the kill test in a child process"]
fn crash_child_writer() {
    let Some(dir) = std::env::var_os(CHILD_DIR_VAR) else {
        return;
    };
    let mut ws = Workspace::open_or_create(Path::new(&dir), durable_config()).expect("open");
    let start: usize = std::env::var(CHILD_START_VAR)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    for i in start..usize::MAX {
        let id = ws
            .create_page(None, Position::End, &format!("Trang {i}"))
            .expect("create page");
        ws.edit_page(id, |doc| {
            doc.insert_text(&[0], 0, &format!("dấu vết số {i} qzx{i}q"))
        })
        .expect("edit page");
        // Acknowledged: both writes are committed. The parent may kill us any
        // time after reading this line, including in the middle of the next write.
        println!("ACK {i} {id}");
    }
}

/// Starts the child on `dir` numbering its pages from `start`, waits until it
/// acknowledged `stop_after` writes, then kills it without warning. Returns the acknowledged `(i, id)`.
fn run_and_kill(dir: &Path, start: usize, stop_after: usize) -> Vec<(usize, String)> {
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--ignored",
            "--exact",
            "crash_child_writer",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR_VAR, dir)
        .env(CHILD_START_VAR, start.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn child");
    let stdout = child.stdout.take().expect("child stdout");
    let mut acked = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("read child output");
        let mut parts = line.split(' ');
        if parts.next() == Some("ACK") {
            let i = parts.next().and_then(|n| n.parse().ok()).expect("index");
            acked.push((i, parts.next().expect("id").to_owned()));
            if acked.len() >= stop_after {
                break;
            }
        }
    }
    child.kill().expect("kill child");
    child.wait().expect("reap child");
    assert!(
        acked.len() >= stop_after,
        "the child stopped early: {acked:?}"
    );
    acked
}

fn verify_after_crash(dir: &Path, acked: &[(usize, String)]) {
    let mut ws = Workspace::open(dir, durable_config()).expect("reopen after the kill");
    ws.check_integrity().expect("the database file is sound");
    for (i, id) in acked {
        let id = id.parse().expect("node id");
        assert_eq!(
            page_text(&mut ws, id),
            format!("dấu vết số {i} qzx{i}q"),
            "acknowledged write {i} must survive"
        );
        let hits = ws.search(&format!("qzx{i}q"), 3).expect("search");
        assert_eq!(hits[0].id, id, "query qzx{i}q gave {hits:#?}");
        assert_eq!(
            ws.node(id).expect("node").expect("indexed").title,
            format!("Trang {i}")
        );
    }
    // Whatever the interrupted write left must be all-or-nothing: derived data
    // equals a recomputation, and every queued update belongs to stored data.
    let before = ws.dump_derived_indexes().expect("dump");
    ws.rebuild_indexes().expect("rebuild");
    assert_eq!(
        before,
        ws.dump_derived_indexes().expect("dump"),
        "no half-updated index"
    );
    let sidebar_ids: Vec<_> = ws
        .sidebar()
        .expect("sidebar")
        .iter()
        .map(|r| r.id)
        .collect();
    let tree_ids: Vec<_> = ws
        .tree()
        .snapshot()
        .expect("snapshot")
        .iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(sidebar_ids.len(), tree_ids.len());
}

#[test]
fn killing_the_process_between_and_during_writes_loses_nothing_acknowledged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut all = Vec::new();
    for round in 0..3 {
        // The first round creates the workspace inside the child; later rounds
        // resume with the write-ahead log the kill left behind.
        let acked = run_and_kill(dir.path(), round * 1000, 8 + round * 9);
        verify_after_crash(dir.path(), &acked);
        all.extend(acked);
    }
    verify_after_crash(dir.path(), &all);
}

fn raw(dir: &Path) -> Connection {
    Connection::open(dir.join(ne_store::DB_FILE)).expect("raw connection")
}

#[test]
fn a_transaction_that_fails_half_way_changes_nothing_and_memory_rolls_back() {
    let (dir, mut ws) = new_workspace();
    let page = page_with(&mut ws, "Trang", &paragraphs(&["bản gốc"]));
    let target = page_with(&mut ws, "Đích", &paragraphs(&["x"]));
    let queued = ws.outbox_len().expect("len");
    let log_before = ws
        .update_log_size(&ne_store::DocRef::Page(page))
        .expect("size");

    // Make the link index reject inserts, so a page edit that adds a link
    // fails after its update row was already written in the transaction.
    raw(dir.path())
        .execute_batch(
            "CREATE TRIGGER reject_links BEFORE INSERT ON links
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .expect("trigger");
    let link = ne_core::page::Block::new(ne_core::page::BlockKind::PageLink {
        page_id: target.to_string(),
        title: String::new(),
    });
    let result = ws.edit_page(page, |d| {
        d.insert_text(&[0], 0, "chưa lưu ")?;
        d.append_block(&[], &link).map(|_| ())
    });
    assert!(result.is_err());
    assert_eq!(ws.outbox_len().expect("len"), queued, "nothing was queued");
    assert_eq!(
        ws.update_log_size(&ne_store::DocRef::Page(page))
            .expect("size"),
        log_before
    );
    assert_eq!(
        page_text(&mut ws, page),
        "bản gốc",
        "memory was reloaded from storage"
    );
    assert!(ws.search("chua luu", 5).expect("search").is_empty());

    // The same failure inside a tree operation.
    raw(dir.path())
        .execute_batch(
            "CREATE TRIGGER reject_titles BEFORE INSERT ON nodes WHEN NEW.title = 'cấm'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .expect("trigger");
    let nodes_before = ws.tree().snapshot().expect("snapshot");
    assert!(ws.create_folder(None, Position::End, "cấm").is_err());
    assert_eq!(
        ws.tree().snapshot().expect("snapshot"),
        nodes_before,
        "the tree was reloaded"
    );
    assert_eq!(ws.outbox_len().expect("len"), queued);
    assert_eq!(ws.sidebar().expect("sidebar").len(), nodes_before.len());

    // Once the fault is gone the workspace keeps working and nothing leaked.
    raw(dir.path())
        .execute_batch("DROP TRIGGER reject_links; DROP TRIGGER reject_titles;")
        .expect("drop triggers");
    ws.edit_page(page, |d| d.insert_text(&[0], 0, "đã lưu "))
        .expect("edit works again");
    ws.create_folder(None, Position::End, "cấm")
        .expect("folder works again");
    assert_eq!(page_text(&mut ws, page), "đã lưu bản gốc");
    let dump = ws.dump_derived_indexes().expect("dump");
    ws.rebuild_indexes().expect("rebuild");
    assert_eq!(dump, ws.dump_derived_indexes().expect("dump"));
    drop(ws);
    let mut ws = reopen(dir.path());
    assert_eq!(page_text(&mut ws, page), "đã lưu bản gốc");
}

#[test]
fn an_uncommitted_transaction_from_a_dead_writer_is_rolled_back_on_reopen() {
    let (dir, mut ws) = new_workspace();
    let page = page_with(&mut ws, "Trang", &paragraphs(&["đã ghi"]));
    drop(ws);
    {
        // A writer that began changing the file and never committed.
        let conn = raw(dir.path());
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             BEGIN IMMEDIATE;
             UPDATE docs SET snapshot = x'00', version = x'00';
             DELETE FROM updates;
             INSERT INTO outbox (client_id, doc_id, kind, bytes, created_at)
                 VALUES ('ghost', 'tree', 'tree', x'00', 0);",
        )
        .expect("partial write");
        // Dropping the connection with the transaction open aborts it.
    }
    let mut ws = reopen(dir.path());
    ws.check_integrity().expect("sound");
    assert_eq!(page_text(&mut ws, page), "đã ghi");
    assert!(
        ws.outbox_peek(100)
            .expect("peek")
            .iter()
            .all(|i| i.client_id != "ghost")
    );
}
