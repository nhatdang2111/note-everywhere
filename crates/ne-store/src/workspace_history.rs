//! Page history on the [`Workspace`]: versions, previews and restoring.

use ne_core::NodeId;
use ne_core::page::{Block, HistoryReport, PageDoc};

use crate::error::Result;
use crate::workspace::{Change, Workspace};

impl Workspace {
    /// Edits a page from Rust like [`Workspace::edit_page`] and also returns
    /// the update that holds exactly the change, which the shell sends to the
    /// editor if the page is open. The update is empty when nothing changed.
    pub fn edit_page_update<R>(
        &mut self,
        id: NodeId,
        edit: impl FnOnce(&PageDoc) -> ne_core::Result<R>,
    ) -> Result<(R, Vec<u8>, Change)> {
        self.load_into_cache(id, true)?;
        let Some(doc) = self.pages.get(id) else {
            return Err(crate::Error::NotFound(format!("page {id}")));
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
            return Ok((value, Vec::new(), Change::default()));
        }
        let change = self.commit_page_update(id, &update, true, false)?;
        Ok((value, update, change))
    }

    /// The versions of a page, newest first, and how far back they go; see
    /// [`ne_core::page::HistoryReport`].
    pub fn page_history(&mut self, id: NodeId) -> Result<HistoryReport> {
        Ok(self.open_page(id)?.history()?)
    }

    /// The page as it was at `version`, as blocks. Nothing changes.
    pub fn page_version_blocks(&mut self, id: NodeId, version: &str) -> Result<Vec<Block>> {
        Ok(self.open_page(id)?.blocks_at(version)?)
    }

    /// Restores a page to `version` as a new change that sync delivers to
    /// every device. Returns the update (for the editor, if the page is
    /// open) and what changed.
    pub fn restore_page_version(&mut self, id: NodeId, version: &str) -> Result<(Vec<u8>, Change)> {
        let ((), update, change) = self.edit_page_update(id, |doc| doc.restore_to(version))?;
        Ok((update, change))
    }
}
