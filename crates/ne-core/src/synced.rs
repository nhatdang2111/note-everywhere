//! Synced blocks: content that appears on several pages and is stored once.
//!
//! # Design
//!
//! The content of a synced block lives in its own document, laid out exactly
//! like a page (a `doc` root of blocks, so the editor can bind a nested editor
//! to it) plus the role marker `synced` (see [`crate::roles`]). Each page that
//! shows it holds a *placeholder*: a `synced_block` node whose `syncId`
//! attribute is the id of that document. Editing the content in any page is an
//! ordinary edit of the one shared document, so it reaches every occurrence on
//! every device by normal sync, with the same merge rules as a page.
//! `docs/decisions/0008-synced-blocks.md` records the choice and what the UI
//! has to do.
//!
//! This module holds the page-side operations; the store keeps the shared
//! documents, the list of occurrences and the clean-up of orphans.

use std::collections::BTreeSet;

use crate::error::{Error, Result};
use crate::ids::NodeId;
use crate::page::{Block, BlockKind, PageDoc};
use crate::roles::Role;

/// The sync ids of every placeholder in `blocks`, nested ones included,
/// sorted and without repeats.
pub fn sync_ids(blocks: &[Block]) -> Vec<String> {
    fn walk(blocks: &[Block], out: &mut BTreeSet<String>) {
        for block in blocks {
            if let BlockKind::SyncedBlock { sync_id } = &block.kind {
                out.insert(sync_id.clone());
            }
            walk(&block.children, out);
        }
    }
    let mut out = BTreeSet::new();
    walk(blocks, &mut out);
    out.into_iter().collect()
}

/// Checks that `id` is a well-formed sync id (it has the shape of a document
/// id and is not a tree node's own id format by accident of the caller).
pub fn parse_sync_id(id: &str) -> Result<NodeId> {
    id.parse()
        .map_err(|_| Error::Invalid(format!("{id:?} is not a synced block id")))
}

/// A placeholder block for the shared document `sync_id`.
pub fn placeholder(sync_id: &str) -> Block {
    Block::new(BlockKind::SyncedBlock {
        sync_id: sync_id.to_owned(),
    })
}

/// A new shared document holding `blocks`. Blocks that are themselves
/// placeholders are rejected: shared content never contains shared content.
pub fn new_shared_doc(blocks: &[Block], peer_id: u64) -> Result<PageDoc> {
    if !sync_ids(blocks).is_empty() {
        return Err(Error::Invalid(
            "a synced block cannot contain another synced block".into(),
        ));
    }
    let doc = PageDoc::with_peer_id(peer_id)?;
    doc.replace_blocks(blocks)?;
    doc.tag_role(Role::Synced.as_str())?;
    Ok(doc)
}

/// The top-level blocks `start..end` of `page`, for turning them into a synced
/// block. Rejects an empty or out-of-range selection, and a selection that
/// already holds a placeholder.
pub fn selection(page: &PageDoc, start: usize, end: usize) -> Result<Vec<Block>> {
    let blocks = page.blocks()?;
    if start >= end || end > blocks.len() {
        return Err(Error::Invalid(format!(
            "blocks {start}..{end} are not a selection of this page"
        )));
    }
    let chosen = blocks[start..end].to_vec();
    if !sync_ids(&chosen).is_empty() {
        return Err(Error::Invalid(
            "the selection already holds a synced block".into(),
        ));
    }
    Ok(chosen)
}

/// Replaces the top-level blocks `start..end` of `page` with one placeholder
/// for `sync_id`. The placeholder goes in first, so the page never lacks a
/// block.
pub fn replace_with_placeholder(
    page: &PageDoc,
    start: usize,
    end: usize,
    sync_id: &str,
) -> Result<()> {
    page.insert_block(&[], start, &placeholder(sync_id))?;
    for index in (start + 1..=end).rev() {
        page.delete_block(&[index])?;
    }
    Ok(())
}

/// Inserts a placeholder for `sync_id` as child `index` of the block at
/// `parent` (the empty path is the page).
pub fn insert_placeholder(
    page: &PageDoc,
    parent: &[usize],
    index: usize,
    sync_id: &str,
) -> Result<()> {
    page.insert_block(parent, index, &placeholder(sync_id))
}

/// The sync id of the placeholder at `path`.
pub fn placeholder_at(page: &PageDoc, path: &[usize]) -> Result<String> {
    match page.block(path)?.kind {
        BlockKind::SyncedBlock { sync_id } => Ok(sync_id),
        _ => Err(Error::Invalid("the block is not a synced block".into())),
    }
}

/// Replaces the placeholder at `path` with a copy of the shared `content`,
/// which is what unsyncing one occurrence does. Returns the sync id that was
/// there.
pub fn unsync_at(page: &PageDoc, path: &[usize], content: &[Block]) -> Result<String> {
    let sync_id = placeholder_at(page, path)?;
    let Some((&index, parent)) = path.split_last() else {
        return Err(Error::NoSuchBlock(Vec::new()));
    };
    for (offset, copy) in content.iter().enumerate() {
        page.insert_block(parent, index + offset, copy)?;
    }
    page.delete_block(&[parent, &[index + content.len()]].concat())?;
    Ok(sync_id)
}
