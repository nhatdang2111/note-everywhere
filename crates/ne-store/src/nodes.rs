//! The derived node index: a SQLite mirror of the workspace tree for fast
//! sidebar queries, kept in step incrementally from the tree's change log.

use std::collections::{BTreeSet, HashMap};

use ne_core::NodeId;
use ne_core::tree::{NodeInfo, NodeKind, WorkspaceTree};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::search::fold;

/// One node of the sidebar index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    /// Stable id.
    pub id: NodeId,
    /// Parent node, `None` at the top level.
    pub parent: Option<NodeId>,
    /// Index among all siblings, trashed ones included. Visible siblings keep
    /// their relative order, so sorting by it is enough.
    pub position: usize,
    /// Folder or page.
    pub kind: NodeKind,
    /// Display title.
    pub title: String,
    /// Optional icon.
    pub icon: Option<String>,
    /// Pinned as a favourite.
    pub favorite: bool,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at: i64,
    /// Time of the last change to the node's own metadata or placement.
    pub updated_at: i64,
    /// Set when the node itself is in the trash.
    pub trashed_at: Option<i64>,
    /// True when the node or one of its ancestors is in the trash.
    pub hidden: bool,
}

/// What an incremental sync of the index changed, for the caller to follow up.
#[derive(Debug, Default)]
pub(crate) struct SyncOutcome {
    /// Nodes that no longer exist (their rows are gone), with the internal row
    /// id they had, which keyed their search entry.
    pub removed: Vec<(NodeId, i64)>,
    /// Nodes that are new or were retitled: their search entry needs a refresh.
    pub reindex: Vec<NodeId>,
    /// Nodes that are new.
    pub added: Vec<NodeId>,
}

const COLUMNS: &str = "id, parent_id, position, kind, title, icon, favorite, created_at, \
                       updated_at, trashed_at, hidden, in_database";

fn parse_id(text: &str) -> Result<NodeId> {
    text.parse()
        .map_err(|_| Error::Corrupt(format!("node id {text:?} in the index")))
}

fn read_row(row: &Row<'_>) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        id: row.get(0)?,
        parent: row.get(1)?,
        position: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        icon: row.get(5)?,
        favorite: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        trashed_at: row.get(9)?,
        hidden: row.get(10)?,
        in_database: row.get(11)?,
    })
}

struct RawRow {
    id: String,
    parent: Option<String>,
    position: i64,
    kind: String,
    title: String,
    icon: Option<String>,
    favorite: bool,
    created_at: i64,
    updated_at: i64,
    trashed_at: Option<i64>,
    hidden: bool,
    in_database: bool,
}

impl RawRow {
    fn into_row(self) -> Result<NodeRow> {
        Ok(NodeRow {
            id: parse_id(&self.id)?,
            parent: self.parent.as_deref().map(parse_id).transpose()?,
            position: usize::try_from(self.position).unwrap_or_default(),
            kind: NodeKind::parse(&self.kind).unwrap_or(NodeKind::Folder),
            title: self.title,
            icon: self.icon,
            favorite: self.favorite,
            created_at: self.created_at,
            updated_at: self.updated_at,
            trashed_at: self.trashed_at,
            hidden: self.hidden,
        })
    }

    /// True when every column except `hidden` already holds what the tree says.
    fn matches(&self, info: &NodeInfo, position: usize, in_database: bool) -> bool {
        self.in_database == in_database
            && self.parent == info.parent.map(|p| p.to_string())
            && usize::try_from(self.position).ok() == Some(position)
            && self.kind == kind_str(info.kind)
            && self.title == info.title
            && self.icon == info.icon
            && self.favorite == info.favorite
            && self.created_at == info.created_at
            && self.updated_at == info.updated_at
            && self.trashed_at == info.trashed_at
    }
}

fn kind_str(kind: NodeKind) -> &'static str {
    kind.as_str()
}

/// True when the node is a row: a child of a database node.
fn is_row(tree: &WorkspaceTree, info: &NodeInfo) -> bool {
    info.parent
        .and_then(|p| tree.get(p).ok())
        .is_some_and(|parent| parent.kind == NodeKind::Database)
}

/// Inserts or updates the row of `info`, keeping its `rid` and `hidden`.
fn upsert(conn: &Connection, info: &NodeInfo, position: usize, in_database: bool) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO nodes (id, parent_id, position, kind, title, title_norm, icon, favorite,
                            created_at, updated_at, trashed_at, title_key, in_database)
         VALUES (?1, ?2, ?3, ?4, ?5, ?11, ?6, ?7, ?8, ?9, ?10, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
             parent_id = excluded.parent_id, position = excluded.position,
             kind = excluded.kind, title = excluded.title,
             title_norm = excluded.title_norm, icon = excluded.icon,
             favorite = excluded.favorite, created_at = excluded.created_at,
             updated_at = excluded.updated_at, trashed_at = excluded.trashed_at,
             title_key = excluded.title_key, in_database = excluded.in_database",
    )?
    .execute(params![
        info.id.to_string(),
        info.parent.map(|p| p.to_string()),
        i64::try_from(position).unwrap_or(i64::MAX),
        kind_str(info.kind),
        info.title,
        info.icon,
        info.favorite,
        info.created_at,
        info.updated_at,
        info.trashed_at,
        fold(&info.title),
        ne_core::collation::sort_key(&info.title),
        in_database,
    ])?;
    Ok(())
}

fn set_hidden(conn: &Connection, id: NodeId, hidden: bool) -> Result<()> {
    conn.prepare_cached("UPDATE nodes SET hidden = ?2 WHERE id = ?1 AND hidden != ?2")?
        .execute(params![id.to_string(), hidden])?;
    Ok(())
}

/// True when the node or any ancestor is in the trash.
fn is_hidden(tree: &WorkspaceTree, id: NodeId) -> Result<bool> {
    if tree.get(id)?.trashed_at.is_some() {
        return Ok(true);
    }
    Ok(tree.ancestors(id)?.iter().any(|a| a.trashed_at.is_some()))
}

/// Sets `hidden` on `root` and everything below it.
fn refresh_hidden(conn: &Connection, tree: &WorkspaceTree, root: NodeId) -> Result<()> {
    let root_hidden = is_hidden(tree, root)?;
    set_hidden(conn, root, root_hidden)?;
    let mut stack = vec![(root, root_hidden)];
    while let Some((id, hidden)) = stack.pop() {
        for child in tree.children(Some(id), true)? {
            let child_hidden = hidden || child.trashed_at.is_some();
            set_hidden(conn, child.id, child_hidden)?;
            stack.push((child.id, child_hidden));
        }
    }
    Ok(())
}

/// Rows below `id` (and `id` itself) according to the index.
fn indexed_subtree(conn: &Connection, id: NodeId) -> Result<Vec<NodeId>> {
    let mut statement = conn.prepare_cached(
        "WITH RECURSIVE sub(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN sub ON n.parent_id = sub.id
         )
         SELECT sub.id FROM sub JOIN nodes ON nodes.id = sub.id",
    )?;
    let ids: Vec<String> = statement
        .query_map([id.to_string()], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ids.iter().map(|s| parse_id(s)).collect()
}

struct Stored {
    parent: Option<String>,
}

fn stored(conn: &Connection, id: NodeId) -> Result<Option<Stored>> {
    Ok(conn
        .prepare_cached("SELECT parent_id FROM nodes WHERE id = ?1")?
        .query_row([id.to_string()], |row| {
            Ok(Stored {
                parent: row.get(0)?,
            })
        })
        .optional()?)
}

/// Brings the index in line with the tree after `changed` nodes were touched.
pub(crate) fn sync(
    conn: &Connection,
    tree: &WorkspaceTree,
    changed: &BTreeSet<NodeId>,
) -> Result<SyncOutcome> {
    let mut outcome = SyncOutcome::default();
    let mut affected: BTreeSet<Option<NodeId>> = BTreeSet::new();
    let mut removed_ids: Vec<(NodeId, i64)> = Vec::new();

    for &id in changed {
        if let Some(old) = stored(conn, id)? {
            affected.insert(old.parent.as_deref().map(parse_id).transpose()?);
        }
        if tree.contains(id) {
            affected.insert(tree.get(id)?.parent);
            affected.insert(Some(id));
        } else {
            for below in indexed_subtree(conn, id)? {
                if !tree.contains(below)
                    && let Some(rid) = rid_of(conn, below)?
                {
                    removed_ids.push((below, rid));
                }
            }
        }
    }
    for (id, _) in &removed_ids {
        conn.prepare_cached("DELETE FROM nodes WHERE id = ?1")?
            .execute([id.to_string()])?;
    }
    outcome.removed = removed_ids;

    let mut structural: Vec<(NodeId, bool)> = Vec::new();
    for parent in affected {
        if parent.is_some_and(|p| !tree.contains(p)) {
            continue;
        }
        let existing = rows_under(conn, parent)?;
        for (position, info) in tree.children(parent, true)?.into_iter().enumerate() {
            let key = info.id.to_string();
            let row = is_row(tree, &info);
            match existing.get(&key) {
                Some(raw) if raw.matches(&info, position, row) => {}
                Some(raw) => {
                    if raw.title != info.title {
                        outcome.reindex.push(info.id);
                    }
                    if raw.trashed_at != info.trashed_at {
                        structural.push((info.id, false));
                    }
                    upsert(conn, &info, position, row)?;
                }
                None => {
                    // Absent under this parent: new, or moved here from elsewhere.
                    let known = stored(conn, info.id)?.is_some();
                    if known {
                        structural.push((info.id, false));
                    } else {
                        outcome.added.push(info.id);
                        structural.push((info.id, true));
                    }
                    outcome.reindex.push(info.id);
                    upsert(conn, &info, position, row)?;
                }
            }
        }
    }
    for (id, is_new) in structural {
        if is_new {
            set_hidden(conn, id, is_hidden(tree, id)?)?;
        } else {
            refresh_hidden(conn, tree, id)?;
        }
    }
    Ok(outcome)
}

fn rows_under(conn: &Connection, parent: Option<NodeId>) -> Result<HashMap<String, RawRow>> {
    let parent = parent.map(|p| p.to_string());
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM nodes WHERE parent_id IS ?1"
    ))?;
    let rows = statement.query_map([parent], read_row)?;
    let mut map = HashMap::new();
    for row in rows {
        let row = row?;
        map.insert(row.id.clone(), row);
    }
    Ok(map)
}

/// Recomputes the whole index from the tree.
pub(crate) fn rebuild(conn: &Connection, tree: &WorkspaceTree) -> Result<()> {
    conn.execute("DELETE FROM nodes", [])?;
    let mut stack: Vec<(Option<NodeId>, bool)> = vec![(None, false)];
    while let Some((parent, parent_hidden)) = stack.pop() {
        for (position, info) in tree.children(parent, true)?.into_iter().enumerate() {
            upsert(conn, &info, position, is_row(tree, &info))?;
            let hidden = parent_hidden || info.trashed_at.is_some();
            set_hidden(conn, info.id, hidden)?;
            stack.push((Some(info.id), hidden));
        }
    }
    Ok(())
}

/// Every visible node, top level first and siblings in order, which is what
/// the sidebar renders. Rows of a database are not part of it: they are shown
/// by the database.
pub(crate) fn sidebar(conn: &Connection) -> Result<Vec<NodeRow>> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM nodes WHERE hidden = 0 AND in_database = 0
         ORDER BY parent_id IS NOT NULL, parent_id, position"
    ))?;
    let rows = statement.query_map([], read_row)?;
    rows.map(|raw| raw?.into_row()).collect()
}

/// Visible children of `parent` in order.
pub(crate) fn children(conn: &Connection, parent: Option<NodeId>) -> Result<Vec<NodeRow>> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM nodes WHERE hidden = 0 AND parent_id IS ?1 ORDER BY position"
    ))?;
    let rows = statement.query_map([parent.map(|p| p.to_string())], read_row)?;
    rows.map(|raw| raw?.into_row()).collect()
}

/// One node by id.
pub(crate) fn get(conn: &Connection, id: NodeId) -> Result<Option<NodeRow>> {
    let row = conn
        .prepare_cached(&format!("SELECT {COLUMNS} FROM nodes WHERE id = ?1"))?
        .query_row([id.to_string()], read_row)
        .optional()?;
    row.map(RawRow::into_row).transpose()
}

/// The internal row id of a node, which keys its search entry.
pub(crate) fn rid_of(conn: &Connection, id: NodeId) -> Result<Option<i64>> {
    Ok(conn
        .prepare_cached("SELECT rid FROM nodes WHERE id = ?1")?
        .query_row([id.to_string()], |row| row.get(0))
        .optional()?)
}

/// Computes the title sort key of nodes that were indexed before the key
/// existed.
pub(crate) fn backfill_title_keys(conn: &Connection) -> Result<()> {
    let missing: Vec<(i64, String)> = conn
        .prepare("SELECT rid, title FROM nodes WHERE title_key IS NULL")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut update = conn.prepare_cached("UPDATE nodes SET title_key = ?2 WHERE rid = ?1")?;
    for (rid, title) in missing {
        update.execute(params![rid, ne_core::collation::sort_key(&title)])?;
    }
    Ok(())
}
