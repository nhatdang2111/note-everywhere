//! Page operations of the [`Workspace`]: opening, editing, applying updates.

use ne_core::NodeId;
use ne_core::page::PageDoc;
use ne_core::roles::Role;
use ne_core::tree::NodeKind;

use crate::docs::{self, DocKind, DocRef};
use crate::error::{Error, Result};
use crate::outbox;
use crate::pages;
use crate::workspace::{Change, Workspace, load_page};
use crate::workspace_aux::well_known_role;
use crate::workspace_tree::maybe_compact;

impl Workspace {
    /// Loads a page into the cache if it is not there. A page node with no
    /// stored document gets the empty skeleton; that is safe on every device
    /// because the skeleton is the same operations everywhere, so it merges
    /// into the creator's copy without duplicating anything. `check_node`
    /// requires the page node to exist in the tree.
    pub(crate) fn load_into_cache(&mut self, id: NodeId, check_node: bool) -> Result<()> {
        if self.pages.contains(id) {
            return Ok(());
        }
        if check_node {
            let info = self.tree.get(id)?;
            if info.kind != NodeKind::Page {
                return Err(Error::InvalidInput(format!(
                    "node {id} is a folder or a database, not a page"
                )));
            }
        }
        let clock = self.config.clock.clone();
        let doc = match load_page(&self.conn, id, self.peer_id)? {
            Some(doc) => doc.with_clock(clock),
            None => {
                if !check_node && docs::role(&self.conn, &id.to_string())?.is_none() {
                    // An id that is not a node is only ever opened for a
                    // document the store holds.
                    return Err(Error::NotFound(format!("document {id}")));
                }
                let doc = PageDoc::with_peer_id(self.peer_id)?.with_clock(clock);
                let now = self.now();
                let tx = self.conn.transaction()?;
                docs::insert(
                    &tx,
                    &id.to_string(),
                    DocKind::Page,
                    &doc.export_snapshot()?,
                    &doc.version(),
                    now,
                )?;
                pages::index_page(&tx, id, &doc, now)?;
                tx.commit()?;
                doc
            }
        };
        self.pages.insert(id, doc);
        Ok(())
    }

    /// Opens a page for reading and for core-side edits. The document stays
    /// cached until more pages push it out.
    pub fn open_page(&mut self, id: NodeId) -> Result<&PageDoc> {
        self.load_into_cache(id, true)?;
        self.pages
            .get(id)
            .ok_or_else(|| Error::NotFound(format!("page {id}")))
    }

    /// The full snapshot of a page, which is what the editor opens.
    pub fn page_snapshot(&mut self, id: NodeId) -> Result<Vec<u8>> {
        Ok(self.open_page(id)?.export_snapshot()?)
    }

    /// Whether a page document is in memory (not whether it is stored).
    pub fn is_page_cached(&self, id: NodeId) -> bool {
        self.pages.contains(id)
    }

    /// Number of page documents in memory.
    pub fn cached_page_count(&self) -> usize {
        self.pages.len()
    }

    /// Drops every cached page document; the next open loads from storage.
    pub fn evict_pages(&mut self) {
        self.pages.clear();
    }

    /// Applies an update produced by the editor to a page: imports it,
    /// stores it, refreshes the search, link and attachment indexes and queues
    /// it for sync, all in one transaction.
    pub fn apply_local_update(&mut self, id: NodeId, update: &[u8]) -> Result<Change> {
        self.load_into_cache(id, true)?;
        let imported = match self.pages.get(id) {
            Some(doc) => doc.import(update),
            None => return Err(Error::NotFound(format!("page {id}"))),
        };
        let imported = imported.inspect_err(|_| self.pages.remove(id))?;
        self.commit_page_update(id, update, true, imported.has_missing_dependencies)
    }

    /// Edits a page from Rust (Markdown import, voice text, tests) and
    /// persists the edit like [`Self::apply_local_update`]. If `edit` fails
    /// part-way, nothing is stored and the page reloads from storage.
    pub fn edit_page<R>(
        &mut self,
        id: NodeId,
        edit: impl FnOnce(&PageDoc) -> ne_core::Result<R>,
    ) -> Result<(R, Change)> {
        self.load_into_cache(id, true)?;
        let Some(doc) = self.pages.get(id) else {
            return Err(Error::NotFound(format!("page {id}")));
        };
        let before = doc.version();
        let (value, update, unchanged) = match doc.edit(edit) {
            Ok((value, update)) => (value, update, doc.version() == before),
            Err(err) => {
                self.pages.remove(id);
                return Err(err.into());
            }
        };
        if unchanged {
            return Ok((value, Change::default()));
        }
        let change = self.commit_page_update(id, &update, true, false)?;
        Ok((value, change))
    }

    /// Persists an update that is already applied to the cached document.
    pub(crate) fn commit_page_update(
        &mut self,
        id: NodeId,
        update: &[u8],
        outbound: bool,
        missing_dependencies: bool,
    ) -> Result<Change> {
        let result = self.try_commit_page_update(id, update, outbound);
        if result.is_err() {
            // Memory is ahead of storage; reload on next use.
            self.pages.remove(id);
        }
        result.map(|()| Change {
            page: Some(id),
            missing_dependencies,
            ..Change::default()
        })
    }

    fn try_commit_page_update(&mut self, id: NodeId, update: &[u8], outbound: bool) -> Result<()> {
        let now = self.now();
        let key = id.to_string();
        let Some(doc) = self.pages.get(id) else {
            return Err(Error::NotFound(format!("page {id}")));
        };
        let version = doc.version();
        let tx = self.conn.transaction()?;
        docs::append_update(&tx, &key, update, &version, now)?;
        if outbound {
            outbox::enqueue(&tx, &DocRef::Page(id), update, now)?;
        }
        pages::index_page(&tx, id, doc, now)?;
        maybe_compact(&tx, &self.config, &key, now, || {
            Ok((doc.export_snapshot()?, version.clone()))
        })?;
        tx.commit()?;
        Ok(())
    }

    /// Applies an update received from the sync server to the tree, a page
    /// or a vault document, and reports what the UI should refresh. Nothing is
    /// queued for sync.
    pub fn apply_remote_update(&mut self, doc: &DocRef, update: &[u8]) -> Result<Change> {
        match doc {
            DocRef::Tree => self.apply_remote_tree(update),
            DocRef::Page(id) => self.apply_remote_page(*id, update),
            DocRef::Vault(id) => self.apply_remote_vault(id, update),
        }
    }

    fn apply_remote_page(&mut self, id: NodeId, update: &[u8]) -> Result<Change> {
        let key = id.to_string();
        let known = self.pages.contains(id) || docs::exists(&self.conn, &key)?;
        // A document that is not a plain page says what it is in its first
        // update; once stored, the role is read back from the store.
        let role = if known {
            docs::role(&self.conn, &key)?
        } else {
            ne_core::roles::role_of(update).or_else(|| well_known_role(id))
        };
        if let Some(role) = role
            && role != Role::Synced
        {
            return self.apply_remote_aux(id, role, update, !known);
        }
        if !known {
            // First sight of this page. Its first update is stored as the
            // snapshot; it may be a full snapshot or an update, both import.
            let doc = PageDoc::from_snapshot(update)?.with_clock(self.config.clock.clone());
            doc.set_peer_id(self.peer_id)?;
            let (now, version) = (self.now(), doc.version());
            let tx = self.conn.transaction()?;
            docs::insert(&tx, &key, DocKind::Page, update, &version, now)?;
            if role == Some(Role::Synced) {
                docs::set_role(&tx, &key, Role::Synced)?;
                pages::refresh_orphan_state(&tx, &key, now)?;
            }
            pages::index_page(&tx, id, &doc, now)?;
            tx.commit()?;
            self.pages.insert(id, doc);
            return Ok(Change {
                page: Some(id),
                ..Change::default()
            });
        }
        self.load_into_cache(id, false)?;
        let Some(doc) = self.pages.get(id) else {
            return Err(Error::NotFound(format!("page {id}")));
        };
        let before = doc.version();
        let imported = doc.import(update).inspect_err(|_| self.pages.remove(id))?;
        let unchanged = self
            .pages
            .get(id)
            .is_some_and(|doc| doc.version() == before);
        if unchanged && !imported.has_missing_dependencies {
            return Ok(Change::default());
        }
        self.commit_page_update(id, update, false, imported.has_missing_dependencies)
    }

    fn apply_remote_vault(&mut self, id: &str, update: &[u8]) -> Result<Change> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        if !docs::exists(&tx, id)? {
            docs::insert(&tx, id, DocKind::Vault, &[], &[], now)?;
        }
        let version = docs::load(&tx, id)?.map(|d| d.version).unwrap_or_default();
        docs::append_update(&tx, id, update, &version, now)?;
        tx.commit()?;
        Ok(Change::default())
    }

    /// Forces the update log of a document into a new snapshot now.
    pub fn compact(&mut self, doc: &DocRef) -> Result<()> {
        let now = self.now();
        match doc {
            DocRef::Tree => {
                let tx = self.conn.transaction()?;
                docs::compact(
                    &tx,
                    &doc.id(),
                    &self.tree.export_snapshot()?,
                    &self.tree.version(),
                    now,
                )?;
                tx.commit()?;
            }
            DocRef::Page(id) => {
                let (snapshot, version) = self.document_state(*id)?;
                let tx = self.conn.transaction()?;
                docs::compact(&tx, &doc.id(), &snapshot, &version, now)?;
                tx.commit()?;
            }
            DocRef::Vault(_) => {
                return Err(Error::InvalidInput(
                    "vault documents are compacted with put_vault_doc".into(),
                ));
            }
        }
        Ok(())
    }

    /// Update log size of a stored document: `(updates, bytes)`.
    pub fn update_log_size(&self, doc: &DocRef) -> Result<(usize, usize)> {
        if !docs::exists(&self.conn, &doc.id())? {
            return Err(Error::NotFound(doc.to_string()));
        }
        docs::log_size(&self.conn, &doc.id())
    }
}
