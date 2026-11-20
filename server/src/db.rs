//! Thin wrapper over the Durable Object's SQLite storage: schema, typed row
//! access and the row counters the request-budget test reads.

use std::cell::Cell;

use worker::{Error, Result, SqlStorage, SqlStorageValue};

pub type Row = Vec<SqlStorageValue>;
pub type Params = Vec<SqlStorageValue>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE,
    label TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS throttle (
    key TEXT PRIMARY KEY,
    failures INTEGER NOT NULL,
    locked_until INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS updates (
    seq INTEGER PRIMARY KEY,
    client_id BLOB NOT NULL UNIQUE,
    doc_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    kind INTEGER NOT NULL,
    upto INTEGER NOT NULL,
    payload BLOB,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS updates_doc ON updates (doc_id, seq);
CREATE INDEX IF NOT EXISTS updates_tombstones ON updates (at) WHERE payload IS NULL;
CREATE TABLE IF NOT EXISTS parts (
    grp BLOB NOT NULL,
    idx INTEGER NOT NULL,
    payload BLOB NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (grp, idx)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS blobs (
    hash TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    state INTEGER NOT NULL,
    created_at INTEGER NOT NULL
) WITHOUT ROWID;
";

/// Tables wiped by the test-only reset route.
pub const TABLES: [&str; 6] = ["meta", "sessions", "throttle", "updates", "parts", "blobs"];

pub struct Db {
    sql: SqlStorage,
    written: Cell<u64>,
    read: Cell<u64>,
}

fn unexpected(what: &str) -> Error {
    Error::RustError(format!("unexpected column: {what}"))
}

impl Db {
    pub fn open(sql: SqlStorage) -> Result<Self> {
        sql.exec(SCHEMA, None)?;
        let db = Self {
            sql,
            written: Cell::new(0),
            read: Cell::new(0),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Brings a database created by an older version up to the current schema.
    fn migrate(&self) -> Result<()> {
        let has_last_seen = self.one(
            "SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'last_seen_at'",
            vec![],
        )?;
        if has_last_seen.is_none() {
            self.execute(
                "ALTER TABLE sessions ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0",
                vec![],
            )?;
        }
        self.take_counters();
        Ok(())
    }

    /// Runs a statement and collects every row.
    pub fn query(&self, query: &str, params: Params) -> Result<Vec<Row>> {
        let mut rows = Vec::new();
        self.query_while(query, params, |row| {
            rows.push(row);
            true
        })?;
        Ok(rows)
    }

    /// Runs a statement and feeds rows to `visit` until it returns `false`, so
    /// a large result is never read past what the caller needs.
    pub fn query_while(
        &self,
        query: &str,
        params: Params,
        mut visit: impl FnMut(Row) -> bool,
    ) -> Result<()> {
        let cursor = self.sql.exec(query, params)?;
        for row in cursor.raw() {
            if !visit(row?) {
                break;
            }
        }
        self.written
            .set(self.written.get() + cursor.rows_written() as u64);
        self.read.set(self.read.get() + cursor.rows_read() as u64);
        Ok(())
    }

    pub fn execute(&self, query: &str, params: Params) -> Result<()> {
        self.query(query, params).map(drop)
    }

    pub fn one(&self, query: &str, params: Params) -> Result<Option<Row>> {
        Ok(self.query(query, params)?.into_iter().next())
    }

    /// Rows written and read since the last call; used by the test counters.
    pub fn take_counters(&self) -> (u64, u64) {
        (self.written.replace(0), self.read.replace(0))
    }
}

pub fn int(row: &Row, index: usize) -> Result<i64> {
    match row.get(index) {
        Some(SqlStorageValue::Integer(v)) => Ok(*v),
        Some(SqlStorageValue::Float(v)) => Ok(*v as i64),
        _ => Err(unexpected("integer")),
    }
}

pub fn uint(row: &Row, index: usize) -> Result<u64> {
    u64::try_from(int(row, index)?).map_err(|_| unexpected("unsigned"))
}

pub fn text(row: &Row, index: usize) -> Result<String> {
    match row.get(index) {
        Some(SqlStorageValue::String(v)) => Ok(v.clone()),
        _ => Err(unexpected("text")),
    }
}

pub fn blob(row: &Row, index: usize) -> Result<Vec<u8>> {
    match row.get(index) {
        Some(SqlStorageValue::Blob(v)) => Ok(v.clone()),
        _ => Err(unexpected("blob")),
    }
}

/// Integer parameters as the SQL layer wants them.
pub fn n(value: u64) -> SqlStorageValue {
    SqlStorageValue::Integer(i64::try_from(value).unwrap_or(i64::MAX))
}

pub fn s(value: &str) -> SqlStorageValue {
    SqlStorageValue::String(value.to_owned())
}

pub fn b(value: &[u8]) -> SqlStorageValue {
    SqlStorageValue::Blob(value.to_vec())
}

// Meta keys.
pub const META_USERNAME: &str = "username";
pub const META_ACCOUNT: &str = "account";
pub const META_WORKSPACE: &str = "workspace_id";
pub const META_SEED: &str = "seed";
pub const META_VAULT: &str = "vault";
pub const META_STATS: &str = "stats";

impl Db {
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        self.one("SELECT value FROM meta WHERE key = ?", vec![s(key)])?
            .map(|row| text(&row, 0))
            .transpose()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            vec![s(key), s(value)],
        )
    }
}
