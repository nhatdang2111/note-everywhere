//! The [`Workspace`] facade: the one type the application shell talks to.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ne_core::NodeId;
use ne_core::page::{PageDoc, SKELETON_PEER_ID};
use ne_core::tree::{Clock, NodeInfo, SystemClock, WorkspaceTree};
use rusqlite::Connection;
use uuid::Uuid;

use crate::blobs::BlobStore;
use crate::cache::PageCache;
use crate::db::{self, Durability};
use crate::docs::{self, DocKind, DocRef, TREE_DOC_ID};
use crate::error::{Error, Result};
use crate::nodes::{self, NodeRow};
use crate::outbox::{self, OutboxItem};
use crate::pages::{self, Backlink};
use crate::search::{self, SearchHit};

/// File name of the database inside the workspace directory.
pub const DB_FILE: &str = "workspace.db";
/// Directory of attachment blobs inside the workspace directory.
pub const ATTACHMENTS_DIR: &str = "attachments";
const PEER_ID_SETTING: &str = "device.peer_id";

/// Tunables of a [`Workspace`]. The defaults suit the desktop app.
#[derive(Clone)]
pub struct StoreConfig {
    /// How hard commits try to survive power loss.
    pub durability: Durability,
    /// Fold a document's update log into a new snapshot once it holds this
    /// many updates...
    pub compact_after_updates: usize,
    /// ...or this many bytes of updates.
    pub compact_after_bytes: usize,
    /// How many open pages are kept in memory.
    pub page_cache_capacity: usize,
    /// Source of timestamps; replaced in tests.
    pub clock: Arc<dyn Clock>,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            durability: Durability::Full,
            compact_after_updates: 100,
            compact_after_bytes: 512 * 1024,
            page_cache_capacity: 16,
            clock: Arc::new(SystemClock),
        }
    }
}

/// What changed in the workspace, for the UI to refresh.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Change {
    /// Tree nodes that exist and whose metadata, placement or children
    /// changed; re-read them.
    pub nodes: BTreeSet<NodeId>,
    /// Nodes that were permanently deleted, with their page documents.
    pub removed: BTreeSet<NodeId>,
    /// The page whose content changed, if any.
    pub page: Option<NodeId>,
    /// True when the update needs history that has not arrived yet; it is kept
    /// and takes effect once the missing updates are applied.
    pub missing_dependencies: bool,
}

/// A local workspace: one SQLite database, the live tree, open pages and the
/// attachment directory.
///
/// Every method that changes data persists it before returning, in one SQLite
/// transaction that also updates the derived indexes and the sync outbox. If
/// the transaction fails, memory is rolled back to what is stored, so memory
/// and disk never disagree.
pub struct Workspace {
    pub(crate) dir: PathBuf,
    pub(crate) conn: Connection,
    pub(crate) blobs: BlobStore,
    pub(crate) tree: WorkspaceTree,
    pub(crate) tree_version: Vec<u8>,
    pub(crate) pages: PageCache,
    pub(crate) peer_id: u64,
    pub(crate) config: StoreConfig,
}

fn new_peer_id() -> u64 {
    loop {
        let id = Uuid::new_v4().as_u64_pair().0;
        if id != SKELETON_PEER_ID && id != u64::MAX {
            return id;
        }
    }
}

pub(crate) fn load_tree(
    conn: &Connection,
    clock: Arc<dyn Clock>,
    peer_id: u64,
) -> Result<(WorkspaceTree, Vec<u8>)> {
    let stored = docs::load(conn, TREE_DOC_ID)?
        .ok_or_else(|| Error::Corrupt("the workspace tree document is missing".into()))?;
    let tree = WorkspaceTree::from_snapshot(&stored.snapshot, clock)?;
    for update in &stored.updates {
        tree.import(update)?;
    }
    // Loading is not a change; drop what the imports recorded.
    tree.take_changes();
    tree.set_peer_id(peer_id)?;
    let version = tree.version();
    Ok((tree, version))
}

/// Loads a page document from storage with the device's replica id.
pub(crate) fn load_page(conn: &Connection, id: NodeId, peer_id: u64) -> Result<Option<PageDoc>> {
    let Some(stored) = docs::load(conn, &id.to_string())? else {
        return Ok(None);
    };
    let doc = PageDoc::from_snapshot(&stored.snapshot)?;
    for update in &stored.updates {
        doc.import(update)?;
    }
    doc.set_peer_id(peer_id)?;
    Ok(Some(doc))
}

impl Workspace {
    /// Creates a new workspace in `dir` (created if missing). Fails if the
    /// directory already holds one.
    pub fn create(dir: impl AsRef<Path>, config: StoreConfig) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, &e))?;
        let mut conn = db::open(&dir.join(DB_FILE), config.durability)?;
        if docs::exists(&conn, TREE_DOC_ID)? {
            return Err(Error::AlreadyExists(dir));
        }
        let peer_id = new_peer_id();
        let tree = WorkspaceTree::with_peer_id(peer_id, config.clock.clone())?;
        let version = tree.version();
        let now = config.clock.now_ms();
        let tx = conn.transaction()?;
        docs::insert(
            &tx,
            TREE_DOC_ID,
            DocKind::Tree,
            &tree.export_snapshot()?,
            &version,
            now,
        )?;
        outbox::set_setting(&tx, PEER_ID_SETTING, &peer_id.to_string())?;
        tx.commit()?;
        Self::assemble(dir, conn, tree, version, peer_id, config)
    }

    /// Opens the workspace in `dir`. Fails with [`Error::NoWorkspace`] if there
    /// is none.
    pub fn open(dir: impl AsRef<Path>, config: StoreConfig) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let db_path = dir.join(DB_FILE);
        if !db_path.is_file() {
            return Err(Error::NoWorkspace(dir));
        }
        let conn = db::open(&db_path, config.durability)?;
        if !docs::exists(&conn, TREE_DOC_ID)? {
            return Err(Error::NoWorkspace(dir));
        }
        let peer_id = match outbox::setting(&conn, PEER_ID_SETTING)? {
            Some(text) => text
                .parse()
                .map_err(|_| Error::Corrupt(format!("device peer id {text:?}")))?,
            None => {
                let id = new_peer_id();
                outbox::set_setting(&conn, PEER_ID_SETTING, &id.to_string())?;
                id
            }
        };
        let (tree, version) = load_tree(&conn, config.clock.clone(), peer_id)?;
        Self::assemble(dir, conn, tree, version, peer_id, config)
    }

    /// Opens the workspace in `dir`, creating it first if there is none.
    pub fn open_or_create(dir: impl AsRef<Path>, config: StoreConfig) -> Result<Self> {
        match Self::open(dir.as_ref(), config.clone()) {
            Err(Error::NoWorkspace(_)) => Self::create(dir, config),
            other => other,
        }
    }

    fn assemble(
        dir: PathBuf,
        conn: Connection,
        tree: WorkspaceTree,
        tree_version: Vec<u8>,
        peer_id: u64,
        config: StoreConfig,
    ) -> Result<Self> {
        let blobs = BlobStore::open(dir.join(ATTACHMENTS_DIR))?;
        let pages = PageCache::new(config.page_cache_capacity);
        Ok(Self {
            dir,
            conn,
            blobs,
            tree,
            tree_version,
            pages,
            peer_id,
            config,
        })
    }

    // ---- accessors ----

    /// The workspace directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The live tree, for reads (children, ancestors, trash, ...). Change it
    /// only through the methods of the workspace, which persist.
    pub fn tree(&self) -> &WorkspaceTree {
        &self.tree
    }

    /// This device's replica id, stable across sessions.
    pub fn peer_id(&self) -> u64 {
        self.peer_id
    }

    /// The schema version of the database file.
    pub fn schema_version(&self) -> Result<i64> {
        db::schema_version(&self.conn)
    }

    /// Runs SQLite's integrity check. `Ok(())` means the file is sound.
    pub fn check_integrity(&self) -> Result<()> {
        let report: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if report == "ok" {
            Ok(())
        } else {
            Err(Error::Corrupt(report))
        }
    }

    pub(crate) fn now(&self) -> i64 {
        self.config.clock.now_ms()
    }

    // ---- reads from the index ----

    /// Every visible node, top level first and siblings in order: the data of
    /// the sidebar, from the SQLite index rather than the CRDT.
    pub fn sidebar(&self) -> Result<Vec<NodeRow>> {
        nodes::sidebar(&self.conn)
    }

    /// Visible children of `parent` (`None` is the top level) in order.
    pub fn children(&self, parent: Option<NodeId>) -> Result<Vec<NodeRow>> {
        nodes::children(&self.conn, parent)
    }

    /// One node from the index, trashed or not.
    pub fn node(&self, id: NodeId) -> Result<Option<NodeRow>> {
        nodes::get(&self.conn, id)
    }

    /// Visible favourites, in sidebar order.
    pub fn favorites(&self) -> Result<Vec<NodeInfo>> {
        Ok(self.tree.favorites()?)
    }

    /// Top-level trashed items; their subtrees go with them.
    pub fn trash(&self) -> Result<Vec<NodeInfo>> {
        Ok(self.tree.trash()?)
    }

    /// Full-text search over titles and bodies, best match first, see
    /// [`crate::search`] for the matching rules. Trashed pages and vault
    /// documents are never found.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        search::run(&self.conn, query, limit)
    }

    /// Pages that link to `page`.
    pub fn backlinks(&self, page: NodeId) -> Result<Vec<Backlink>> {
        pages::backlinks(&self.conn, page)
    }

    /// Number of words in the body of a page, from the index.
    pub fn word_count(&self, page: NodeId) -> Result<Option<usize>> {
        let Some(rid) = nodes::rid_of(&self.conn, page)? else {
            return Ok(None);
        };
        let count: Option<i64> = self
            .conn
            .query_row(
                "SELECT word_count FROM page_text WHERE rid = ?1",
                [rid],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(count.map(|c| usize::try_from(c).unwrap_or_default()))
    }

    // ---- outbox, cursor, settings ----

    /// Queues an update for sync and returns its client id. The workspace
    /// queues its own updates; this is for callers with updates of their own,
    /// such as the vault.
    pub fn outbox_enqueue(&mut self, doc: &DocRef, bytes: &[u8]) -> Result<String> {
        outbox::enqueue(&self.conn, doc, bytes, self.now())
    }

    /// The oldest `limit` queued updates, still queued.
    pub fn outbox_peek(&self, limit: usize) -> Result<Vec<OutboxItem>> {
        outbox::peek(&self.conn, limit)
    }

    /// Removes acknowledged updates by client id and returns how many were
    /// queued. Repeating an acknowledgement is harmless.
    pub fn outbox_acknowledge(&mut self, client_ids: &[String]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let removed = outbox::acknowledge(&tx, client_ids)?;
        tx.commit()?;
        Ok(removed)
    }

    /// Number of updates waiting to be sent.
    pub fn outbox_len(&self) -> Result<usize> {
        outbox::len(&self.conn)
    }

    /// The sync cursor, as last stored.
    pub fn sync_cursor(&self) -> Result<Option<Vec<u8>>> {
        outbox::cursor(&self.conn)
    }

    /// Stores the sync cursor.
    pub fn set_sync_cursor(&mut self, cursor: &[u8]) -> Result<()> {
        outbox::set_cursor(&self.conn, cursor)
    }

    /// A value of the sync metadata store.
    pub fn sync_meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        outbox::meta(&self.conn, key)
    }

    /// Stores a value in the sync metadata store.
    pub fn set_sync_meta(&mut self, key: &str, value: &[u8]) -> Result<()> {
        outbox::set_meta(&self.conn, key, value)
    }

    /// A key-value setting.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        outbox::setting(&self.conn, key)
    }

    /// Stores a key-value setting. The `device.` prefix is reserved.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        if key.starts_with("device.") {
            return Err(Error::InvalidInput(format!("setting {key:?} is reserved")));
        }
        outbox::set_setting(&self.conn, key, value)
    }

    /// Removes a setting.
    pub fn remove_setting(&mut self, key: &str) -> Result<()> {
        if key.starts_with("device.") {
            return Err(Error::InvalidInput(format!("setting {key:?} is reserved")));
        }
        outbox::remove_setting(&self.conn, key)
    }

    // ---- vault documents (opaque) ----

    /// Replaces a vault document's stored state. The store keeps vault data
    /// opaque, never reads it and never puts it in the search index.
    pub fn put_vault_doc(&mut self, id: &str, snapshot: &[u8], version: &[u8]) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        if docs::exists(&tx, id)? {
            docs::compact(&tx, id, snapshot, version, now)?;
        } else {
            docs::insert(&tx, id, DocKind::Vault, snapshot, version, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A vault document: its snapshot, version, and the updates stored since.
    pub fn vault_doc(&self, id: &str) -> Result<Option<VaultDoc>> {
        let Some(stored) = docs::load(&self.conn, id)? else {
            return Ok(None);
        };
        Ok(Some(VaultDoc {
            snapshot: stored.snapshot,
            version: stored.version,
            updates: stored.updates,
        }))
    }

    /// Ids of all stored vault documents.
    pub fn vault_doc_ids(&self) -> Result<Vec<String>> {
        docs::ids(&self.conn, DocKind::Vault)
    }

    /// Removes a vault document.
    pub fn delete_vault_doc(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        docs::delete(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
}

/// A stored vault document, opaque to the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultDoc {
    /// The last compacted state.
    pub snapshot: Vec<u8>,
    /// The version recorded with the latest write.
    pub version: Vec<u8>,
    /// Updates applied since the snapshot, oldest first.
    pub updates: Vec<Vec<u8>>,
}
