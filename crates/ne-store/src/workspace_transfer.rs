//! Attachments, Markdown import and export, and index maintenance of the
//! [`Workspace`].

use std::path::Path;
use std::time::Duration;

use ne_core::NodeId;
use ne_core::markdown::{
    Attachment, ExportReport, ImportReport, ImportSink, PageSource, export_tree, import_directory,
};
use ne_core::page::PageDoc;
use rusqlite::Connection;
use rusqlite::types::ValueRef;

use crate::blobs::{self, AttachmentInfo, BlobStore, GcReport};
use crate::docs::{self, DocKind, DocRef, TREE_DOC_ID};
use crate::error::{Error, Result};
use crate::nodes;
use crate::outbox;
use crate::pages;
use crate::workspace::{Workspace, load_page};
use crate::workspace_tree::{apply_tree_changes, maybe_compact};

/// The store seen as the source of an export.
struct StoreSource<'a> {
    conn: &'a Connection,
    blobs: &'a BlobStore,
    peer_id: u64,
}

fn core_error(err: &Error) -> ne_core::Error {
    ne_core::Error::Io(err.to_string())
}

impl PageSource for StoreSource<'_> {
    fn page_doc(&self, id: NodeId) -> ne_core::Result<Option<PageDoc>> {
        load_page(self.conn, id, self.peer_id).map_err(|e| core_error(&e))
    }

    fn attachment(&self, hash: &str) -> ne_core::Result<Option<Attachment>> {
        let Some(info) = blobs::info(self.conn, hash).map_err(|e| core_error(&e))? else {
            return Ok(None);
        };
        let bytes = self
            .blobs
            .read(hash, info.size)
            .map_err(|e| core_error(&e))?;
        Ok(Some(Attachment {
            file_name: info.file_name,
            bytes,
        }))
    }
}

/// Collects imported pages and stores attachments as they are found.
struct ImportCollector<'a> {
    conn: &'a Connection,
    blobs: &'a BlobStore,
    now: i64,
    pages: Vec<(NodeId, PageDoc)>,
}

impl ImportSink for ImportCollector<'_> {
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

impl Workspace {
    // ---- attachments ----

    /// Stores a file given as bytes. The content is addressed by its SHA-256;
    /// adding the same content again stores nothing new and returns the
    /// existing record (with the name it was first added under).
    pub fn add_attachment(&mut self, file_name: &str, bytes: &[u8]) -> Result<AttachmentInfo> {
        let hash = self.blobs.put_bytes(bytes)?;
        blobs::register(&self.conn, &hash, file_name, bytes.len() as u64, self.now())?;
        self.attachment(&hash)?
            .ok_or_else(|| Error::Corrupt("attachment row missing after insert".into()))
    }

    /// Stores a file from disk without loading it into memory, with the same
    /// deduplication as [`Self::add_attachment`].
    pub fn add_attachment_from_path(&mut self, path: &Path) -> Result<AttachmentInfo> {
        let (hash, size) = self.blobs.put_file(path)?;
        let name = path.file_name().map_or_else(
            || "attachment".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        blobs::register(&self.conn, &hash, &name, size, self.now())?;
        self.attachment(&hash)?
            .ok_or_else(|| Error::Corrupt("attachment row missing after insert".into()))
    }

    /// The record of an attachment, if stored.
    pub fn attachment(&self, hash: &str) -> Result<Option<AttachmentInfo>> {
        if !blobs::is_valid_hash(hash) {
            return Err(Error::InvalidInput(format!(
                "{hash:?} is not an attachment hash"
            )));
        }
        blobs::info(&self.conn, hash)
    }

    /// The content of an attachment.
    pub fn read_attachment(&self, hash: &str) -> Result<Vec<u8>> {
        let info = self
            .attachment(hash)?
            .ok_or_else(|| Error::NotFound(format!("attachment {hash}")))?;
        self.blobs.read(hash, info.size)
    }

    /// Where an attachment's file lives, for streaming it to the webview.
    pub fn attachment_path(&self, hash: &str) -> Result<std::path::PathBuf> {
        self.attachment(hash)?
            .ok_or_else(|| Error::NotFound(format!("attachment {hash}")))?;
        self.blobs.existing_path(hash)
    }

    /// Removes attachments that no page shows. Only attachments (and stray
    /// files) at least `min_age` old are removed, so a file added just before
    /// the image block that shows it is saved is not collected in between.
    pub fn gc_attachments(&mut self, min_age: Duration) -> Result<GcReport> {
        let age_ms = i64::try_from(min_age.as_millis()).unwrap_or(i64::MAX);
        let cutoff = self.now().saturating_sub(age_ms);
        let tx = self.conn.transaction()?;
        let dropped = blobs::drop_unreferenced(&tx, cutoff)?;
        tx.commit()?;
        let mut report = GcReport::default();
        for (hash, size) in &dropped {
            self.blobs.remove(hash)?;
            report.removed += 1;
            report.bytes_freed += size;
        }
        let known = blobs::known_hashes(&self.conn)?;
        report.orphan_files = self.blobs.sweep(&known, min_age)?;
        Ok(report)
    }

    // ---- Markdown ----

    /// Exports `root` (a page or folder, with everything below it that is not
    /// trashed), or the whole workspace for `None`, into `dest`. See
    /// [`ne_core::markdown::export_tree`] for the layout.
    pub fn export_markdown(&self, root: Option<NodeId>, dest: &Path) -> Result<ExportReport> {
        let source = StoreSource {
            conn: &self.conn,
            blobs: &self.blobs,
            peer_id: self.peer_id,
        };
        Ok(export_tree(&self.tree, root, &source, dest)?)
    }

    /// Imports a folder of Markdown files as children of `parent` (`None` is
    /// the top level), keeping the hierarchy and relative images. See
    /// [`ne_core::markdown::import_directory`]. Everything is stored in one
    /// transaction; if the import fails nothing is kept.
    pub fn import_markdown_dir(
        &mut self,
        parent: Option<NodeId>,
        dir: &Path,
    ) -> Result<ImportReport> {
        let mut sink = ImportCollector {
            conn: &self.conn,
            blobs: &self.blobs,
            now: self.config.clock.now_ms(),
            pages: Vec::new(),
        };
        let report = match import_directory(&self.tree, parent, dir, &mut sink) {
            Ok(report) => report,
            Err(err) => {
                self.recover_tree();
                return Err(err.into());
            }
        };
        let imported = sink.pages;
        match self.persist_import(imported) {
            Ok(()) => Ok(report),
            Err(err) => {
                self.recover_tree();
                Err(err)
            }
        }
    }

    fn persist_import(&mut self, imported: Vec<(NodeId, PageDoc)>) -> Result<()> {
        let now = self.now();
        let version = self.tree.version();
        let changed = self.tree.take_changes();
        let tx = self.conn.transaction()?;
        if version != self.tree_version {
            let update = self.tree.export_updates_since(&self.tree_version)?;
            docs::append_update(&tx, TREE_DOC_ID, &update, &version, now)?;
            outbox::enqueue(&tx, &DocRef::Tree, &update, now)?;
        }
        for (id, doc) in &imported {
            doc.set_peer_id(self.peer_id)?;
            let key = id.to_string();
            docs::insert(
                &tx,
                &key,
                DocKind::Page,
                &doc.export_snapshot()?,
                &doc.version(),
                now,
            )?;
            outbox::enqueue(&tx, &DocRef::Page(*id), &doc.export_all_updates()?, now)?;
        }
        apply_tree_changes(
            &tx,
            &self.tree,
            &mut self.pages,
            self.peer_id,
            &changed,
            &[],
        )?;
        maybe_compact(&tx, &self.config, TREE_DOC_ID, now, || {
            Ok((self.tree.export_snapshot()?, version.clone()))
        })?;
        tx.commit()?;
        self.tree_version = version;
        Ok(())
    }

    // ---- derived data ----

    /// Recomputes every derived table (node index, search, links, attachment
    /// references) from the stored documents. The incremental updates keep
    /// them equal to this; it exists for repair and for tests.
    pub fn rebuild_indexes(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        pages::clear_all(&tx)?;
        nodes::rebuild(&tx, &self.tree)?;
        let mut stack = vec![None];
        while let Some(parent) = stack.pop() {
            for info in self.tree.children(parent, true)? {
                stack.push(Some(info.id));
                match load_page(&tx, info.id, self.peer_id)? {
                    Some(doc) if info.kind == ne_core::tree::NodeKind::Page => {
                        pages::index_page(&tx, info.id, &doc)?;
                    }
                    _ => pages::reindex_title(&tx, info.id)?,
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// A deterministic text dump of every derived table, keyed by node id
    /// rather than internal row ids, so two databases can be compared.
    #[doc(hidden)]
    pub fn dump_derived_indexes(&self) -> Result<String> {
        let mut out = String::new();
        let mut dump = |title: &str, sql: &str| -> Result<()> {
            out.push_str(&format!("== {title}\n"));
            let mut statement = self.conn.prepare(sql)?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let cells: Vec<String> = (0..columns)
                    .map(|i| match row.get_ref(i) {
                        Ok(ValueRef::Null) => "NULL".to_owned(),
                        Ok(ValueRef::Integer(n)) => n.to_string(),
                        Ok(ValueRef::Real(x)) => x.to_string(),
                        Ok(ValueRef::Text(text)) => {
                            String::from_utf8_lossy(text).replace('\n', "\\n")
                        }
                        Ok(ValueRef::Blob(blob)) => format!("blob({} bytes)", blob.len()),
                        Err(e) => e.to_string(),
                    })
                    .collect();
                out.push_str(&cells.join(" | "));
                out.push('\n');
            }
            Ok(())
        };
        dump(
            "nodes",
            "SELECT id, parent_id, position, kind, title, icon, favorite, created_at, updated_at, \
             trashed_at, hidden FROM nodes ORDER BY id",
        )?;
        dump(
            "page_text",
            "SELECT n.id, p.body, p.word_count FROM page_text p JOIN nodes n ON n.rid = p.rid \
             ORDER BY n.id",
        )?;
        dump("links", "SELECT * FROM links ORDER BY 1, 2, 3")?;
        dump(
            "attachment_refs",
            "SELECT * FROM attachment_refs ORDER BY 1, 2",
        )?;
        dump(
            "attachment_counts",
            "SELECT hash, ref_count FROM attachments ORDER BY hash",
        )?;
        dump(
            "indexed",
            "SELECT n.id FROM nodes n WHERE n.rid IN (SELECT rowid FROM search) ORDER BY n.id",
        )?;
        dump("search_rows", "SELECT COUNT(*) FROM search")?;
        Ok(out)
    }
}
