//! The durable sync outbox, the sync cursor and key-value settings.

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::docs::{DocKind, DocRef};
use crate::error::{Error, Result};

/// Key under which the sync cursor is kept in `sync_meta`.
const CURSOR_KEY: &str = "cursor";

/// One update waiting to be sent to the sync server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    /// Position in the queue; items are sent in ascending order.
    pub seq: i64,
    /// Identifier generated when the update was queued. It never changes, so
    /// the server can drop a repeated delivery of the same update.
    pub client_id: String,
    /// The document the update belongs to.
    pub doc: DocRef,
    /// The update bytes, as produced by the CRDT engine.
    pub bytes: Vec<u8>,
    /// When the update was queued, in milliseconds since the Unix epoch.
    pub created_at: i64,
}

/// Queues an update and returns its client id.
pub(crate) fn enqueue(conn: &Connection, doc: &DocRef, bytes: &[u8], now: i64) -> Result<String> {
    let client_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO outbox (client_id, doc_id, kind, bytes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![client_id, doc.id(), doc.kind().as_str(), bytes, now],
    )?;
    Ok(client_id)
}

/// The oldest `limit` queued updates, without removing them.
pub(crate) fn peek(conn: &Connection, limit: usize) -> Result<Vec<OutboxItem>> {
    let mut statement = conn.prepare_cached(
        "SELECT seq, client_id, doc_id, kind, bytes, created_at
         FROM outbox ORDER BY seq LIMIT ?1",
    )?;
    let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    let mut items = Vec::new();
    for row in rows {
        let (seq, client_id, doc_id, kind, bytes, created_at) = row?;
        let kind =
            DocKind::parse(&kind).ok_or_else(|| Error::Corrupt(format!("outbox kind {kind:?}")))?;
        items.push(OutboxItem {
            seq,
            client_id,
            doc: DocRef::from_parts(kind, &doc_id)?,
            bytes,
            created_at,
        });
    }
    Ok(items)
}

/// Removes acknowledged updates. Unknown ids are ignored, so a repeated
/// acknowledgement is harmless. Returns how many were removed.
pub(crate) fn acknowledge(conn: &Connection, client_ids: &[String]) -> Result<usize> {
    let mut statement = conn.prepare_cached("DELETE FROM outbox WHERE client_id = ?1")?;
    let mut removed = 0;
    for id in client_ids {
        removed += statement.execute([id])?;
    }
    Ok(removed)
}

/// Number of updates waiting.
pub(crate) fn len(conn: &Connection) -> Result<usize> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))?;
    Ok(usize::try_from(count).unwrap_or_default())
}

pub(crate) fn cursor(conn: &Connection) -> Result<Option<Vec<u8>>> {
    meta(conn, CURSOR_KEY)
}

pub(crate) fn set_cursor(conn: &Connection, cursor: &[u8]) -> Result<()> {
    set_meta(conn, CURSOR_KEY, cursor)
}

pub(crate) fn meta(conn: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row("SELECT value FROM sync_meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

pub(crate) fn set_meta(conn: &Connection, key: &str, value: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

pub(crate) fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn remove_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}
