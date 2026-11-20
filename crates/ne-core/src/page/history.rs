//! Page history: versions from the CRDT history, read-only previews and
//! restoring as a new change.
//!
//! # Versions
//!
//! A CRDT document keeps every change with its author, time and what it
//! depended on. [`PageDoc::history`] groups consecutive changes by one author
//! into a *version* (a session): a new version starts when the author changes
//! or when more than [`HISTORY_SESSION_GAP_SECS`] pass between two changes. A
//! version is identified by the state of the document right after its last
//! change, counting only what that change causally depended on; a concurrent
//! edit by another device that the author had not seen is not part of it.
//!
//! The author is a replica id, not a person or a device name. The editor
//! creates a fresh replica id each time it opens a page, so one sitting at the
//! editor is one author and reopening the page starts a new one. Change times
//! come from the device that made the change and are only as good as its
//! clock; a change without a time (an editor that does not record times) sits
//! in a version whose times are unknown.
//!
//! # Preview and restore
//!
//! [`PageDoc::blocks_at`] rebuilds the document as it was at a version and
//! returns it as blocks; nothing is changed. [`PageDoc::restore_to`] writes
//! those blocks back as a *new forward change*: the history keeps growing, the
//! restored state is an ordinary edit that sync delivers to every device, and
//! the version being left stays in the history so the restore can itself be
//! undone. A restore replaces the page content wholesale, so an edit another
//! device makes to a block while the restore travels to it lands on a block
//! that no longer exists and is lost.
//!
//! # How far back history goes
//!
//! Storage compaction folds the update log into a snapshot **with** history,
//! so by default history reaches back to the creation of the page. A page's
//! history is trimmed only by [`PageDoc::export_shallow_snapshot_at`], which
//! drops everything before a chosen version. [`HistoryReport::trimmed`] says
//! whether that has happened to this copy of the page and
//! [`HistoryReport::oldest_at`] the time of the oldest version still there;
//! versions before it cannot be previewed or restored. Trimming is local to a
//! device: other devices keep their longer history until they trim too.

use loro::{Frontiers, ID};
use serde::Serialize;
use std::ops::ControlFlow;

use super::PageDoc;
use super::model::{Block, BlockKind};
use crate::error::{Error, Result};

/// Two changes by one author more than this many seconds apart belong to
/// different versions.
pub const HISTORY_SESSION_GAP_SECS: i64 = 300;

/// One version of a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// Opaque id to pass to preview and restore.
    pub id: String,
    /// The author's replica id as text.
    pub author: String,
    /// Time of the first change, in ms since the Unix epoch; `None` when the
    /// change carries no time.
    pub started_at: Option<i64>,
    /// Time of the last change.
    pub ended_at: Option<i64>,
    /// Number of changes grouped into the version.
    pub changes: usize,
    /// Number of operations in those changes, a rough measure of how much was
    /// edited.
    pub operations: usize,
}

/// The versions of a page and how far back they go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryReport {
    /// Newest first.
    pub versions: Vec<VersionInfo>,
    /// True when this copy of the page had its older history trimmed away.
    pub trimmed: bool,
    /// False when versions cannot be previewed or restored on this copy,
    /// which is the case for a trimmed one.
    pub previewable: bool,
    /// Time of the oldest version still available, when known.
    pub oldest_at: Option<i64>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn version_id(last: ID) -> String {
    hex(&Frontiers::from(last).encode())
}

fn frontiers(id: &str) -> Result<Frontiers> {
    let bytes = unhex(id).ok_or_else(|| Error::Missing(format!("version {id}")))?;
    let decoded = Frontiers::decode(&bytes).map_err(|_| Error::Missing(format!("version {id}")))?;
    // The empty frontiers name the state before any change, not a version.
    if decoded.is_empty() {
        return Err(Error::Missing(format!("version {id}")));
    }
    Ok(decoded)
}

struct Session {
    author: u64,
    first: i64,
    last: i64,
    end: ID,
    changes: usize,
    operations: usize,
}

impl PageDoc {
    /// The versions of the page, newest first. See the
    /// [module documentation](self) for how changes are grouped and what the
    /// list can and cannot tell.
    pub fn history(&self) -> Result<HistoryReport> {
        self.doc.commit();
        let tips: Vec<ID> = self.doc.oplog_frontiers().iter().collect();
        let mut changes = Vec::new();
        self.doc
            .travel_change_ancestors(&tips, &mut |meta| {
                changes.push(meta);
                ControlFlow::Continue(())
            })
            .map_err(|e| Error::Crdt(e.to_string()))?;
        // Travel visits the latest first; sessions are built oldest first.
        changes.reverse();
        let mut sessions: Vec<Session> = Vec::new();
        for meta in changes {
            let len = i32::try_from(meta.len).unwrap_or(i32::MAX);
            let end = ID::new(meta.id.peer, meta.id.counter + len - 1);
            let at = meta.timestamp;
            if let Some(open) = sessions.last_mut()
                && open.author == meta.id.peer
                && (open.last == 0) == (at == 0)
                && at - open.last <= HISTORY_SESSION_GAP_SECS
            {
                open.last = at;
                open.end = end;
                open.changes += 1;
                open.operations += meta.len;
                continue;
            }
            sessions.push(Session {
                author: meta.id.peer,
                first: at,
                last: at,
                end,
                changes: 1,
                operations: meta.len,
            });
        }
        let ms = |secs: i64| (secs != 0).then(|| secs.saturating_mul(1000));
        let versions: Vec<VersionInfo> = sessions
            .iter()
            .rev()
            .map(|s| VersionInfo {
                id: version_id(s.end),
                author: s.author.to_string(),
                started_at: ms(s.first),
                ended_at: ms(s.last),
                changes: s.changes,
                operations: s.operations,
            })
            .collect();
        let oldest_at = sessions.iter().find_map(|s| ms(s.first));
        Ok(HistoryReport {
            versions,
            trimmed: self.doc.is_shallow(),
            previewable: !self.doc.is_shallow(),
            oldest_at,
        })
    }

    /// The page as it was at `version` (an id from [`Self::history`]), as
    /// blocks. Nothing in the page changes.
    pub fn blocks_at(&self, version: &str) -> Result<Vec<Block>> {
        let at = frontiers(version)?;
        if self.doc.is_shallow() {
            return Err(Error::Invalid(
                "this copy of the page has trimmed history and cannot show past versions".into(),
            ));
        }
        self.doc.commit();
        let past = self
            .doc
            .fork_at(&at)
            .map_err(|e| Error::Missing(format!("version {version}: {e}")))?;
        let root = past.get_map(super::ROOT_NAME);
        super::read::read_root(&root)
    }

    /// Replaces the page content with `blocks` as one change. A page keeps at
    /// least one block: an empty list leaves a single empty paragraph.
    pub fn replace_blocks(&self, blocks: &[Block]) -> Result<()> {
        if let Some(bad) = blocks
            .iter()
            .find(|b| matches!(b.kind, BlockKind::Unknown { .. }))
        {
            return Err(Error::InvalidBlock(format!(
                "cannot write unknown node {:?}",
                bad.kind.node_name()
            )));
        }
        let fallback = [Block::new(BlockKind::Paragraph)];
        let blocks = if blocks.is_empty() {
            &fallback[..]
        } else {
            blocks
        };
        let existing = self.blocks()?.len();
        self.append_blocks(&[], blocks)?;
        for index in (0..existing).rev() {
            self.delete_block(&[index])?;
        }
        Ok(())
    }

    /// Restores the page to `version` as a new change; see the
    /// [module documentation](self).
    pub fn restore_to(&self, version: &str) -> Result<()> {
        let blocks = self.blocks_at(version)?;
        self.replace_blocks(&blocks)
    }

    /// Exports the current state with the history before `version` dropped.
    /// A replica that loads it can preview and restore `version` and later
    /// ones only.
    pub fn export_shallow_snapshot_at(&self, version: &str) -> Result<Vec<u8>> {
        let at = frontiers(version)?;
        self.doc.commit();
        Ok(self.doc.export(loro::ExportMode::shallow_snapshot(&at))?)
    }
}
