//! Structured import on the [`Workspace`]: a folder exported from another
//! notes app as Markdown and CSV, see `ne_core::markdown::structured`.

use std::path::Path;

use ne_core::NodeId;
use ne_core::database::DatabaseDoc;
use ne_core::markdown::ImportSink;
use ne_core::markdown::structured::{StructuredReport, StructuredSink, import_structured};
use ne_core::page::PageDoc;
use rusqlite::Connection;

use crate::blobs::{self, BlobStore};
use crate::error::{Error, Result};
use crate::workspace::Workspace;

struct Collector<'a> {
    conn: &'a Connection,
    blobs: &'a BlobStore,
    now: i64,
    pages: Vec<(NodeId, PageDoc)>,
    databases: Vec<(NodeId, DatabaseDoc)>,
}

fn core_error(err: &Error) -> ne_core::Error {
    ne_core::Error::Io(err.to_string())
}

impl ImportSink for Collector<'_> {
    fn add_attachment(&mut self, file_name: &str, bytes: Vec<u8>) -> ne_core::Result<String> {
        let size = bytes.len() as u64;
        let hash = self.blobs.put_bytes(&bytes).map_err(|e| core_error(&e))?;
        blobs::register(self.conn, &hash, file_name, size, self.now).map_err(|e| core_error(&e))?;
        Ok(hash)
    }

    fn save_page(&mut self, id: NodeId, doc: PageDoc) -> ne_core::Result<()> {
        self.pages.push((id, doc));
        Ok(())
    }
}

impl StructuredSink for Collector<'_> {
    fn save_database(&mut self, id: NodeId, doc: DatabaseDoc) -> ne_core::Result<()> {
        self.databases.push((id, doc));
        Ok(())
    }
}

impl Workspace {
    /// Imports an export folder of Markdown and CSV files below `parent`
    /// (`None` is the top level): pages keep their hierarchy, links between
    /// exported files become page links, images become attachments, and each
    /// CSV with its row pages becomes a database. Everything is stored in one
    /// transaction; if the import fails nothing is kept. The report lists what
    /// was created and what was passed over.
    pub fn import_structured_dir(
        &mut self,
        parent: Option<NodeId>,
        dir: &Path,
    ) -> Result<StructuredReport> {
        let mut sink = Collector {
            conn: &self.conn,
            blobs: &self.blobs,
            now: self.config.clock.now_ms(),
            pages: Vec::new(),
            databases: Vec::new(),
        };
        let report = match import_structured(&self.tree, parent, dir, &mut sink) {
            Ok(report) => report,
            Err(err) => {
                self.recover_tree();
                return Err(err.into());
            }
        };
        let (pages, databases) = (sink.pages, sink.databases);
        match self.persist_import(pages, databases) {
            Ok(()) => Ok(report),
            Err(err) => {
                self.recover_tree();
                Err(err)
            }
        }
    }
}
