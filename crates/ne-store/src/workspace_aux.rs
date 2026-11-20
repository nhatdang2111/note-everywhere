//! Documents that are not pages but travel through sync under a node-shaped id
//! (see `ne_core::roles`): loading them, applying updates from other devices
//! and queueing them for a first upload.
//!
//! Databases are cached by id; the template library and the reminders are
//! single documents with fixed ids, created on first use.

use std::collections::BTreeSet;

use ne_core::NodeId;
use ne_core::database::DatabaseDoc;
use ne_core::reminders::RemindersDoc;
use ne_core::roles::Role;
use ne_core::template::LibraryDoc;

use crate::docs::{self, DocKind, DocRef};
use crate::error::{Error, Result};
use crate::outbox;
use crate::workspace::{Change, Workspace};
use crate::workspace_tree::maybe_compact;

/// The role of the documents whose id is fixed.
pub(crate) fn well_known_role(id: NodeId) -> Option<Role> {
    if id == NodeId::library() {
        Some(Role::Library)
    } else if id == NodeId::reminders() {
        Some(Role::Reminders)
    } else {
        None
    }
}

fn parse_rows(rows: &BTreeSet<String>) -> BTreeSet<NodeId> {
    rows.iter().filter_map(|r| r.parse().ok()).collect()
}

impl Workspace {
    // ---- databases ----

    /// Loads a database document into the cache. It must be stored: a database
    /// node whose document has not arrived yet is not found.
    pub(crate) fn load_database(&mut self, id: NodeId) -> Result<()> {
        if self.databases.contains_key(&id) {
            return Ok(());
        }
        let key = id.to_string();
        if docs::role(&self.conn, &key)? != Some(Role::Database) {
            return Err(Error::NotFound(format!("database {id}")));
        }
        let stored = docs::load(&self.conn, &key)?
            .ok_or_else(|| Error::NotFound(format!("database {id}")))?;
        let doc = DatabaseDoc::from_snapshot(&stored.snapshot)?;
        for update in &stored.updates {
            doc.import(update)?;
        }
        doc.set_peer_id(self.peer_id)?;
        doc.take_changes();
        self.databases.insert(id, doc);
        Ok(())
    }

    /// Persists an update that is already applied to a cached database: the
    /// log, the sync outbox (for local edits), the cell index and compaction,
    /// in one transaction. On failure the cached document is dropped, so the
    /// next use reloads what is stored.
    pub(crate) fn commit_database_update(
        &mut self,
        id: NodeId,
        update: &[u8],
        outbound: bool,
    ) -> Result<Change> {
        match self.try_commit_database_update(id, update, outbound) {
            Ok(change) => Ok(change),
            Err(err) => {
                self.databases.remove(&id);
                Err(err)
            }
        }
    }

    fn try_commit_database_update(
        &mut self,
        id: NodeId,
        update: &[u8],
        outbound: bool,
    ) -> Result<Change> {
        let now = self.now();
        let key = id.to_string();
        let Some(doc) = self.databases.get(&id) else {
            return Err(Error::NotFound(format!("database {id}")));
        };
        let version = doc.version();
        let changes = doc.take_changes();
        let tx = self.conn.transaction()?;
        docs::append_update(&tx, &key, update, &version, now)?;
        if outbound {
            outbox::enqueue(&tx, &DocRef::Page(id), update, now)?;
        }
        crate::db_index::apply_changes(&tx, id, doc, &changes)?;
        maybe_compact(&tx, &self.config, &key, now, || {
            Ok((doc.export_snapshot()?, version.clone()))
        })?;
        tx.commit()?;
        let mut nodes = parse_rows(&changes.rows);
        nodes.insert(id);
        Ok(Change {
            nodes,
            page: Some(id),
            ..Change::default()
        })
    }

    // ---- library and reminders ----

    /// Whether a document is stored (used to read the library and the
    /// reminders without creating them).
    pub(crate) fn has_document(&self, id: NodeId) -> Result<bool> {
        docs::exists(&self.conn, &id.to_string())
    }

    /// Loads the template library, creating it on first use.
    pub(crate) fn load_library(&mut self) -> Result<&LibraryDoc> {
        if self.library.is_none() {
            let id = NodeId::library();
            let key = id.to_string();
            let doc = match docs::load(&self.conn, &key)? {
                Some(stored) => {
                    let doc = LibraryDoc::from_snapshot(&stored.snapshot)?;
                    for update in &stored.updates {
                        doc.import(update)?;
                    }
                    doc.set_peer_id(self.peer_id)?;
                    doc
                }
                None => {
                    let doc = LibraryDoc::new()?;
                    doc.set_peer_id(self.peer_id)?;
                    self.store_new_aux(
                        id,
                        Role::Library,
                        &doc.export_snapshot()?,
                        &doc.version(),
                        &doc.export_all_updates()?,
                    )?;
                    doc
                }
            };
            self.library = Some(doc);
        }
        self.library
            .as_ref()
            .ok_or_else(|| Error::Corrupt("the template library is missing".into()))
    }

    /// Loads the reminders document, creating it on first use.
    pub(crate) fn load_reminders(&mut self) -> Result<&RemindersDoc> {
        if self.reminders.is_none() {
            let id = NodeId::reminders();
            let key = id.to_string();
            let doc = match docs::load(&self.conn, &key)? {
                Some(stored) => {
                    let doc = RemindersDoc::from_snapshot(&stored.snapshot)?;
                    for update in &stored.updates {
                        doc.import(update)?;
                    }
                    doc.set_peer_id(self.peer_id)?;
                    doc
                }
                None => {
                    let doc = RemindersDoc::new()?;
                    doc.set_peer_id(self.peer_id)?;
                    self.store_new_aux(
                        id,
                        Role::Reminders,
                        &doc.export_snapshot()?,
                        &doc.version(),
                        &doc.export_all_updates()?,
                    )?;
                    doc
                }
            };
            self.reminders = Some(doc);
        }
        self.reminders
            .as_ref()
            .ok_or_else(|| Error::Corrupt("the reminders document is missing".into()))
    }

    /// Stores a document this device just created and queues its first state.
    pub(crate) fn store_new_aux(
        &mut self,
        id: NodeId,
        role: Role,
        snapshot: &[u8],
        version: &[u8],
        first_update: &[u8],
    ) -> Result<()> {
        let now = self.now();
        let key = id.to_string();
        let tx = self.conn.transaction()?;
        docs::insert(&tx, &key, DocKind::Page, snapshot, version, now)?;
        docs::set_role(&tx, &key, role)?;
        outbox::enqueue(&tx, &DocRef::Page(id), first_update, now)?;
        if role == Role::Synced {
            crate::pages::refresh_orphan_state(&tx, &key, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Persists an edit of the library or the reminders: log, outbox and
    /// compaction in one transaction.
    pub(crate) fn commit_singleton_update(
        &mut self,
        id: NodeId,
        update: &[u8],
        version: &[u8],
        outbound: bool,
        state: impl FnOnce() -> Result<(Vec<u8>, Vec<u8>)>,
    ) -> Result<()> {
        let now = self.now();
        let key = id.to_string();
        let tx = self.conn.transaction()?;
        docs::append_update(&tx, &key, update, version, now)?;
        if outbound {
            outbox::enqueue(&tx, &DocRef::Page(id), update, now)?;
        }
        maybe_compact(&tx, &self.config, &key, now, state)?;
        tx.commit()?;
        Ok(())
    }

    // ---- sync ----

    /// The snapshot and version of any stored document of the `DocRef::Page`
    /// family: a page, a database, a synced block, the library, the reminders.
    pub(crate) fn document_state(&mut self, id: NodeId) -> Result<(Vec<u8>, Vec<u8>)> {
        let key = id.to_string();
        match docs::role(&self.conn, &key)? {
            Some(Role::Database) => {
                self.load_database(id)?;
                let doc = self
                    .databases
                    .get(&id)
                    .ok_or_else(|| Error::NotFound(format!("database {id}")))?;
                Ok((doc.export_snapshot()?, doc.version()))
            }
            Some(Role::Library) => {
                let doc = self.load_library()?;
                Ok((doc.export_snapshot()?, doc.version()))
            }
            Some(Role::Reminders) => {
                let doc = self.load_reminders()?;
                Ok((doc.export_snapshot()?, doc.version()))
            }
            Some(Role::Synced) | None => {
                self.load_into_cache(id, false)?;
                let page = self
                    .pages
                    .get(id)
                    .ok_or_else(|| Error::NotFound(format!("page {id}")))?;
                Ok((page.export_snapshot()?, page.version()))
            }
        }
    }

    /// The documents that sync carries under a node-shaped id and that are
    /// not a page of the tree: every database, synced block, the template
    /// library and the reminders. A caller that uploads or compacts "every
    /// page" for a server should include these as `DocRef::Page(id)`;
    /// [`Self::document_snapshot`] reads each.
    pub fn synced_documents(&self) -> Result<Vec<DocRef>> {
        let mut out = Vec::new();
        for (id, _) in docs::ids_with_roles(&self.conn)? {
            let node: NodeId = id
                .parse()
                .map_err(|_| Error::Corrupt(format!("document id {id:?}")))?;
            out.push(DocRef::Page(node));
        }
        Ok(out)
    }

    /// The full state of a document of the `DocRef::Page` family, whatever its
    /// role, for uploading it.
    pub fn document_snapshot(&mut self, id: NodeId) -> Result<Vec<u8>> {
        Ok(self.document_state(id)?.0)
    }

    /// Applies an update from the server to a document with a role other than
    /// a plain page or a synced block.
    pub(crate) fn apply_remote_aux(
        &mut self,
        id: NodeId,
        role: Role,
        update: &[u8],
        first_sight: bool,
    ) -> Result<Change> {
        let key = id.to_string();
        let now = self.now();
        match role {
            Role::Database => {
                if first_sight {
                    let doc = DatabaseDoc::from_snapshot(update)?;
                    doc.set_peer_id(self.peer_id)?;
                    let version = doc.version();
                    let tx = self.conn.transaction()?;
                    docs::insert(&tx, &key, DocKind::Page, update, &version, now)?;
                    docs::set_role(&tx, &key, Role::Database)?;
                    crate::db_index::rebuild(&tx, id, &doc)?;
                    tx.commit()?;
                    self.databases.insert(id, doc);
                    return Ok(Change {
                        nodes: [id].into(),
                        page: Some(id),
                        ..Change::default()
                    });
                }
                self.load_database(id)?;
                let Some(doc) = self.databases.get(&id) else {
                    return Err(Error::NotFound(format!("database {id}")));
                };
                let before = doc.version();
                let imported = doc.import(update).inspect_err(|_| {
                    self.databases.remove(&id);
                })?;
                let unchanged = self
                    .databases
                    .get(&id)
                    .is_some_and(|d| d.version() == before);
                if unchanged && !imported.has_missing_dependencies {
                    return Ok(Change::default());
                }
                let mut change = self.commit_database_update(id, update, false)?;
                change.missing_dependencies = imported.has_missing_dependencies;
                Ok(change)
            }
            Role::Library => self.apply_remote_singleton(id, role, update, first_sight),
            Role::Reminders => self.apply_remote_singleton(id, role, update, first_sight),
            Role::Synced => Err(Error::InvalidInput(
                "synced blocks are applied as pages".into(),
            )),
        }
    }

    fn apply_remote_singleton(
        &mut self,
        id: NodeId,
        role: Role,
        update: &[u8],
        first_sight: bool,
    ) -> Result<Change> {
        let now = self.now();
        let key = id.to_string();
        if first_sight {
            let version = match role {
                Role::Library => {
                    let doc = LibraryDoc::from_snapshot(update)?;
                    doc.set_peer_id(self.peer_id)?;
                    let version = doc.version();
                    self.library = Some(doc);
                    version
                }
                _ => {
                    let doc = RemindersDoc::from_snapshot(update)?;
                    doc.set_peer_id(self.peer_id)?;
                    let version = doc.version();
                    self.reminders = Some(doc);
                    version
                }
            };
            let tx = self.conn.transaction()?;
            docs::insert(&tx, &key, DocKind::Page, update, &version, now)?;
            docs::set_role(&tx, &key, role)?;
            tx.commit()?;
            return Ok(Change::default());
        }
        let (before, imported, version, snapshot) = match role {
            Role::Library => {
                let doc = self.load_library()?;
                let before = doc.version();
                let imported = doc.import(update)?;
                (before, imported, doc.version(), doc.export_snapshot())
            }
            _ => {
                let doc = self.load_reminders()?;
                let before = doc.version();
                let imported = doc.import(update)?;
                (before, imported, doc.version(), doc.export_snapshot())
            }
        };
        if version == before && !imported.has_missing_dependencies {
            return Ok(Change::default());
        }
        self.commit_singleton_update(id, update, &version, false, || {
            Ok((snapshot?, version.clone()))
        })?;
        Ok(Change {
            missing_dependencies: imported.has_missing_dependencies,
            ..Change::default()
        })
    }
}
