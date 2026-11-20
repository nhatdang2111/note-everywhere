//! Importing the folder of a "Markdown & CSV" export from another notes app.
//!
//! The layout this reads is the one the common notes app documents for its
//! export (see the research notes of this project):
//!
//! * a page is `<Title> <32 hex id>.md`; its subpages and attachments sit in a
//!   directory next to it named like the file without the extension;
//! * a database is `<Name> <32 hex id>.csv` (or `..._all.csv`, preferred when
//!   both exist), and its row pages are the `.md` files of a directory named
//!   like the CSV without the extension (with or without the id);
//! * links between pages are percent-encoded relative paths; images are
//!   percent-encoded relative paths into the page's directory.
//!
//! # What becomes what
//!
//! * Directories and `.md` files keep the hierarchy. A title is the file name
//!   without the id, or the page's own first heading when it has one.
//! * Links to other exported pages, databases and rows become page links (or
//!   `child_page` blocks for a subpage on a line of its own), found by the id
//!   at the end of the target's name, or by path when there is none.
//! * Images are read, stored as attachments and shown by `attachment:` sources.
//! * Each CSV with its row directory becomes a database node. The first column
//!   is the row title; every other column becomes a property whose type is
//!   guessed from its values (see [`infer`]). Rows are matched to their pages by
//!   title; a CSV row without a page gets an empty page, a page without a CSV
//!   row becomes a row with no values. The property block at the top of an
//!   exported row page is dropped, as the CSV carries it.
//!
//! # Not supported
//!
//! The export carries no schema or view settings, so there are no select
//! colours, views, filters, formulas, rollups or relations to databases that
//! are not part of the export; those columns come in as text, select or
//! multi-select by the guesses above. HTML exports, comments, icons and cover
//! images, embeds and bookmarks, table blocks, and files that are not images
//! are not imported (the text of a link to such a file stays, its target is
//! not kept). Times lose their time zone. The importer reports what it passed
//! over in [`StructuredReport`].

mod csv;
mod dates;
mod infer;
mod scan;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use self::csv::parse_csv;
use self::infer::{infer, linked_ids, to_cell, trailing_hex_id};
use self::scan::{DbPlan, Item, PagePlan, is_markdown, match_key, plan, resolve, scan, split_name};
use super::folder::{ImportSink, MAX_IMPORT_FILE_BYTES, normalize};
use super::import::{ImportEnv, PageTarget, import_blocks, is_external};
use crate::database::{CellValue, DatabaseDoc, PropertyType, RelationConfig, fold_name};
use crate::error::{Error, Result};
use crate::ids::NodeId;
use crate::page::{PageDoc, attachment_src};
use crate::tree::{Position, WorkspaceTree};

pub use self::csv::parse_csv as read_csv;
pub use self::dates::parse_cell_date;

/// Where an import hands over what it builds.
pub trait StructuredSink: ImportSink {
    /// Receives the finished document of a database node already created in
    /// the tree. It carries a random replica id; the sink should give it the
    /// device's own with [`DatabaseDoc::set_peer_id`] before editing it.
    fn save_database(&mut self, id: NodeId, doc: DatabaseDoc) -> Result<()>;
}

/// What an import created and what it passed over.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StructuredReport {
    /// Folder nodes created for directories without a page of the same name.
    pub folders: usize,
    /// Page nodes created, rows not counted.
    pub pages: usize,
    /// Database nodes created.
    pub databases: usize,
    /// Row pages created.
    pub rows: usize,
    /// Distinct image files stored as attachments.
    pub attachments: usize,
    /// Links to exported pages that were turned into page links.
    pub links_rewritten: usize,
    /// Links that look like links to exported pages but point at nothing in
    /// the export.
    pub links_unresolved: usize,
    /// Images that could not be found or read.
    pub images_missing: usize,
    /// Everything passed over, with the reason: a file too large, a CSV that
    /// does not parse, a duplicate, a file kind that is not imported.
    pub skipped: Vec<String>,
}

struct Pending {
    id: NodeId,
    md: PathBuf,
    is_row: bool,
}

struct RowPlan {
    id: NodeId,
    csv_row: Option<usize>,
}

struct DbWork {
    id: NodeId,
    header: Vec<String>,
    csv_rows: Vec<Vec<String>>,
    rows: Vec<RowPlan>,
}

struct Importer<'a> {
    tree: &'a WorkspaceTree,
    sink: &'a mut dyn StructuredSink,
    report: StructuredReport,
    pending: Vec<Pending>,
    by_hex: HashMap<String, NodeId>,
    by_path: HashMap<PathBuf, NodeId>,
    databases: Vec<DbWork>,
    database_of_row: HashMap<NodeId, usize>,
    stored: HashMap<PathBuf, String>,
}

fn read_limited(path: &Path) -> Result<Option<Vec<u8>>> {
    let io = |e: std::io::Error| Error::Io(format!("{}: {e}", path.display()));
    if fs::metadata(path).map_err(io)?.len() > MAX_IMPORT_FILE_BYTES {
        return Ok(None);
    }
    Ok(Some(fs::read(path).map_err(io)?))
}

impl Importer<'_> {
    fn create_items(&mut self, items: Vec<Item>, parent: Option<NodeId>) -> Result<()> {
        for item in items {
            match item {
                Item::Folder { name, children } => {
                    let id = self.tree.create_folder(parent, Position::End, &name)?;
                    self.report.folders += 1;
                    self.create_items(children, Some(id))?;
                }
                Item::Page(page) => self.create_page(page, parent)?,
                Item::Database(db) => self.create_database(db, parent)?,
            }
        }
        Ok(())
    }

    fn create_page(&mut self, page: PagePlan, parent: Option<NodeId>) -> Result<()> {
        let id = self.tree.create_page(parent, Position::End, &page.title)?;
        self.report.pages += 1;
        self.register(id, page.hex.as_deref(), &page.md);
        self.pending.push(Pending {
            id,
            md: page.md,
            is_row: false,
        });
        self.create_items(page.children, Some(id))
    }

    fn register(&mut self, id: NodeId, hex: Option<&str>, md: &Path) {
        if let Some(hex) = hex {
            self.by_hex.entry(hex.to_owned()).or_insert(id);
        }
        self.by_path.insert(normalize(md), id);
    }

    fn create_database(&mut self, db: DbPlan, parent: Option<NodeId>) -> Result<()> {
        let rows_dir = db.rows.clone();
        let table = match read_limited(&db.csv)? {
            Some(bytes) => parse_csv(&String::from_utf8_lossy(&bytes)),
            None => Err(Error::Invalid("larger than the import limit".into())),
        };
        let mut table = match table {
            Ok(table) if !table.is_empty() => table,
            Ok(_) => {
                self.report
                    .skipped
                    .push(format!("{}: the CSV is empty", db.csv.display()));
                return Ok(());
            }
            Err(err) => {
                self.report
                    .skipped
                    .push(format!("{}: {err}", db.csv.display()));
                return Ok(());
            }
        };
        let header = table.remove(0);
        let id = self
            .tree
            .create_database(parent, Position::End, &db.title)?;
        self.report.databases += 1;
        if let Some(hex) = &db.hex {
            self.by_hex.entry(hex.clone()).or_insert(id);
        }
        let index = self.databases.len();
        // Row pages by title, each used once, in file order.
        let mut files: Vec<(String, &scan::FileEntry, bool)> = Vec::new();
        if let Some(dir) = &rows_dir {
            for file in dir.files.iter().filter(|f| is_markdown(f)) {
                files.push((match_key(&scan::title_of(&file.stem)), file, false));
            }
        }
        let mut work = DbWork {
            id,
            header,
            csv_rows: table,
            rows: Vec::new(),
        };
        let mut order: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for (row_index, row) in work.csv_rows.iter().enumerate() {
            let title = row.first().map_or("", |t| t.trim());
            let found = files
                .iter()
                .position(|(key, _, used)| !*used && *key == match_key(title));
            if let Some(i) = found {
                files[i].2 = true;
            }
            order.push((Some(row_index), found));
        }
        for (i, (_, _, used)) in files.iter().enumerate() {
            if !used {
                order.push((None, Some(i)));
            }
        }
        for (csv_row, file) in order {
            let title = match (csv_row, file) {
                (Some(r), _) => work.csv_rows[r].first().map_or("", |t| t.trim()).to_owned(),
                (None, Some(f)) => scan::title_of(&files[f].1.stem),
                (None, None) => String::new(),
            };
            let title = if title.is_empty() {
                "Untitled".to_owned()
            } else {
                title
            };
            let row_id = self.tree.create_page(Some(id), Position::End, &title)?;
            self.report.rows += 1;
            self.database_of_row.insert(row_id, index);
            if let Some(f) = file {
                let entry = files[f].1;
                let (_, hex) = split_name(&entry.stem);
                self.register(row_id, hex.as_deref(), &entry.path);
                self.pending.push(Pending {
                    id: row_id,
                    md: entry.path.clone(),
                    is_row: true,
                });
                // A directory named like the row page holds its subpages.
                if let Some(sub) = rows_dir
                    .as_ref()
                    .and_then(|d| d.dirs.iter().find(|s| s.name == entry.stem))
                {
                    let mut skipped = Vec::new();
                    let children = plan(sub, &mut skipped);
                    self.report.skipped.append(&mut skipped);
                    self.create_items(children, Some(row_id))?;
                }
            } else {
                self.pending_empty(row_id)?;
            }
            work.rows.push(RowPlan {
                id: row_id,
                csv_row,
            });
        }
        self.databases.push(work);
        Ok(())
    }

    fn pending_empty(&mut self, id: NodeId) -> Result<()> {
        self.sink.save_page(id, PageDoc::new()?)
    }

    // ---- pages ----

    fn import_pending(&mut self) -> Result<()> {
        let pending = std::mem::take(&mut self.pending);
        for page in &pending {
            self.import_page(page)?;
        }
        Ok(())
    }

    fn import_page(&mut self, pending: &Pending) -> Result<()> {
        let Some(bytes) = read_limited(&pending.md)? else {
            self.report.skipped.push(format!(
                "{}: larger than the import limit",
                pending.md.display()
            ));
            return self.pending_empty(pending.id);
        };
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if pending.is_row {
            text = strip_property_block(&text);
        }
        let base = pending
            .md
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut env = ExportEnv {
            importer: self,
            base,
            current: pending.id,
            failure: None,
        };
        let imported = import_blocks(&text, &mut env);
        if let Some(err) = env.failure {
            return Err(err);
        }
        if let Some(title) = imported.title.as_deref().filter(|t| !t.trim().is_empty())
            && !pending.is_row
        {
            self.tree.rename(pending.id, title.trim())?;
        }
        let doc = PageDoc::new()?;
        super::write_imported(&doc, &imported.blocks)?;
        self.sink.save_page(pending.id, doc)
    }

    // ---- databases ----

    fn build_databases(&mut self) -> Result<()> {
        let works = std::mem::take(&mut self.databases);
        for (index, work) in works.iter().enumerate() {
            let doc = self.build_database(work, index, &works)?;
            self.sink.save_database(work.id, doc)?;
        }
        Ok(())
    }

    fn build_database(
        &mut self,
        work: &DbWork,
        index: usize,
        all: &[DbWork],
    ) -> Result<DatabaseDoc> {
        let doc = DatabaseDoc::new()?;
        let mut used: HashSet<String> = HashSet::new();
        for column in 1..work.header.len() {
            let cells: Vec<&str> = work
                .csv_rows
                .iter()
                .map(|row| row.get(column).map_or("", String::as_str))
                .collect();
            let base = work.header[column].trim();
            let mut name = if base.is_empty() {
                format!("Column {}", column + 1)
            } else {
                base.to_owned()
            };
            while !used.insert(fold_name(&name)) {
                name.push_str(" 2");
            }
            let name: String = name.chars().take(crate::database::MAX_NAME_CHARS).collect();
            let (mut ty, options) = infer(&cells);
            let mut relation = None;
            if ty == PropertyType::Relation {
                match self.relation_target(&cells, all) {
                    Some(target) => {
                        relation = Some(RelationConfig {
                            database: all[target].id.to_string(),
                            reverse_property: None,
                            backlink_of: None,
                        });
                    }
                    None => ty = PropertyType::Text,
                }
            }
            let property = doc.add_property(&name, ty, relation)?;
            let mut ids: HashMap<String, String> = HashMap::new();
            for option in &options {
                if let Ok(id) = doc.add_option(&property, option, None) {
                    ids.insert(fold_name(option), id);
                } else if let Some(existing) = ids.get(&fold_name(option)) {
                    let existing = existing.clone();
                    ids.insert(fold_name(option), existing);
                }
            }
            for row in &work.rows {
                let Some(csv_row) = row.csv_row else { continue };
                let text = cells[csv_row];
                let value = if ty == PropertyType::Relation {
                    self.relation_value(text, index, all)
                } else {
                    to_cell(ty, text, &|name| ids.get(&fold_name(name)).cloned())
                };
                if let Some(value) = value {
                    doc.set_cell(&row.id.to_string(), &property, Some(value))?;
                }
            }
        }
        Ok(doc)
    }

    /// The one database every linked row of a relation column belongs to.
    fn relation_target(&self, cells: &[&str], all: &[DbWork]) -> Option<usize> {
        let mut target: Option<usize> = None;
        for cell in cells.iter().filter(|c| !c.trim().is_empty()) {
            for hex in linked_ids(cell) {
                let row = self.by_hex.get(&hex)?;
                let database = *self.database_of_row.get(row)?;
                if *target.get_or_insert(database) != database {
                    return None;
                }
            }
        }
        target.filter(|&t| t < all.len())
    }

    fn relation_value(&self, text: &str, _index: usize, _all: &[DbWork]) -> Option<CellValue> {
        let rows: Vec<String> = linked_ids(text)
            .iter()
            .filter_map(|hex| self.by_hex.get(hex))
            .map(ToString::to_string)
            .collect();
        (!rows.is_empty()).then_some(CellValue::Relation(rows))
    }
}

/// Removes the block of `Name: value` lines an exported row page has under its
/// title, since the CSV carries those values.
fn strip_property_block(text: &str) -> String {
    let mut lines = text.lines();
    let mut out: Vec<&str> = Vec::new();
    let mut seen_title = false;
    for line in lines.by_ref() {
        out.push(line);
        if line.starts_with("# ") {
            seen_title = true;
            break;
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    if !seen_title {
        return text.to_owned();
    }
    let rest: Vec<&str> = lines.collect();
    let mut start = 0;
    while rest.get(start).is_some_and(|l| l.trim().is_empty()) {
        start += 1;
    }
    let mut end = start;
    while rest.get(end).is_some_and(|l| looks_like_property(l)) {
        end += 1;
    }
    // Only a block of property lines that ends at a blank line (or the end of
    // the file) is a property block.
    let block_ends = rest.get(end).is_none_or(|l| l.trim().is_empty());
    let skip_to = if end > start && block_ends {
        end
    } else {
        start
    };
    out.push("");
    out.extend(&rest[skip_to..]);
    out.join("\n")
}

fn looks_like_property(line: &str) -> bool {
    let Some((name, _)) = line.split_once(": ") else {
        return line.trim_end().ends_with(':') && !line.starts_with(['#', '-', '*', '>', ' ']);
    };
    !name.is_empty()
        && name.chars().count() <= 60
        && !name.starts_with(['#', '-', '*', '>', '[', '!', '`', ' '])
        && !name.contains(['[', ']', '(', ')', '`', '|'])
}

struct ExportEnv<'a, 'b> {
    importer: &'a mut Importer<'b>,
    base: PathBuf,
    current: NodeId,
    failure: Option<Error>,
}

impl ImportEnv for ExportEnv<'_, '_> {
    fn image(&mut self, src: &str) -> String {
        if is_external(src) || self.failure.is_some() {
            return src.to_owned();
        }
        let path = resolve(&self.base, src);
        if let Some(stored) = self.importer.stored.get(&path) {
            return stored.clone();
        }
        let bytes = match read_limited(&path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(_) => {
                self.importer.report.images_missing += 1;
                return src.to_owned();
            }
        };
        let name = path
            .file_name()
            .map_or_else(|| "image".to_owned(), |n| n.to_string_lossy().into_owned());
        match self.importer.sink.add_attachment(&name, bytes) {
            Ok(hash) => {
                let stored = attachment_src(&hash);
                self.importer.report.attachments += 1;
                self.importer.stored.insert(path, stored.clone());
                stored
            }
            Err(err) => {
                self.failure = Some(err);
                src.to_owned()
            }
        }
    }

    fn page(&mut self, href: &str) -> Option<PageTarget> {
        if is_external(href) {
            return None;
        }
        let path = resolve(&self.base, href);
        let by_hex = path
            .file_stem()
            .and_then(|s| trailing_hex_id(&s.to_string_lossy()))
            .and_then(|hex| self.importer.by_hex.get(&hex).copied());
        let target = by_hex.or_else(|| self.importer.by_path.get(&path).copied());
        let looks_like_page = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("csv"));
        let Some(id) = target else {
            if looks_like_page {
                self.importer.report.links_unresolved += 1;
            }
            return None;
        };
        let info = self.importer.tree.get(id).ok()?;
        self.importer.report.links_rewritten += 1;
        Some(PageTarget {
            id,
            title: info.title,
            is_child: info.parent == Some(self.current),
        })
    }
}

/// Imports the export in `dir` below `parent` (`None` is the top level). The
/// tree nodes are created through `tree`; documents, databases and
/// attachments go to `sink`. If it fails part-way, what was created in the
/// tree is the caller's to discard.
pub fn import_structured(
    tree: &WorkspaceTree,
    parent: Option<NodeId>,
    dir: &Path,
    sink: &mut dyn StructuredSink,
) -> Result<StructuredReport> {
    let root = scan(dir, "", 0)?;
    let mut importer = Importer {
        tree,
        sink,
        report: StructuredReport::default(),
        pending: Vec::new(),
        by_hex: HashMap::new(),
        by_path: HashMap::new(),
        databases: Vec::new(),
        database_of_row: HashMap::new(),
        stored: HashMap::new(),
    };
    let mut skipped = Vec::new();
    let items = plan(&root, &mut skipped);
    importer.report.skipped = skipped;
    importer.create_items(items, parent)?;
    importer.import_pending()?;
    importer.build_databases()?;
    Ok(importer.report)
}
