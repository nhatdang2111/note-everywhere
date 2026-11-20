//! Reading the directory tree of an export into a plan: pages, databases and
//! plain folders, before anything is created.

use std::fs;
use std::path::{Path, PathBuf};

use super::infer::trailing_hex_id;
use crate::error::{Error, Result};
use crate::markdown::folder::{MAX_IMPORT_DEPTH, percent_decode};

/// A file found in the export.
#[derive(Debug, Clone)]
pub(super) struct FileEntry {
    pub stem: String,
    pub ext: String,
    pub path: PathBuf,
}

/// A directory found in the export.
#[derive(Debug, Clone)]
pub(super) struct Dir {
    pub name: String,
    pub files: Vec<FileEntry>,
    pub dirs: Vec<Dir>,
}

fn io_error(path: &Path, err: &std::io::Error) -> Error {
    Error::Io(format!("{}: {err}", path.display()))
}

/// Lists a directory, skipping hidden entries and symlinks, sorted by name so
/// the result does not depend on the file system.
pub(super) fn scan(path: &Path, name: &str, depth: usize) -> Result<Dir> {
    if depth > MAX_IMPORT_DEPTH {
        return Err(Error::Io(format!(
            "directories are nested deeper than {MAX_IMPORT_DEPTH} levels"
        )));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| io_error(path, &e))? {
        entries.push(entry.map_err(|e| io_error(path, &e))?);
    }
    entries.sort_by_key(|e| e.file_name());
    let mut dir = Dir {
        name: name.to_owned(),
        files: Vec::new(),
        dirs: Vec::new(),
    };
    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type().map_err(|e| io_error(path, &e))?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            dir.dirs.push(scan(&entry.path(), &file_name, depth + 1)?);
        } else if kind.is_file() {
            let p = Path::new(&file_name);
            dir.files.push(FileEntry {
                stem: p
                    .file_stem()
                    .map_or_else(|| file_name.clone(), |s| s.to_string_lossy().into_owned()),
                ext: p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
                path: entry.path(),
            });
        }
    }
    Ok(dir)
}

/// What an exported name stands for: the title and the id at its end.
pub(super) fn split_name(stem: &str) -> (String, Option<String>) {
    match trailing_hex_id(stem) {
        Some(id) => {
            let title = stem.trim_end()[..stem.trim_end().len() - 32]
                .trim_end()
                .to_owned();
            (title, Some(id))
        }
        None => (stem.trim().to_owned(), None),
    }
}

/// A page title from a file stem, never empty.
pub(super) fn title_of(stem: &str) -> String {
    let (title, _) = split_name(stem);
    if title.is_empty() {
        "Untitled".to_owned()
    } else {
        title
    }
}

pub(super) fn is_markdown(file: &FileEntry) -> bool {
    file.ext == "md" || file.ext == "markdown"
}

/// A row title as the CSV and the file name must agree on it: trimmed, case
/// and accents ignored.
pub(super) fn match_key(title: &str) -> String {
    crate::database::fold_name(title.trim())
}

/// A relative link target as a path below `base`, percent-decoded, with
/// fragments and queries dropped and `.`/`..` resolved.
pub(super) fn resolve(base: &Path, href: &str) -> PathBuf {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    crate::markdown::folder::normalize(&base.join(percent_decode(href)))
}

/// A planned directory: what to create under one parent.
#[derive(Debug)]
pub(super) enum Item {
    Folder { name: String, children: Vec<Item> },
    Page(PagePlan),
    Database(DbPlan),
}

#[derive(Debug)]
pub(super) struct PagePlan {
    pub title: String,
    pub hex: Option<String>,
    pub md: PathBuf,
    pub children: Vec<Item>,
}

#[derive(Debug)]
pub(super) struct DbPlan {
    pub title: String,
    pub hex: Option<String>,
    pub csv: PathBuf,
    /// The directory of row pages, when the export has one.
    pub rows: Option<Dir>,
}

/// Plans the contents of `dir`. `skipped` collects what is passed over and why.
pub(super) fn plan(dir: &Dir, skipped: &mut Vec<String>) -> Vec<Item> {
    let mut consumed = vec![false; dir.dirs.len()];
    let mut items = Vec::new();
    let find_dir = |name: &str, consumed: &mut [bool]| -> Option<usize> {
        let at = dir
            .dirs
            .iter()
            .enumerate()
            .position(|(i, d)| !consumed[i] && d.name == name)?;
        consumed[at] = true;
        Some(at)
    };
    // `X.csv` and `X_all.csv` hold the same database; the `_all` file has
    // every row, so it is the one imported.
    let csvs: Vec<&FileEntry> = dir.files.iter().filter(|f| f.ext == "csv").collect();
    for file in &csvs {
        let base = file.stem.strip_suffix("_all").unwrap_or(&file.stem);
        let is_all = base.len() != file.stem.len();
        let has_all = csvs.iter().any(|f| f.stem == format!("{}_all", file.stem));
        if has_all && !is_all {
            skipped.push(format!(
                "{}: the matching _all file is imported instead",
                file.path.display()
            ));
            continue;
        }
        let (title, hex) = split_name(base);
        let rows = [base.to_owned(), title.clone()]
            .iter()
            .find_map(|name| find_dir(name, &mut consumed))
            .map(|i| dir.dirs[i].clone());
        items.push(Item::Database(DbPlan {
            title: if title.is_empty() {
                "Untitled".into()
            } else {
                title
            },
            hex,
            csv: file.path.clone(),
            rows,
        }));
    }
    for file in dir.files.iter().filter(|f| is_markdown(f)) {
        let children = find_dir(&file.stem, &mut consumed)
            .map(|i| plan(&dir.dirs[i], skipped))
            .unwrap_or_default();
        let (_, hex) = split_name(&file.stem);
        items.push(Item::Page(PagePlan {
            title: title_of(&file.stem),
            hex,
            md: file.path.clone(),
            children,
        }));
    }
    for (i, sub) in dir.dirs.iter().enumerate() {
        if !consumed[i] {
            let children = plan(sub, skipped);
            if !children.is_empty() {
                items.push(Item::Folder {
                    name: title_of(&sub.name),
                    children,
                });
            }
        }
    }
    items
}
