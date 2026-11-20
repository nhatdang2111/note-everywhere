//! Synced blocks on the [`Workspace`]: shared content documents, their
//! occurrences and their clean-up. The model is in `ne_core::synced`; the
//! decision record is `docs/decisions/0008-synced-blocks.md`.
//!
//! # When the last occurrence goes
//!
//! * Unsyncing the last occurrence copies the content into the page and
//!   deletes the shared document in the same call.
//! * Deleting the placeholder block (or the page for good) can leave a shared
//!   document with no occurrence. It is kept, so undo and restoring an older
//!   version of the page still find it, and collected by
//!   [`Workspace::synced_gc`] once it has been without occurrence for the age
//!   the caller chooses. Pages in the trash still count as occurrences.

use ne_core::NodeId;
use ne_core::page::PageDoc;
use ne_core::roles::Role;
use ne_core::synced;
use rusqlite::params;

use crate::docs;
use crate::error::{Error, Result};
use crate::workspace::{Change, Workspace};

/// A shared document and where it is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedInfo {
    /// The shared document's id (the `syncId` of its placeholders).
    pub id: NodeId,
    /// Pages that show it, trashed ones included.
    pub occurrences: usize,
    /// Since when it has had no occurrence, if it has none.
    pub orphaned_at: Option<i64>,
}

/// A page that shows a synced block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The page.
    pub page: NodeId,
    /// Its title.
    pub title: String,
    /// True when the page is in the trash.
    pub trashed: bool,
}

impl Workspace {
    fn require_synced(&self, id: NodeId) -> Result<()> {
        match docs::role(&self.conn, &id.to_string())? {
            Some(Role::Synced) => Ok(()),
            _ => Err(Error::NotFound(format!("synced block {id}"))),
        }
    }

    /// Turns the top-level blocks `start..end` of a page into a synced block:
    /// a new shared document takes the blocks and one placeholder replaces
    /// them in the page. Returns the sync id and the page update for the
    /// editor.
    pub fn synced_create(
        &mut self,
        page: NodeId,
        start: usize,
        end: usize,
    ) -> Result<(NodeId, Vec<u8>)> {
        let blocks = synced::selection(self.open_page(page)?, start, end)?;
        let sync_id = NodeId::fresh_document();
        let shared =
            synced::new_shared_doc(&blocks, self.peer_id)?.with_clock(self.config.clock.clone());
        // The shared document is stored first, so no placeholder is ever
        // written for a document that does not exist.
        self.store_new_aux(
            sync_id,
            Role::Synced,
            &shared.export_snapshot()?,
            &shared.version(),
            &shared.export_all_updates()?,
        )?;
        self.pages.insert(sync_id, shared);
        let key = sync_id.to_string();
        let ((), update, _) = self.edit_page_update(page, |doc| {
            synced::replace_with_placeholder(doc, start, end, &key)
        })?;
        Ok((sync_id, update))
    }

    /// Inserts a placeholder for an existing synced block as child `index` of
    /// the block at `parent` (the empty path is the page). Returns the page
    /// update for the editor.
    pub fn synced_insert(
        &mut self,
        page: NodeId,
        parent: &[usize],
        index: usize,
        sync_id: NodeId,
    ) -> Result<Vec<u8>> {
        self.require_synced(sync_id)?;
        let key = sync_id.to_string();
        let ((), update, _) = self.edit_page_update(page, |doc| {
            synced::insert_placeholder(doc, parent, index, &key)
        })?;
        Ok(update)
    }

    /// The shared document as the editor opens it: a full snapshot.
    pub fn synced_open(&mut self, id: NodeId) -> Result<Vec<u8>> {
        self.require_synced(id)?;
        self.load_into_cache(id, false)?;
        let doc = self
            .pages
            .get(id)
            .ok_or_else(|| Error::NotFound(format!("synced block {id}")))?;
        Ok(doc.export_snapshot()?)
    }

    /// Applies an update the editor made to a shared document.
    pub fn synced_apply_update(&mut self, id: NodeId, update: &[u8]) -> Result<Change> {
        self.require_synced(id)?;
        self.load_into_cache(id, false)?;
        let imported = match self.pages.get(id) {
            Some(doc) => doc.import(update),
            None => return Err(Error::NotFound(format!("synced block {id}"))),
        };
        let imported = imported.inspect_err(|_| self.pages.remove(id))?;
        self.commit_page_update(id, update, true, imported.has_missing_dependencies)
    }

    /// The pages that show a synced block, by title.
    pub fn synced_occurrences(&self, id: NodeId) -> Result<Vec<Occurrence>> {
        let mut statement = self.conn.prepare_cached(
            "SELECT n.id, n.title, n.hidden FROM synced_refs r JOIN nodes n ON n.id = r.page_id
             WHERE r.sync_id = ?1 ORDER BY n.title, n.id",
        )?;
        let rows = statement
            .query_map([id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(page, title, trashed)| {
                Ok(Occurrence {
                    page: page
                        .parse()
                        .map_err(|_| Error::Corrupt(format!("page id {page:?}")))?,
                    title,
                    trashed,
                })
            })
            .collect()
    }

    /// Every shared document the store holds, with its occurrence count.
    pub fn synced_list(&self) -> Result<Vec<SyncedInfo>> {
        let mut statement = self.conn.prepare_cached(
            "SELECT d.id, (SELECT COUNT(*) FROM synced_refs r WHERE r.sync_id = d.id), s.orphaned_at
             FROM docs d LEFT JOIN synced_state s ON s.sync_id = d.id
             WHERE d.role = 'synced' ORDER BY d.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, count, orphaned_at)| {
                Ok(SyncedInfo {
                    id: id
                        .parse()
                        .map_err(|_| Error::Corrupt(format!("document id {id:?}")))?,
                    occurrences: usize::try_from(count).unwrap_or_default(),
                    orphaned_at,
                })
            })
            .collect()
    }

    /// Replaces the placeholder at `path` of a page with a copy of the shared
    /// content, so that occurrence stops following the others. If it was the
    /// last occurrence the shared document is deleted. Returns the page update
    /// for the editor.
    pub fn synced_unsync(&mut self, page: NodeId, path: &[usize]) -> Result<Vec<u8>> {
        let sync_id: NodeId =
            synced::parse_sync_id(&synced::placeholder_at(self.open_page(page)?, path)?)?;
        self.require_synced(sync_id)?;
        self.load_into_cache(sync_id, false)?;
        let content = self
            .pages
            .get(sync_id)
            .ok_or_else(|| Error::NotFound(format!("synced block {sync_id}")))?
            .blocks()?;
        let ((), update, _) =
            self.edit_page_update(page, |doc| synced::unsync_at(doc, path, &content).map(drop))?;
        if self.synced_occurrences(sync_id)?.is_empty() {
            self.delete_synced_document(sync_id)?;
        }
        Ok(update)
    }

    fn delete_synced_document(&mut self, id: NodeId) -> Result<()> {
        let key = id.to_string();
        let tx = self.conn.transaction()?;
        docs::delete(&tx, &key)?;
        tx.execute("DELETE FROM synced_state WHERE sync_id = ?1", [&key])?;
        tx.commit()?;
        self.pages.remove(id);
        Ok(())
    }

    /// Deletes shared documents that have had no occurrence for at least
    /// `min_age_ms`. Returns how many were deleted.
    pub fn synced_gc(&mut self, min_age_ms: i64) -> Result<usize> {
        let cutoff = self.now().saturating_sub(min_age_ms);
        let doomed: Vec<String> = self
            .conn
            .prepare_cached(
                "SELECT s.sync_id FROM synced_state s
                 WHERE s.orphaned_at <= ?1
                   AND NOT EXISTS (SELECT 1 FROM synced_refs r WHERE r.sync_id = s.sync_id)",
            )?
            .query_map(params![cutoff], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut removed = 0;
        for id in doomed {
            if let Ok(id) = id.parse::<NodeId>() {
                self.delete_synced_document(id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// The shared document as a page, for tests and exports.
    #[doc(hidden)]
    pub fn synced_doc(&mut self, id: NodeId) -> Result<&PageDoc> {
        self.require_synced(id)?;
        self.load_into_cache(id, false)?;
        self.pages
            .get(id)
            .ok_or_else(|| Error::NotFound(format!("synced block {id}")))
    }
}
