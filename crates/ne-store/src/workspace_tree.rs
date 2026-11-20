//! Tree operations of the [`Workspace`]: each one edits the live tree and
//! persists the change, the derived indexes and the outbox in one transaction.

use std::collections::{BTreeMap, BTreeSet};

use ne_core::NodeId;
use ne_core::page::PageDoc;
use ne_core::tree::{Position, WorkspaceTree};
use rusqlite::Connection;

use crate::cache::PageCache;
use crate::db_index;
use crate::docs::{self, DocKind, DocRef, TREE_DOC_ID};
use crate::error::Result;
use crate::nodes;
use crate::outbox;
use crate::pages;
use crate::workspace::{Change, StoreConfig, Workspace, load_page, load_tree};

/// What applying tree changes to the indexes found.
pub(crate) struct TreeOutcome {
    pub removed: BTreeSet<NodeId>,
    /// Rows that were deleted, by the database whose cells still hold values
    /// for them.
    pub purged: BTreeMap<String, Vec<String>>,
}

/// Updates the node index, the search index and the page documents after the
/// tree changed. `explicit_removed` lists nodes the caller knows were deleted
/// (the index finds those it has rows for by itself).
pub(crate) fn apply_tree_changes(
    conn: &Connection,
    tree: &WorkspaceTree,
    cache: &mut PageCache,
    peer_id: u64,
    changed: &BTreeSet<NodeId>,
    explicit_removed: &[NodeId],
    now: i64,
) -> Result<TreeOutcome> {
    let outcome = nodes::sync(conn, tree, changed)?;
    let mut removed: BTreeSet<NodeId> = BTreeSet::new();
    let mut purged: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for &(id, rid) in &outcome.removed {
        note_purged(conn, id, &mut purged)?;
        pages::remove_page(conn, id, Some(rid), now)?;
        drop_document(conn, id)?;
        cache.remove(id);
        removed.insert(id);
    }
    for &id in explicit_removed {
        if removed.insert(id) {
            note_purged(conn, id, &mut purged)?;
            pages::remove_page(conn, id, None, now)?;
            drop_document(conn, id)?;
            cache.remove(id);
        }
    }
    let added: BTreeSet<NodeId> = outcome.added.iter().copied().collect();
    let reindex: BTreeSet<NodeId> = outcome.reindex.iter().copied().collect();
    for id in reindex {
        if !tree.contains(id) {
            continue;
        }
        let is_page = tree.get(id)?.kind == ne_core::tree::NodeKind::Page;
        if is_page && added.contains(&id) && docs::exists(conn, &id.to_string())? {
            // A page whose node just appeared may already have content (it
            // arrived first, or it was created in this very transaction).
            match cache.get(id) {
                Some(doc) => pages::index_page(conn, id, doc, now)?,
                None => match load_page(conn, id, peer_id)? {
                    Some(doc) => pages::index_page(conn, id, &doc, now)?,
                    None => pages::reindex_title(conn, id)?,
                },
            }
        } else {
            pages::reindex_title(conn, id)?;
        }
    }
    Ok(TreeOutcome { removed, purged })
}

fn note_purged(
    conn: &Connection,
    id: NodeId,
    purged: &mut BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let key = id.to_string();
    if let Some(db) = db_index::row_database(conn, &key)? {
        purged.entry(db).or_default().push(key);
    }
    Ok(())
}

/// Deletes the stored document of a node that is gone, with what is derived
/// from it: for a database the index of its cells, for a row the cells that
/// were indexed for it.
fn drop_document(conn: &Connection, id: NodeId) -> Result<()> {
    let key = id.to_string();
    db_index::remove_database(conn, &key)?;
    db_index::remove_row(conn, &key)?;
    docs::delete(conn, &key)
}

/// Folds the update log of a document into a new snapshot when it has grown
/// past the configured thresholds.
pub(crate) fn maybe_compact(
    conn: &Connection,
    config: &StoreConfig,
    id: &str,
    now: i64,
    state: impl FnOnce() -> Result<(Vec<u8>, Vec<u8>)>,
) -> Result<()> {
    let (count, bytes) = docs::log_size(conn, id)?;
    if count >= config.compact_after_updates || bytes >= config.compact_after_bytes {
        let (snapshot, version) = state()?;
        docs::compact(conn, id, &snapshot, &version, now)?;
    }
    Ok(())
}

impl Workspace {
    /// Reloads the tree from storage, discarding unsaved in-memory changes.
    pub(crate) fn recover_tree(&mut self) {
        self.pages.clear();
        if let Ok((tree, version)) = load_tree(&self.conn, self.config.clock.clone(), self.peer_id)
        {
            self.tree = tree;
            self.tree_version = version;
        }
    }

    /// Runs a tree operation, then persists what it changed. `extra` runs in
    /// the same transaction with the operation's result.
    pub(crate) fn local_tree_change<R>(
        &mut self,
        op: impl FnOnce(&WorkspaceTree) -> ne_core::Result<R>,
        removed: impl FnOnce(&R) -> Vec<NodeId>,
        extra: impl FnOnce(&Connection, &R, &mut PageCache) -> Result<()>,
    ) -> Result<R> {
        let value = match op(&self.tree) {
            Ok(value) => value,
            Err(err) => {
                // An operation that fails after editing leaves unsaved changes.
                if self.tree.version() != self.tree_version {
                    self.recover_tree();
                }
                return Err(err.into());
            }
        };
        match self.persist_local_tree(&value, &removed(&value), extra) {
            Ok(()) => Ok(value),
            Err(err) => {
                self.recover_tree();
                Err(err)
            }
        }
    }

    fn persist_local_tree<R>(
        &mut self,
        value: &R,
        removed: &[NodeId],
        extra: impl FnOnce(&Connection, &R, &mut PageCache) -> Result<()>,
    ) -> Result<()> {
        let now = self.now();
        let version = self.tree.version();
        let changed = self.tree.take_changes();
        let tx = self.conn.transaction()?;
        if version != self.tree_version {
            let update = self.tree.export_updates_since(&self.tree_version)?;
            docs::append_update(&tx, TREE_DOC_ID, &update, &version, now)?;
            outbox::enqueue(&tx, &DocRef::Tree, &update, now)?;
        }
        extra(&tx, value, &mut self.pages)?;
        let outcome = apply_tree_changes(
            &tx,
            &self.tree,
            &mut self.pages,
            self.peer_id,
            &changed,
            removed,
            now,
        )?;
        maybe_compact(&tx, &self.config, TREE_DOC_ID, now, || {
            Ok((self.tree.export_snapshot()?, version.clone()))
        })?;
        tx.commit()?;
        self.tree_version = version;
        for id in &outcome.removed {
            self.databases.remove(id);
        }
        self.clear_deleted_rows(&outcome.purged);
        Ok(())
    }

    /// Creates a folder under `parent` (`None` is the top level).
    pub fn create_folder(
        &mut self,
        parent: Option<NodeId>,
        position: Position,
        title: &str,
    ) -> Result<NodeId> {
        self.local_tree_change(
            |tree| tree.create_folder(parent, position, title),
            |_| Vec::new(),
            |_, _, _| Ok(()),
        )
    }

    /// Creates a page under `parent` together with its document: the empty
    /// skeleton page is stored and queued for sync as the page's first update
    /// in the same transaction as the tree node.
    pub fn create_page(
        &mut self,
        parent: Option<NodeId>,
        position: Position,
        title: &str,
    ) -> Result<NodeId> {
        let (peer_id, now) = (self.peer_id, self.now());
        let clock = self.config.clock.clone();
        self.local_tree_change(
            |tree| tree.create_page(parent, position, title),
            |_| Vec::new(),
            |conn, id, cache| {
                let doc = PageDoc::with_peer_id(peer_id)?.with_clock(clock);
                let key = id.to_string();
                docs::insert(
                    conn,
                    &key,
                    DocKind::Page,
                    &doc.export_snapshot()?,
                    &doc.version(),
                    now,
                )?;
                outbox::enqueue(conn, &DocRef::Page(*id), &doc.export_all_updates()?, now)?;
                cache.insert(*id, doc);
                Ok(())
            },
        )
    }

    /// Renames a node.
    pub fn rename(&mut self, id: NodeId, title: &str) -> Result<()> {
        self.local_tree_change(|t| t.rename(id, title), |_| Vec::new(), |_, _, _| Ok(()))
    }

    /// Sets or clears a node's icon.
    pub fn set_icon(&mut self, id: NodeId, icon: Option<&str>) -> Result<()> {
        self.local_tree_change(|t| t.set_icon(id, icon), |_| Vec::new(), |_, _, _| Ok(()))
    }

    /// Pins or unpins a node as a favourite.
    pub fn set_favorite(&mut self, id: NodeId, favorite: bool) -> Result<()> {
        self.local_tree_change(
            |t| t.set_favorite(id, favorite),
            |_| Vec::new(),
            |_, _, _| Ok(()),
        )
    }

    /// Moves a node under `new_parent` (`None` is the top level).
    pub fn move_node(
        &mut self,
        id: NodeId,
        new_parent: Option<NodeId>,
        position: Position,
    ) -> Result<()> {
        self.local_tree_change(
            |t| t.move_node(id, new_parent, position),
            |_| Vec::new(),
            |_, _, _| Ok(()),
        )
    }

    /// Moves a node among its siblings.
    pub fn reorder(&mut self, id: NodeId, position: Position) -> Result<()> {
        self.local_tree_change(
            |t| t.reorder(id, position),
            |_| Vec::new(),
            |_, _, _| Ok(()),
        )
    }

    /// Moves a node and its subtree to the trash.
    pub fn trash_node(&mut self, id: NodeId) -> Result<()> {
        self.local_tree_change(|t| t.trash_node(id), |_| Vec::new(), |_, _, _| Ok(()))
    }

    /// Takes a node out of the trash.
    pub fn restore(&mut self, id: NodeId) -> Result<()> {
        self.local_tree_change(|t| t.restore(id), |_| Vec::new(), |_, _, _| Ok(()))
    }

    /// Permanently deletes a trashed node and its subtree, together with the
    /// page documents, index entries and attachment references of every page
    /// in it. Returns the removed ids.
    pub fn delete_permanently(&mut self, id: NodeId) -> Result<Vec<NodeId>> {
        self.local_tree_change(|t| t.delete_permanently(id), Clone::clone, |_, _, _| Ok(()))
    }

    /// Permanently deletes everything in the trash, with its page documents.
    pub fn empty_trash(&mut self) -> Result<Vec<NodeId>> {
        self.local_tree_change(|t| t.empty_trash(), Clone::clone, |_, _, _| Ok(()))
    }

    /// Applies tree updates received from the sync server. Returns what the
    /// UI should refresh.
    pub(crate) fn apply_remote_tree(&mut self, bytes: &[u8]) -> Result<Change> {
        match self.try_apply_remote_tree(bytes) {
            Ok(change) => Ok(change),
            Err(err) => {
                self.recover_tree();
                Err(err)
            }
        }
    }

    fn try_apply_remote_tree(&mut self, bytes: &[u8]) -> Result<Change> {
        let imported = self.tree.import(bytes)?;
        let changed = self.tree.take_changes();
        let version = self.tree.version();
        if version == self.tree_version && !imported.has_missing_dependencies {
            return Ok(Change::default());
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        docs::append_update(&tx, TREE_DOC_ID, bytes, &version, now)?;
        let outcome = apply_tree_changes(
            &tx,
            &self.tree,
            &mut self.pages,
            self.peer_id,
            &changed,
            &[],
            now,
        )?;
        maybe_compact(&tx, &self.config, TREE_DOC_ID, now, || {
            Ok((self.tree.export_snapshot()?, version.clone()))
        })?;
        tx.commit()?;
        self.tree_version = version;
        for id in &outcome.removed {
            self.databases.remove(id);
        }
        let nodes = changed
            .into_iter()
            .filter(|id| self.tree.contains(*id))
            .collect();
        Ok(Change {
            nodes,
            removed: outcome.removed,
            page: None,
            missing_dependencies: imported.has_missing_dependencies,
        })
    }
}
