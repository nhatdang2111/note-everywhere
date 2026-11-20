//! Synced blocks for the UI; see `docs/decisions/0008-synced-blocks.md`.

use std::time::Duration;

use ne_core::NodeId;
use serde::Serialize;

use super::types::OpenPage;
use super::{AppCore, AppError, AppResult, lock, node_id};

/// Shared documents with no occurrence are deleted after this long.
pub const SYNCED_ORPHAN_KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedOccurrence {
    pub page_id: String,
    pub title: String,
    pub trashed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedBlockInfo {
    pub sync_id: String,
    pub occurrences: usize,
    pub orphaned_at: Option<i64>,
}

fn sync_node(id: &str) -> AppResult<NodeId> {
    id.parse()
        .map_err(|_| AppError::not_found("That synced block"))
}

impl AppCore {
    pub async fn synced_create(
        &self,
        page_id: &str,
        start: usize,
        end: usize,
    ) -> AppResult<String> {
        let page = node_id(page_id)?;
        let (sync_id, update) = self.store(|ws| ws.synced_create(page, start, end))?;
        self.push_page_update(page, update);
        self.after_local_change();
        self.schedule_maintenance();
        Ok(sync_id.to_string())
    }

    pub async fn synced_insert(
        &self,
        page_id: &str,
        parent_path: &[usize],
        index: usize,
        sync_id: &str,
    ) -> AppResult<()> {
        let (page, sync) = (node_id(page_id)?, sync_node(sync_id)?);
        let update = self.store(|ws| ws.synced_insert(page, parent_path, index, sync))?;
        self.push_page_update(page, update);
        self.after_local_change();
        Ok(())
    }

    pub async fn synced_open(&self, sync_id: &str) -> AppResult<OpenPage> {
        let sync = sync_node(sync_id)?;
        let snapshot = self.store(|ws| ws.synced_open(sync))?;
        lock(&self.inner.open_pages).insert(sync);
        Ok(OpenPage { snapshot })
    }

    pub async fn synced_apply_update(&self, sync_id: &str, update: &[u8]) -> AppResult<()> {
        let sync = sync_node(sync_id)?;
        if update.is_empty() {
            return Err(AppError::invalid("The update is empty."));
        }
        self.store(|ws| ws.synced_apply_update(sync, update))?;
        self.after_local_change();
        self.schedule_maintenance();
        Ok(())
    }

    pub async fn synced_close(&self, sync_id: &str) -> AppResult<()> {
        let sync = sync_node(sync_id)?;
        lock(&self.inner.open_pages).remove(&sync);
        Ok(())
    }

    pub async fn synced_unsync(&self, page_id: &str, path: &[usize]) -> AppResult<()> {
        let page = node_id(page_id)?;
        let update = self.store(|ws| ws.synced_unsync(page, path))?;
        self.push_page_update(page, update);
        self.after_local_change();
        Ok(())
    }

    pub async fn synced_occurrences(&self, sync_id: &str) -> AppResult<Vec<SyncedOccurrence>> {
        let sync = sync_node(sync_id)?;
        self.store(|ws| {
            Ok(ws
                .synced_occurrences(sync)?
                .into_iter()
                .map(|o| SyncedOccurrence {
                    page_id: o.page.to_string(),
                    title: o.title,
                    trashed: o.trashed,
                })
                .collect())
        })
    }

    pub async fn synced_list(&self) -> AppResult<Vec<SyncedBlockInfo>> {
        self.store(|ws| {
            Ok(ws
                .synced_list()?
                .into_iter()
                .map(|i| SyncedBlockInfo {
                    sync_id: i.id.to_string(),
                    occurrences: i.occurrences,
                    orphaned_at: i.orphaned_at,
                })
                .collect())
        })
    }

    /// Deletes shared documents that nothing has shown for 30 days.
    pub(crate) fn synced_collect_orphans(&self) {
        let keep = i64::try_from(SYNCED_ORPHAN_KEEP.as_millis()).unwrap_or(i64::MAX);
        // A workspace that is closed or a failing collection is retried at the
        // next start.
        let _ = self.with_ws(|ws| ws.synced_gc(keep).map_err(AppError::from));
    }
}
