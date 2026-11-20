//! CRDT document rows: snapshot, update log, compaction.

use std::fmt;

use ne_core::NodeId;
use ne_core::roles::Role;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

/// Id of the single workspace tree document.
pub(crate) const TREE_DOC_ID: &str = "tree";

/// The kind of a stored document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    /// The workspace tree.
    Tree,
    /// The content of one page.
    Page,
    /// A vault document. The store keeps it opaque and never indexes it.
    Vault,
}

impl DocKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Tree => "tree",
            Self::Page => "page",
            Self::Vault => "vault",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "tree" => Some(Self::Tree),
            "page" => Some(Self::Page),
            "vault" => Some(Self::Vault),
            _ => None,
        }
    }
}

/// Names one stored document, as the sync layer addresses it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocRef {
    /// The workspace tree.
    Tree,
    /// The content of the page with this node id.
    Page(NodeId),
    /// A vault document with this caller-chosen id.
    Vault(String),
}

impl DocRef {
    /// The kind of document this names.
    pub fn kind(&self) -> DocKind {
        match self {
            Self::Tree => DocKind::Tree,
            Self::Page(_) => DocKind::Page,
            Self::Vault(_) => DocKind::Vault,
        }
    }

    /// The stable string id used in storage and on the wire.
    pub fn id(&self) -> String {
        match self {
            Self::Tree => TREE_DOC_ID.to_owned(),
            Self::Page(id) => id.to_string(),
            Self::Vault(id) => id.clone(),
        }
    }

    /// Rebuilds a reference from the stored kind and id.
    pub fn from_parts(kind: DocKind, id: &str) -> Result<Self> {
        match kind {
            DocKind::Tree => Ok(Self::Tree),
            DocKind::Page => {
                Ok(Self::Page(id.parse().map_err(|_| {
                    Error::Corrupt(format!("page document id {id:?}"))
                })?))
            }
            DocKind::Vault => Ok(Self::Vault(id.to_owned())),
        }
    }
}

impl fmt::Display for DocRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tree => f.write_str("workspace tree"),
            Self::Page(id) => write!(f, "page {id}"),
            Self::Vault(id) => write!(f, "vault document {id}"),
        }
    }
}

/// A stored document: its snapshot and the updates logged after it.
pub(crate) struct StoredDoc {
    pub snapshot: Vec<u8>,
    pub version: Vec<u8>,
    pub updates: Vec<Vec<u8>>,
}

pub(crate) fn exists(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM docs WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

pub(crate) fn load(conn: &Connection, id: &str) -> Result<Option<StoredDoc>> {
    let row = conn
        .query_row(
            "SELECT snapshot, version FROM docs WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?;
    let Some((snapshot, version)) = row else {
        return Ok(None);
    };
    let mut statement =
        conn.prepare_cached("SELECT bytes FROM updates WHERE doc_id = ?1 ORDER BY seq")?;
    let updates = statement
        .query_map([id], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(StoredDoc {
        snapshot,
        version,
        updates,
    }))
}

pub(crate) fn insert(
    conn: &Connection,
    id: &str,
    kind: DocKind,
    snapshot: &[u8],
    version: &[u8],
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO docs (id, kind, snapshot, version, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, kind.as_str(), snapshot, version, now],
    )?;
    Ok(())
}

/// Appends an update to the log and records the document's new version.
pub(crate) fn append_update(
    conn: &Connection,
    id: &str,
    bytes: &[u8],
    version: &[u8],
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO updates (doc_id, bytes) VALUES (?1, ?2)",
        params![id, bytes],
    )?;
    let size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    conn.execute(
        "UPDATE docs SET version = ?2, update_count = update_count + 1,
                         update_bytes = update_bytes + ?3, updated_at = ?4
         WHERE id = ?1",
        params![id, version, size, now],
    )?;
    Ok(())
}

/// Update log size of a document: `(count, bytes)`.
pub(crate) fn log_size(conn: &Connection, id: &str) -> Result<(usize, usize)> {
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT update_count, update_bytes FROM docs WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((
        usize::try_from(count).unwrap_or_default(),
        usize::try_from(bytes).unwrap_or_default(),
    ))
}

/// Folds the update log into a new snapshot.
pub(crate) fn compact(
    conn: &Connection,
    id: &str,
    snapshot: &[u8],
    version: &[u8],
    now: i64,
) -> Result<()> {
    conn.execute("DELETE FROM updates WHERE doc_id = ?1", [id])?;
    conn.execute(
        "UPDATE docs SET snapshot = ?2, version = ?3, update_count = 0,
                         update_bytes = 0, updated_at = ?4
         WHERE id = ?1",
        params![id, snapshot, version, now],
    )?;
    Ok(())
}

/// Records what a stored document is besides a plain page.
pub(crate) fn set_role(conn: &Connection, id: &str, role: Role) -> Result<()> {
    conn.execute(
        "UPDATE docs SET role = ?2 WHERE id = ?1",
        params![id, role.as_str()],
    )?;
    Ok(())
}

/// The role of a stored document; `None` for a plain page or no document.
pub(crate) fn role(conn: &Connection, id: &str) -> Result<Option<Role>> {
    let text: Option<String> = conn
        .query_row("SELECT role FROM docs WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(text.as_deref().and_then(Role::parse))
}

/// Ids of the stored documents that have a role.
pub(crate) fn ids_with_roles(conn: &Connection) -> Result<Vec<(String, Role)>> {
    let mut statement =
        conn.prepare_cached("SELECT id, role FROM docs WHERE role != 'page' ORDER BY id")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, role)| Some((id, Role::parse(&role)?)))
        .collect())
}

pub(crate) fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM updates WHERE doc_id = ?1", [id])?;
    conn.execute("DELETE FROM docs WHERE id = ?1", [id])?;
    Ok(())
}

/// Ids of every stored document of a kind.
pub(crate) fn ids(conn: &Connection, kind: DocKind) -> Result<Vec<String>> {
    let mut statement = conn.prepare_cached("SELECT id FROM docs WHERE kind = ?1 ORDER BY id")?;
    let ids = statement
        .query_map([kind.as_str()], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}
