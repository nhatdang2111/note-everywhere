//! Page history and structured import for the UI.

use ne_core::NodeId;
use ne_core::page::{Block, HistoryReport};
use serde_json::{Value, json};

use super::{AppCore, AppResult, CoreEvent, lock, node_id};

impl AppCore {
    /// Hands a change that core made to a page to an editor that has it open,
    /// the way an update from another device is.
    pub(crate) fn push_page_update(&self, page: NodeId, update: Vec<u8>) {
        if update.is_empty() || !lock(&self.inner.open_pages).contains(&page) {
            return;
        }
        self.emit(CoreEvent::PageRemoteUpdate {
            page_id: page.to_string(),
            update,
        });
    }

    pub async fn page_history(&self, id: &str) -> AppResult<HistoryReport> {
        let page = node_id(id)?;
        self.store(|ws| ws.page_history(page))
    }

    pub async fn page_version_preview(&self, id: &str, version_id: &str) -> AppResult<Vec<Block>> {
        let page = node_id(id)?;
        self.store(|ws| ws.page_version_blocks(page, version_id))
    }

    pub async fn page_version_restore(&self, id: &str, version_id: &str) -> AppResult<()> {
        let page = node_id(id)?;
        let (update, _) = self.store(|ws| ws.restore_page_version(page, version_id))?;
        self.push_page_update(page, update);
        self.after_local_change();
        self.schedule_maintenance();
        Ok(())
    }

    /// Imports an export folder (Markdown and CSV) the person picks.
    pub async fn import_structured(&self, parent_id: Option<&str>) -> AppResult<Option<Value>> {
        let parent = parent_id.map(node_id).transpose()?;
        let dialogs = self.inner.deps.dialogs.clone();
        let picked = self
            .blocking(move |_| Ok(dialogs.pick_folder("Import a Markdown and CSV export…")))
            .await?;
        let Some(dir) = picked else {
            return Ok(None);
        };
        let report = self
            .blocking(move |core| core.store(|ws| ws.import_structured_dir(parent, &dir)))
            .await?;
        self.emit(CoreEvent::TreeChanged {
            node_ids: parent_id.map(str::to_owned).into_iter().collect(),
        });
        self.after_local_change();
        self.schedule_maintenance();
        Ok(Some(json!({
            "folders": report.folders,
            "pages": report.pages,
            "databases": report.databases,
            "rows": report.rows,
            "attachments": report.attachments,
            "linksRewritten": report.links_rewritten,
            "linksUnresolved": report.links_unresolved,
            "imagesMissing": report.images_missing,
            "skipped": report.skipped,
        })))
    }
}
