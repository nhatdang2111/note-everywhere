//! Schema creation and migration on open.

mod store_support;

use std::fs;

use ne_store::{Error, SCHEMA_VERSION, Workspace};
use rusqlite::Connection;
use store_support::*;

fn tables(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare");
    statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
}

#[test]
fn an_empty_file_is_migrated_to_the_current_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A zero-byte file is what an interrupted first start can leave behind.
    fs::write(dir.path().join(ne_store::DB_FILE), b"").expect("empty file");
    let ws = Workspace::create(dir.path(), config()).expect("create over an empty file");
    assert_eq!(ws.schema_version().expect("version"), SCHEMA_VERSION);
    ws.check_integrity().expect("sound");
    drop(ws);

    let conn = Connection::open(dir.path().join(ne_store::DB_FILE)).expect("open");
    let names = tables(&conn);
    for expected in [
        "attachment_refs",
        "attachments",
        "docs",
        "links",
        "nodes",
        "outbox",
        "page_text",
        "search",
        "settings",
        "sync_meta",
        "updates",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "table {expected} is missing: {names:?}"
        );
    }
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("mode");
    assert_eq!(mode, "wal");
}

#[test]
fn opening_twice_does_not_migrate_again() {
    let (dir, ws) = new_workspace();
    drop(ws);
    for _ in 0..3 {
        let ws = reopen(dir.path());
        assert_eq!(ws.schema_version().expect("version"), SCHEMA_VERSION);
    }
}

#[test]
fn a_database_from_a_newer_version_is_refused_and_left_alone() {
    let (dir, ws) = new_workspace();
    drop(ws);
    let path = dir.path().join(ne_store::DB_FILE);
    {
        let conn = Connection::open(&path).expect("open");
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("bump");
    }
    match Workspace::open(dir.path(), config()) {
        Err(Error::SchemaTooNew { found, supported }) => {
            assert_eq!((found, supported), (SCHEMA_VERSION + 1, SCHEMA_VERSION));
        }
        other => panic!("expected SchemaTooNew, got {:?}", other.map(|_| ())),
    }
    let conn = Connection::open(&path).expect("open");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, SCHEMA_VERSION + 1, "the file was not touched");
}

#[test]
fn a_directory_without_a_tree_document_is_not_a_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Schema present, but creation was interrupted before the tree was stored.
    drop(Workspace::create(dir.path(), config()).expect("create"));
    {
        let conn = Connection::open(dir.path().join(ne_store::DB_FILE)).expect("open");
        conn.execute("DELETE FROM docs", []).expect("delete");
    }
    assert!(matches!(
        Workspace::open(dir.path(), config()),
        Err(Error::NoWorkspace(_))
    ));
    Workspace::create(dir.path(), config()).expect("creation can be retried");
}

#[test]
fn fts_and_wal_features_are_available_in_the_bundled_sqlite() {
    let conn = Connection::open_in_memory().expect("memory db");
    conn.execute_batch(
        "CREATE VIRTUAL TABLE t USING fts5(a, content='', contentless_delete=1, tokenize='trigram');
         INSERT INTO t(rowid, a) VALUES (1, 'xin chao');",
    )
    .expect("fts5 trigram contentless");
    let hits: i64 = conn
        .query_row("SELECT count(*) FROM t WHERE t MATCH '\"hao\"'", [], |r| {
            r.get(0)
        })
        .expect("match");
    assert_eq!(hits, 1);
}
