//! Exporting a subtree of the workspace to a folder of Markdown files and
//! importing such a folder back into the tree.
//!
//! The layout mirrors the hierarchy: a folder node is a directory, a page is
//! `<name>.md`, and the children of a page live in a directory with the same
//! name next to its file. Attachments sit in an `assets` directory beside the
//! pages that show them.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use super::export::{ExportEnv, PageRef, export_blocks};
use super::filenames::NameAllocator;
use super::import::{ImportEnv, PageTarget, import_blocks, is_external};
use super::inline::{Context, render_inline};
use crate::error::{Error, Result};
use crate::ids::NodeId;
use crate::page::{Inline, PageDoc, attachment_src};
use crate::tree::{NodeInfo, NodeKind, Position, WorkspaceTree};

/// Name of the directory that holds attachments beside the pages.
pub const ASSETS_DIR: &str = "assets";

/// Deepest directory nesting an import follows.
const MAX_IMPORT_DEPTH: usize = 64;

/// Largest file an import reads (a page or an attachment), in bytes.
const MAX_IMPORT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// A stored attachment as the exporter needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The original file name; its extension is kept in the export.
    pub file_name: String,
    /// The content.
    pub bytes: Vec<u8>,
}

/// Where an export reads page documents and attachments from. `ne-core` has no
/// storage of its own; the store implements this.
pub trait PageSource {
    /// The document of a page, or `None` if it has none yet (it is exported
    /// with only its title).
    fn page_doc(&self, id: NodeId) -> Result<Option<PageDoc>>;
    /// The attachment with this content hash, or `None` if it is not stored.
    fn attachment(&self, hash: &str) -> Result<Option<Attachment>>;
}

/// What an export wrote.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// Directories created for folder nodes.
    pub folders: usize,
    /// Markdown files written.
    pub pages: usize,
    /// Attachment files written.
    pub attachments: usize,
    /// Hashes of attachments a page shows but the source does not have.
    pub missing_attachments: Vec<String>,
}

struct Entry {
    id: NodeId,
    kind: NodeKind,
    title: String,
    dir: Vec<String>,
    stem: String,
    has_children: bool,
}

fn io_error(path: &Path, err: &std::io::Error) -> Error {
    Error::Io(format!("{}: {err}", path.display()))
}

fn plan(tree: &WorkspaceTree, root: Option<NodeId>) -> Result<Vec<Entry>> {
    let first = match root {
        Some(id) => vec![tree.get(id)?],
        None => tree.children(None, false)?,
    };
    let mut allocators: HashMap<Vec<String>, NameAllocator> = HashMap::new();
    let mut entries = Vec::new();
    let mut work: Vec<(Vec<NodeInfo>, Vec<String>)> = vec![(first, Vec::new())];
    while let Some((siblings, dir)) = work.pop() {
        let allocator = allocators.entry(dir.clone()).or_insert_with(|| {
            let mut allocator = NameAllocator::new();
            allocator.reserve(ASSETS_DIR);
            allocator
        });
        let mut next = Vec::new();
        for info in siblings {
            let stem = allocator.unique_stem(&info.title);
            let children = tree.children(Some(info.id), false)?;
            let mut child_dir = dir.clone();
            child_dir.push(stem.clone());
            let has_children = !children.is_empty();
            if has_children {
                next.push((children, child_dir));
            }
            entries.push(Entry {
                id: info.id,
                kind: info.kind,
                title: info.title,
                dir: dir.clone(),
                stem,
                has_children,
            });
        }
        // Pushed in reverse so directories are visited in tree order.
        work.extend(next.into_iter().rev());
    }
    Ok(entries)
}

/// The relative `/`-separated path from directory `from` to file `file` in
/// directory `to`.
fn relative_href(from: &[String], to: &[String], file: &str) -> String {
    let common = from.iter().zip(to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(to[common..].iter().map(String::as_str));
    parts.push(file);
    parts.join("/")
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| io_error(path, &err))?;
    file.write_all(bytes).map_err(|err| io_error(path, &err))
}

struct Exporter<'a> {
    dest: &'a Path,
    entries: &'a [Entry],
    index: HashMap<NodeId, usize>,
    source: &'a dyn PageSource,
    asset_names: HashMap<Vec<String>, NameAllocator>,
    asset_files: HashMap<(Vec<String>, String), String>,
    report: ExportReport,
}

struct PageEnv<'a, 'b> {
    exporter: &'b mut Exporter<'a>,
    dir: &'b [String],
    failure: Option<Error>,
}

impl ExportEnv for PageEnv<'_, '_> {
    fn page_link(&mut self, page_id: &str) -> Option<PageRef> {
        let id: NodeId = page_id.parse().ok()?;
        let target = &self.exporter.entries[*self.exporter.index.get(&id)?];
        if target.kind != NodeKind::Page {
            return None;
        }
        Some(PageRef {
            title: target.title.clone(),
            href: relative_href(self.dir, &target.dir, &format!("{}.md", target.stem)),
        })
    }

    fn attachment_path(&mut self, hash: &str) -> Option<String> {
        let key = (self.dir.to_vec(), hash.to_owned());
        if let Some(name) = self.exporter.asset_files.get(&key) {
            return Some(format!("{ASSETS_DIR}/{name}"));
        }
        if self.failure.is_some() {
            return None;
        }
        match self.write_attachment(hash) {
            Ok(Some(name)) => {
                let href = format!("{ASSETS_DIR}/{name}");
                self.exporter.asset_files.insert(key, name);
                Some(href)
            }
            Ok(None) => {
                let missing = &mut self.exporter.report.missing_attachments;
                if !missing.iter().any(|h| h == hash) {
                    missing.push(hash.to_owned());
                }
                None
            }
            Err(err) => {
                self.failure = Some(err);
                None
            }
        }
    }
}

impl PageEnv<'_, '_> {
    fn write_attachment(&mut self, hash: &str) -> Result<Option<String>> {
        let Some(attachment) = self.exporter.source.attachment(hash)? else {
            return Ok(None);
        };
        let names = self
            .exporter
            .asset_names
            .entry(self.dir.to_vec())
            .or_default();
        let name = names.unique_file_name(&attachment.file_name);
        let mut dir = self.exporter.dest.to_path_buf();
        dir.extend(self.dir);
        dir.push(ASSETS_DIR);
        fs::create_dir_all(&dir).map_err(|err| io_error(&dir, &err))?;
        write_new_file(&dir.join(&name), &attachment.bytes)?;
        self.exporter.report.attachments += 1;
        Ok(Some(name))
    }
}

impl Exporter<'_> {
    fn write_page(&mut self, entry: &Entry) -> Result<()> {
        let mut text = String::new();
        if !entry.title.trim().is_empty() {
            let title = Inline::text(entry.title.replace(['\n', '\r'], " "));
            text.push_str(&format!(
                "# {}\n",
                render_inline(&[title], Context::Heading, false)
            ));
        }
        if let Some(doc) = self.source.page_doc(entry.id)? {
            let blocks = doc.blocks()?;
            let mut env = PageEnv {
                exporter: self,
                dir: &entry.dir,
                failure: None,
            };
            let body = export_blocks(&blocks, &mut env);
            if let Some(err) = env.failure {
                return Err(err);
            }
            if !body.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&body);
            }
        }
        let mut path = self.dest.to_path_buf();
        path.extend(&entry.dir);
        write_new_file(&path.join(format!("{}.md", entry.stem)), text.as_bytes())?;
        self.report.pages += 1;
        Ok(())
    }
}

/// Exports `root` (a folder or a page, with everything below it that is not in
/// the trash) into `dest`, or the whole workspace when `root` is `None`.
///
/// Names come from titles made valid on Windows and unique per directory (see
/// [`super::filenames`]). A page starts with its title as a level-1 heading,
/// which import takes back out. Links between exported pages are relative; a
/// link to a page outside the export keeps the `ne-page:` form. Existing files
/// are never overwritten: a name clash fails with [`Error::Io`].
pub fn export_tree(
    tree: &WorkspaceTree,
    root: Option<NodeId>,
    source: &dyn PageSource,
    dest: &Path,
) -> Result<ExportReport> {
    let entries = plan(tree, root)?;
    let index = entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    fs::create_dir_all(dest).map_err(|err| io_error(dest, &err))?;
    let mut exporter = Exporter {
        dest,
        entries: &entries,
        index,
        source,
        asset_names: HashMap::new(),
        asset_files: HashMap::new(),
        report: ExportReport::default(),
    };
    for entry in &entries {
        if entry.kind == NodeKind::Folder || entry.has_children {
            let mut dir = dest.to_path_buf();
            dir.extend(&entry.dir);
            dir.push(&entry.stem);
            fs::create_dir_all(&dir).map_err(|err| io_error(&dir, &err))?;
            if entry.kind == NodeKind::Folder {
                exporter.report.folders += 1;
            }
        }
        if entry.kind == NodeKind::Page {
            exporter.write_page(entry)?;
        }
    }
    Ok(exporter.report)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Where an import hands over what it builds. The store implements this.
pub trait ImportSink {
    /// Stores an attachment and returns its content hash.
    fn add_attachment(&mut self, file_name: &str, bytes: Vec<u8>) -> Result<String>;
    /// Receives the finished document of a page node already created in the
    /// tree. The document carries a random replica id; the sink should give it
    /// the device's own with [`PageDoc::set_peer_id`] before editing it.
    fn save_page(&mut self, id: NodeId, doc: PageDoc) -> Result<()>;
}

/// What an import created.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Folder nodes created for directories without a page of the same name.
    pub folders: usize,
    /// Page nodes created.
    pub pages: usize,
    /// Image files read and handed to the sink; a file shown by several
    /// pages counts once (the sink may still dedup identical contents).
    pub attachments: usize,
    /// Files that could not be read, relative to the imported directory, each
    /// with the reason.
    pub skipped: Vec<String>,
}

struct Scanned {
    name: String,
    files: Vec<(String, PathBuf)>,
    dirs: Vec<Scanned>,
}

fn lower(name: &str) -> String {
    name.to_lowercase()
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// Lists Markdown files below `path`, skipping hidden entries and symlinks.
/// Directories without any Markdown file below them are left out.
fn scan(path: &Path, name: &str, depth: usize) -> Result<Option<Scanned>> {
    if depth > MAX_IMPORT_DEPTH {
        return Err(Error::Io(format!(
            "directories are nested deeper than {MAX_IMPORT_DEPTH} levels"
        )));
    }
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path).map_err(|err| io_error(path, &err))? {
        let entry = entry.map_err(|err| io_error(path, &err))?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type().map_err(|err| io_error(path, &err))?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if let Some(child) = scan(&entry.path(), &file_name, depth + 1)? {
                dirs.push(child);
            }
        } else if kind.is_file() && is_markdown(&entry.path()) {
            let stem = Path::new(&file_name)
                .file_stem()
                .map_or_else(|| file_name.clone(), |s| s.to_string_lossy().into_owned());
            files.push((stem, entry.path()));
        }
    }
    if files.is_empty() && dirs.is_empty() {
        return Ok(None);
    }
    files.sort_by(|a, b| (lower(&a.0), &a.1).cmp(&(lower(&b.0), &b.1)));
    dirs.sort_by_key(|d| lower(&d.name));
    Ok(Some(Scanned {
        name: name.to_owned(),
        files,
        dirs,
    }))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'%'));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Files and directories that share a name ignoring case.
type Group<'a> = (Vec<&'a (String, PathBuf)>, Vec<&'a Scanned>);

struct Pending {
    id: NodeId,
    path: PathBuf,
}

struct Importer<'a> {
    tree: &'a WorkspaceTree,
    root: PathBuf,
    sink: &'a mut dyn ImportSink,
    report: ImportReport,
    pending: Vec<Pending>,
    pages: HashMap<PathBuf, NodeId>,
    pages_lower: HashMap<String, NodeId>,
    stored: HashMap<PathBuf, String>,
}

impl Importer<'_> {
    fn build(&mut self, dir: &Scanned, parent: Option<NodeId>) -> Result<()> {
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        for file in &dir.files {
            groups.entry(lower(&file.0)).or_default().0.push(file);
        }
        for child in &dir.dirs {
            groups.entry(lower(&child.name)).or_default().1.push(child);
        }
        for (files, dirs) in groups.into_values() {
            if files.is_empty() {
                for child in dirs {
                    let id = self
                        .tree
                        .create_folder(parent, Position::End, &child.name)?;
                    self.report.folders += 1;
                    self.build(child, Some(id))?;
                }
                continue;
            }
            for (n, (stem, path)) in files.into_iter().enumerate() {
                let id = self.tree.create_page(parent, Position::End, stem)?;
                self.report.pages += 1;
                let key = normalize(path);
                self.pages_lower
                    .entry(lower(&key.to_string_lossy()))
                    .or_insert(id);
                self.pages.insert(key, id);
                self.pending.push(Pending {
                    id,
                    path: path.clone(),
                });
                // The directory named like the page holds the page's children.
                if n == 0 {
                    for child in &dirs {
                        self.build(child, Some(id))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    fn import_page(&mut self, pending: &Pending) -> Result<()> {
        let size = fs::metadata(&pending.path)
            .map_err(|err| io_error(&pending.path, &err))?
            .len();
        if size > MAX_IMPORT_FILE_BYTES {
            self.report.skipped.push(format!(
                "{}: larger than the import limit",
                self.relative(&pending.path)
            ));
            return Ok(());
        }
        let bytes = fs::read(&pending.path).map_err(|err| io_error(&pending.path, &err))?;
        let text = String::from_utf8_lossy(&bytes);
        let file_dir = pending
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut env = FolderImportEnv {
            importer: self,
            file_dir,
            current: pending.id,
            failure: None,
        };
        let imported = import_blocks(&text, &mut env);
        if let Some(err) = env.failure {
            return Err(err);
        }
        if let Some(title) = &imported.title {
            self.tree.rename(pending.id, title)?;
        }
        let doc = PageDoc::new()?;
        super::write_imported(&doc, &imported.blocks)?;
        self.sink.save_page(pending.id, doc)
    }
}

struct FolderImportEnv<'a, 'b> {
    importer: &'b mut Importer<'a>,
    file_dir: PathBuf,
    current: NodeId,
    failure: Option<Error>,
}

impl FolderImportEnv<'_, '_> {
    /// The files a relative destination may point at, best guess first, all
    /// inside the imported directory. `#` and `?` may start a fragment or a
    /// query, or be part of a file name, so both readings are offered.
    fn candidates(&self, href: &str) -> Vec<PathBuf> {
        if is_external(href) {
            return Vec::new();
        }
        let decoded = percent_decode(href);
        let mut texts: Vec<&str> = Vec::new();
        for full in [decoded.as_str(), href] {
            let stripped = full.split(['#', '?']).next().unwrap_or_default();
            texts.extend([full, stripped]);
        }
        let mut found: Vec<PathBuf> = Vec::new();
        for text in texts {
            if text.is_empty() {
                continue;
            }
            let resolved = normalize(&self.file_dir.join(text));
            if resolved.starts_with(&self.importer.root) && !found.contains(&resolved) {
                found.push(resolved);
            }
        }
        found
    }

    fn read_image(&mut self, path: &Path) -> Result<Option<String>> {
        if let Some(hash) = self.importer.stored.get(path) {
            return Ok(Some(attachment_src(hash)));
        }
        // A link planted in the folder must not lead outside it: resolve the
        // real path and require it to stay inside the imported directory.
        let Ok(real) = fs::canonicalize(path) else {
            return Ok(None);
        };
        let Ok(meta) = fs::metadata(&real) else {
            return Ok(None);
        };
        if !real.starts_with(&self.importer.root)
            || !meta.is_file()
            || meta.len() > MAX_IMPORT_FILE_BYTES
        {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|err| io_error(path, &err))?;
        let name = path
            .file_name()
            .map_or_else(|| "image".to_owned(), |n| n.to_string_lossy().into_owned());
        let hash = self.importer.sink.add_attachment(&name, bytes)?;
        let src = attachment_src(&hash);
        if self
            .importer
            .stored
            .insert(path.to_path_buf(), hash)
            .is_none()
        {
            self.importer.report.attachments += 1;
        }
        Ok(Some(src))
    }
}

impl ImportEnv for FolderImportEnv<'_, '_> {
    fn image(&mut self, src: &str) -> String {
        if self.failure.is_some() {
            return src.to_owned();
        }
        let candidates = self.candidates(src);
        if candidates.is_empty() {
            return src.to_owned();
        }
        for path in &candidates {
            match self.read_image(path) {
                Ok(Some(stored)) => return stored,
                Ok(None) => {}
                Err(err) => {
                    self.failure = Some(err);
                    return src.to_owned();
                }
            }
        }
        let message = format!("{src}: image not found or not readable");
        self.importer.report.skipped.push(message);
        src.to_owned()
    }

    fn page(&mut self, href: &str) -> Option<PageTarget> {
        let id = self.candidates(href).into_iter().find_map(|path| {
            self.importer
                .pages
                .get(&path)
                .or_else(|| {
                    self.importer
                        .pages_lower
                        .get(&lower(&path.to_string_lossy()))
                })
                .copied()
        })?;
        let info = self.importer.tree.get(id).ok()?;
        Some(PageTarget {
            id,
            title: info.title,
            is_child: info.parent == Some(self.current),
        })
    }
}

/// Imports the Markdown files below `dir` as children of `parent` (`None` is
/// the workspace root), creating nodes in `tree` and handing each page's
/// document and each attachment to `sink`.
///
/// A directory becomes a folder node, unless a Markdown file of the same name
/// sits next to it, in which case the directory holds that page's children.
/// Siblings are ordered by name. A page's title is its leading level-1 heading
/// if it has one, else its file name. Relative image files are stored as
/// attachments; relative links to other imported files become page links.
/// Hidden entries and symbolic links are skipped, and nothing outside `dir` is
/// read. If the import fails midway, nodes created so far remain in `tree`, so
/// the caller should discard the tree and reload it.
pub fn import_directory(
    tree: &WorkspaceTree,
    parent: Option<NodeId>,
    dir: &Path,
    sink: &mut dyn ImportSink,
) -> Result<ImportReport> {
    let meta = fs::metadata(dir).map_err(|err| io_error(dir, &err))?;
    if !meta.is_dir() {
        return Err(Error::Io(format!("{} is not a directory", dir.display())));
    }
    let dir = fs::canonicalize(dir).map_err(|err| io_error(dir, &err))?;
    let root = normalize(&dir);
    let Some(scanned) = scan(&dir, "", 0)? else {
        return Ok(ImportReport::default());
    };
    let mut importer = Importer {
        tree,
        root,
        sink,
        report: ImportReport::default(),
        pending: Vec::new(),
        pages: HashMap::new(),
        pages_lower: HashMap::new(),
        stored: HashMap::new(),
    };
    importer.build(&scanned, parent)?;
    let pending = std::mem::take(&mut importer.pending);
    for item in &pending {
        importer.import_page(item)?;
    }
    Ok(importer.report)
}
