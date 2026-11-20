//! Page documents: the content of one page in one Loro document.
//!
//! # Layout
//!
//! [`PageDoc`] reads and writes exactly the container layout that the editor
//! binding (`loro-prosemirror`) produces, so the webview editor and this crate
//! operate on the same document. The layout is specified in
//! `docs/decisions/0006-page-document-structure.md`: a root map `doc` whose
//! nodes are maps with `nodeName`, `attributes` and `children`, text in
//! `LoroText` containers and marks as rich-text attributes.
//!
//! # Creating a page
//!
//! Two devices must never both create the root of one page independently with
//! different identities, because the root's keys are last-writer-wins and one
//! side's blocks would vanish. [`PageDoc::new`] therefore builds the skeleton
//! (the root node and one empty paragraph) under a fixed author id
//! ([`SKELETON_PEER_ID`]) with a fixed sequence of operations, so every device
//! produces the very same operations and importing one into another is a
//! no-op. The skeleton still travels as the page's first update: the creating
//! device stores [`PageDoc::export_snapshot`], queues
//! [`PageDoc::export_all_updates`] for sync, and the editor only ever opens a
//! document that already holds the skeleton. Edits afterwards are authored
//! under the device's own peer id.
//!
//! # Reading and writing
//!
//! [`PageDoc::blocks`] returns the page as a [`Block`] tree. Writes address a
//! block by a path of child indexes (see [`Block`]) and are validated against
//! the editor's content rules first, see the methods in this module. Extraction
//! for indexing lives in [`summarize`], [`outgoing_links`] and friends.
//!
//! # Sync
//!
//! The sync surface has the same shape as [`crate::tree::WorkspaceTree`]:
//! [`export_snapshot`](PageDoc::export_snapshot),
//! [`export_updates_since`](PageDoc::export_updates_since),
//! [`import`](PageDoc::import), [`version`](PageDoc::version) and
//! [`frontiers`](PageDoc::frontiers), plus
//! [`export_shallow_snapshot`](PageDoc::export_shallow_snapshot) for
//! compaction.

mod extract;
mod model;
mod read;
mod schema;
mod write;

use loro::{ExportMode, LoroDoc, LoroList, LoroMap, VersionVector};

use crate::error::{Error, Result};
pub use crate::tree::ImportOutcome;
pub use extract::{
    LinkKind, OutgoingLink, PageSummary, attachment_hashes, block_plain_text, outgoing_links,
    page_plain_text, summarize, word_count,
};
pub use model::{
    ATTACHMENT_HASH_LEN, ATTACHMENT_SRC_PREFIX, Block, BlockKind, Inline, Marks, PAGE_HREF_PREFIX,
    Run, attachment_src, normalize_inline, page_href, parse_attachment_src, parse_page_href,
};

const ROOT_NAME: &str = "doc";

/// The author id every page skeleton is written under. It is the same on all
/// devices on purpose, see the [module documentation](self). Devices must not
/// use it for their own edits.
pub const SKELETON_PEER_ID: u64 = 0x4e45_534b_454c_0000;

/// Typed API over the CRDT document holding the content of one page.
/// See the [module documentation](self) for the model.
pub struct PageDoc {
    doc: LoroDoc,
    root: LoroMap,
}

impl PageDoc {
    /// Creates a page holding the skeleton (one empty paragraph), authored
    /// by the shared skeleton id, with a random replica id for later edits.
    pub fn new() -> Result<Self> {
        let doc = LoroDoc::new();
        let own = doc.peer_id();
        Self::create(doc, own)
    }

    /// Like [`Self::new`] with a fixed replica id for later edits. Every
    /// replica of the same page must use a distinct id.
    pub fn with_peer_id(peer_id: u64) -> Result<Self> {
        Self::create(LoroDoc::new(), peer_id)
    }

    fn create(doc: LoroDoc, own_peer: u64) -> Result<Self> {
        if own_peer == SKELETON_PEER_ID {
            return Err(Error::Crdt("peer id is reserved for page skeletons".into()));
        }
        doc.set_peer_id(SKELETON_PEER_ID)?;
        let root = doc.get_map(ROOT_NAME);
        root.insert(read::KEY_NODE_NAME, ROOT_NAME)?;
        root.insert_container(read::KEY_ATTRIBUTES, LoroMap::new())?;
        let children = root.insert_container(read::KEY_CHILDREN, LoroList::new())?;
        write::write_block(&children, None, &Block::new(BlockKind::Paragraph))?;
        doc.commit();
        doc.set_peer_id(own_peer)?;
        write::configure_text_styles(&doc);
        Ok(Self { doc, root })
    }

    /// Loads a page from bytes produced by [`Self::export_snapshot`] or
    /// [`Self::export_shallow_snapshot`], with a random replica id. The
    /// content is not checked until it is read.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        write::configure_text_styles(&doc);
        let root = doc.get_map(ROOT_NAME);
        Ok(Self { doc, root })
    }

    /// Sets this replica's id. Do it before the first local edit.
    pub fn set_peer_id(&self, peer_id: u64) -> Result<()> {
        if peer_id == SKELETON_PEER_ID {
            return Err(Error::Crdt("peer id is reserved for page skeletons".into()));
        }
        Ok(self.doc.set_peer_id(peer_id)?)
    }

    /// The Loro peer id of this replica.
    pub fn peer_id(&self) -> u64 {
        self.doc.peer_id()
    }

    // ---- reading ----

    /// The top-level blocks of the page.
    pub fn blocks(&self) -> Result<Vec<Block>> {
        read::read_root(&self.root)
    }

    /// The block at `path` with everything below it. The empty path is
    /// rejected; use [`Self::blocks`] for the page.
    pub fn block(&self, path: &[usize]) -> Result<Block> {
        if path.is_empty() {
            return Err(Error::NoSuchBlock(Vec::new()));
        }
        read::read_block(&self.node_at(path)?)
    }

    /// Everything the store indexes about the page, in one pass.
    pub fn summary(&self) -> Result<PageSummary> {
        Ok(summarize(&self.blocks()?))
    }

    /// Plain text of the whole page.
    pub fn plain_text(&self) -> Result<String> {
        Ok(page_plain_text(&self.blocks()?))
    }

    // ---- sync ----

    /// Exports the full state, including history, for storage or first sync.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// Exports the state with history trimmed to the current version. It
    /// shrinks storage, but a replica that is behind the trimmed point can no
    /// longer catch up from it, so use it only when every replica has already
    /// seen the current version or will receive the snapshot itself.
    pub fn export_shallow_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        let frontiers = self.doc.oplog_frontiers();
        Ok(self.doc.export(ExportMode::shallow_snapshot(&frontiers))?)
    }

    /// Exports the whole history as updates, the form in which a new page's
    /// skeleton and first edits are queued for sync.
    pub fn export_all_updates(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::all_updates())?)
    }

    /// Exports every change a replica at `version` (bytes from [`Self::version`])
    /// has not seen yet.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let vv = VersionVector::decode(version)?;
        self.doc.commit();
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    /// Applies a snapshot or updates from the editor or another replica.
    pub fn import(&self, bytes: &[u8]) -> Result<ImportOutcome> {
        let status = self.doc.import(bytes)?;
        Ok(ImportOutcome {
            has_missing_dependencies: status.pending.is_some(),
        })
    }

    /// The current version vector as opaque bytes.
    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    /// The current frontiers as opaque bytes; equal on replicas that hold the
    /// same history.
    pub fn frontiers(&self) -> Vec<u8> {
        self.doc.oplog_frontiers().encode()
    }

    /// Runs `edit` and returns its result together with the update bytes
    /// holding exactly the changes it made, ready to queue for sync.
    pub fn edit<R>(&self, edit: impl FnOnce(&Self) -> Result<R>) -> Result<(R, Vec<u8>)> {
        self.doc.commit();
        let before = self.doc.oplog_vv();
        let result = edit(self);
        self.doc.commit();
        let update = self.doc.export(ExportMode::updates(&before))?;
        result.map(|value| (value, update))
    }
}
