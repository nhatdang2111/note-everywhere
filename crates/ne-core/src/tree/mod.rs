//! The workspace tree: folders and pages in one Loro document.
//!
//! # Model
//!
//! [`WorkspaceTree`] is a typed API over a single Loro document that holds a
//! forest of nodes in Loro's movable tree. Each node is a [`NodeKind::Folder`]
//! or a [`NodeKind::Page`] and carries only metadata (title, icon, favourite
//! flag, timestamps, trash state) in a per-node map. Page *content* lives in
//! separate per-page documents, found through the page node's [`NodeId`].
//!
//! Metadata fields are last-writer-wins registers, so concurrent edits to the
//! same field converge to one value, while edits to different fields (a rename
//! against a move, say) both survive. Structure changes use Loro's movable
//! tree: concurrent moves that would form a cycle converge with one of them
//! dropped, never with a cycle and never with a lost node.
//!
//! # Trash
//!
//! Trashing a node sets `trashed_at` on that node only; the subtree stays
//! attached underneath it. A node is *hidden* when it or any ancestor is
//! trashed. Because hiding is derived from ancestry, a child that another
//! replica adds under a node being trashed is hidden too, with no extra
//! bookkeeping. Restoring clears the flag, so the node returns to its old
//! place; if an ancestor is itself still trashed it moves to the root instead.
//! Permanent deletion removes the node and its whole subtree.
//!
//! # Positions
//!
//! Positions are expressed among *visible* siblings (see [`Position`]); trashed
//! siblings keep their slots and reappear at the same spot on restore.
//!
//! # Sync and change tracking
//!
//! `export_snapshot`, `export_updates_since`, `import`, `version` and
//! `frontiers` expose the sync surface with opaque bytes. Every local edit and
//! every import records the ids of the nodes it touched; [`WorkspaceTree::take_changes`]
//! drains that set, which is all a SQLite indexer needs to re-read the
//! affected rows.

mod changes;
mod node;
mod ops;
mod queries;
mod trash;

use std::collections::BTreeSet;
use std::sync::Arc;

use loro::{
    ExportMode, LoroDoc, LoroMap, LoroTree, Subscription, TreeID, TreeParentId, VersionVector,
};

use crate::error::{Error, Result};
use crate::ids::NodeId;
use changes::ChangeLog;
pub use node::{Clock, MAX_ICON_CHARS, NodeInfo, NodeKind, Position, SystemClock};

const TREE_NAME: &str = "workspace_tree";
const KEY_KIND: &str = "kind";
const KEY_TITLE: &str = "title";
const KEY_ICON: &str = "icon";
const KEY_FAVORITE: &str = "favorite";
const KEY_CREATED_AT: &str = "created_at";
const KEY_UPDATED_AT: &str = "updated_at";
const KEY_TRASHED_AT: &str = "trashed_at";

/// Result of [`WorkspaceTree::import`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOutcome {
    /// True when some imported changes depend on history this replica does not
    /// have yet; they are held back until the missing updates arrive.
    pub has_missing_dependencies: bool,
}

/// Typed API over the CRDT document holding the folder and page hierarchy.
/// See the [module documentation](self) for the model.
pub struct WorkspaceTree {
    doc: LoroDoc,
    tree: LoroTree,
    clock: Arc<dyn Clock>,
    changes: ChangeLog,
    // Dropping the guard would stop change tracking.
    _subscription: Subscription,
}

impl WorkspaceTree {
    /// Creates an empty tree with a random replica id.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::from_doc(LoroDoc::new(), clock)
    }

    /// Creates an empty tree with a fixed replica id. Every replica of the
    /// same workspace must use a distinct id.
    pub fn with_peer_id(peer_id: u64, clock: Arc<dyn Clock>) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer_id)?;
        Ok(Self::from_doc(doc, clock))
    }

    /// Loads a tree from bytes produced by [`Self::export_snapshot`]. Loading
    /// does not record changes: the caller already has the full state.
    pub fn from_snapshot(bytes: &[u8], clock: Arc<dyn Clock>) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        Ok(Self::from_doc(doc, clock))
    }

    fn from_doc(doc: LoroDoc, clock: Arc<dyn Clock>) -> Self {
        let tree = doc.get_tree(TREE_NAME);
        tree.enable_fractional_index(0);
        let changes = ChangeLog::default();
        let subscription = changes.subscribe(&doc);
        Self {
            doc,
            tree,
            clock,
            changes,
            _subscription: subscription,
        }
    }

    /// The Loro peer id of this replica.
    pub fn peer_id(&self) -> u64 {
        self.doc.peer_id()
    }

    /// Exports the full state, including history, for storage or first sync.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// Exports every change a replica at `version` (bytes from [`Self::version`])
    /// has not seen yet.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let vv = VersionVector::decode(version)?;
        self.doc.commit();
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    /// Applies a snapshot or updates from another replica. Node ids touched by
    /// the import are added to the set returned by [`Self::take_changes`].
    pub fn import(&self, bytes: &[u8]) -> Result<ImportOutcome> {
        let status = self.doc.import(bytes)?;
        Ok(ImportOutcome {
            has_missing_dependencies: status.pending.is_some(),
        })
    }

    /// The current version vector as opaque bytes; hand it to a peer so it can
    /// call [`Self::export_updates_since`].
    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    /// The current frontiers as opaque bytes; a compact identifier of the
    /// latest state, equal on replicas that hold the same history.
    pub fn frontiers(&self) -> Vec<u8> {
        self.doc.oplog_frontiers().encode()
    }

    /// Drains the ids of nodes touched since the previous call, by local edits
    /// and by imports. A node is reported when its metadata, its parent or its
    /// list of children changed, or when it was deleted. Sibling index shifts
    /// are covered by the parent being reported; re-read its children.
    pub fn take_changes(&self) -> BTreeSet<NodeId> {
        self.changes.take()
    }

    // ---- internals shared by the submodules ----

    fn now(&self) -> i64 {
        self.clock.now_ms()
    }

    fn commit(&self) {
        self.doc.commit();
    }

    fn parent_id(parent: Option<NodeId>) -> TreeParentId {
        parent.map_or(TreeParentId::Root, |p| TreeParentId::Node(p.tree_id()))
    }

    /// True when the node exists and has not been permanently deleted.
    fn exists(&self, id: TreeID) -> bool {
        self.tree.contains(id) && matches!(self.tree.is_node_deleted(&id), Ok(false))
    }

    fn require_exists(&self, id: NodeId) -> Result<TreeID> {
        let tid = id.tree_id();
        if self.exists(tid) {
            Ok(tid)
        } else {
            Err(Error::NotFound(id))
        }
    }

    /// Ancestors of `id`, nearest first, ending below the root. Fails instead
    /// of looping if the parent links ever form a cycle.
    fn ancestor_chain(&self, id: TreeID) -> Result<Vec<TreeID>> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::from([id]);
        let mut current = id;
        while let Some(TreeParentId::Node(parent)) = self.tree.parent(current) {
            if !seen.insert(parent) {
                return Err(Error::Corrupt(NodeId::from_tree_id(parent)));
            }
            chain.push(parent);
            current = parent;
        }
        Ok(chain)
    }

    fn meta(&self, id: TreeID) -> Result<LoroMap> {
        Ok(self.tree.get_meta(id)?)
    }

    fn trashed_at(&self, id: TreeID) -> Option<i64> {
        let value = self.meta(id).ok()?.get_value();
        value.as_map()?.get(KEY_TRASHED_AT)?.as_i64().copied()
    }

    /// Existing node that is neither trashed nor below a trashed node.
    fn require_visible(&self, id: NodeId) -> Result<TreeID> {
        let tid = self.require_exists(id)?;
        if self.trashed_at(tid).is_some() {
            return Err(Error::Trashed(id));
        }
        for ancestor in self.ancestor_chain(tid)? {
            if self.trashed_at(ancestor).is_some() {
                return Err(Error::Trashed(id));
            }
        }
        Ok(tid)
    }

    /// Optional parent that must be the root or a visible node.
    fn require_visible_parent(&self, parent: Option<NodeId>) -> Result<TreeParentId> {
        if let Some(p) = parent {
            self.require_visible(p)?;
        }
        Ok(Self::parent_id(parent))
    }

    fn touch(&self, meta: &LoroMap) -> Result<()> {
        meta.insert(KEY_UPDATED_AT, self.now())?;
        Ok(())
    }
}
