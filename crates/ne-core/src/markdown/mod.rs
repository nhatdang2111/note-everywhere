//! Markdown import and export (CommonMark with the GFM extensions for task
//! lists, strikethrough and tables).
//!
//! # What export writes
//!
//! | Block | Markdown |
//! |---|---|
//! | paragraph, heading 1-3 | text, `#`..`###` |
//! | bullet / numbered / to-do list | `-` / `1.` / `- [ ]` and `- [x]`, nested by indentation |
//! | quote | `>` |
//! | code | fenced, language as info string, fence longer than any backtick run inside |
//! | divider | `---` |
//! | image | `![alt](path)`; an attachment is written beside the page |
//! | child page, page link | `[Title](relative/path.md)` in a folder export |
//! | bold, italic, strike, code, link | `**`, `*`, `~~`, backticks, `[text](url)` |
//! | soft line break | a backslash at the end of the line |
//!
//! Empty paragraphs are dropped. Consecutive lists of the same kind use
//! different markers (`-` then `*`, `.` then `)`) so they stay separate lists
//! on import.
//!
//! # Callout and toggle
//!
//! Neither has a CommonMark form, so both are block quotes whose first line is
//! a marker in the style of GitHub alerts. The text stays readable in any
//! viewer and import recognises the marker, so the round trip is lossless:
//!
//! ```text
//! > [!callout 💡]
//! >
//! > Body blocks, any kind.
//!
//! > [!toggle open] Summary with **marks**
//! >
//! > Hidden blocks, any kind.
//! ```
//!
//! - `[!callout ICON]` carries the icon between the name and `]`; with no icon
//!   it is `[!callout]`. The body follows after a blank `>` line.
//! - `[!toggle]` is closed and `[!toggle open]` is open. The summary (the
//!   first paragraph of the toggle) continues on the marker line, so it keeps
//!   its marks; the collapsible body follows after a blank `>` line.
//!
//! Names are matched without regard to case. GitHub alerts (`> [!NOTE]`,
//! `[!TIP]`, ...) import as callouts with a matching icon. Every other `>`
//! block is a plain quote and never a toggle.
//!
//! # Import
//!
//! Anything without a block equivalent keeps its text: raw HTML and table rows
//! become paragraphs (cells joined by ` | `), front matter becomes a `yaml`
//! code block, headings below level 3 become level 3 headings, HTML comments
//! are dropped, and a soft line break becomes a space. A leading level-1
//! heading is the page title and is removed from the body, which is what
//! export writes.
//!
//! # Folders
//!
//! [`export_tree`] and [`import_directory`] map the workspace hierarchy to a
//! directory tree and back; see their documentation. File names come from
//! [`filenames`] and are valid on Windows.

mod export;
pub mod filenames;
mod folder;
mod import;
mod inline;
pub mod structured;

use crate::error::Result;
use crate::page::{Block, PageDoc};

pub use export::{ExportEnv, PageRef, StandaloneEnv, export_blocks};
pub use folder::{
    ASSETS_DIR, Attachment, ExportReport, ImportReport, ImportSink, PageSource, export_tree,
    import_directory,
};
pub use import::{ImportEnv, ImportedPage, PageTarget, StandaloneImport, import_blocks};

/// Replaces the content of `doc` with `blocks`. Nothing happens when `blocks`
/// is empty, so an empty import leaves the page's empty paragraph.
pub fn write_imported(doc: &PageDoc, blocks: &[Block]) -> Result<()> {
    if blocks.is_empty() {
        return Ok(());
    }
    let existing = doc.blocks()?.len();
    doc.append_blocks(&[], blocks)?;
    for index in (0..existing).rev() {
        doc.delete_block(&[index])?;
    }
    Ok(())
}
