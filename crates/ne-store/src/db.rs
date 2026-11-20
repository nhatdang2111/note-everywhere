//! Opening the SQLite database: pragmas and schema migrations.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use crate::error::{Error, Result};

/// How hard a commit tries to survive a power cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    /// Every commit is synced (`synchronous = FULL`) before it returns, so
    /// acknowledged data survives a crash of the process. On macOS SQLite
    /// syncs with `fsync`, which the drive may still hold in its cache; a
    /// guarantee against sudden power loss there would also need
    /// `PRAGMA fullfsync`, which is not enabled.
    #[default]
    Full,
    /// Commits are synced at checkpoints only. A crash of the process loses
    /// nothing; a power loss can drop the most recent commits but never
    /// corrupts the database. Meant for bulk loading.
    Normal,
}

/// Migrations in order; entry `i` upgrades the schema from version `i` to
/// `i + 1`. Never edit an entry that has shipped, add a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_databases.sql"),
];

/// The schema version this build writes.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Opens (creating if absent) the database at `path`, configures it and
/// applies pending migrations.
pub(crate) fn open(path: &Path, durability: Durability) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    configure(&conn, durability)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection, durability: Durability) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    // `journal_mode` answers with the mode now in effect.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Corrupt(format!(
            "database refused write-ahead logging (journal mode {mode})"
        )));
    }
    let synchronous = match durability {
        Durability::Full => "FULL",
        Durability::Normal => "NORMAL",
    };
    conn.execute_batch(&format!(
        "PRAGMA synchronous = {synchronous};
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -32768;"
    ))?;
    Ok(())
}

/// The schema version recorded in the file (0 for a fresh database).
pub(crate) fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn migrate(conn: &mut Connection) -> Result<()> {
    // Immediate so that two processes opening a fresh file take turns; the
    // version is read again inside the lock because the other one may have
    // finished while this one waited.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found = schema_version(&tx)?;
    if found > SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    if found == SCHEMA_VERSION {
        return Ok(());
    }
    for sql in &MIGRATIONS[usize::try_from(found).unwrap_or_default()..] {
        tx.execute_batch(sql)?;
    }
    // Schema change and version bump commit together, or not at all.
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}
