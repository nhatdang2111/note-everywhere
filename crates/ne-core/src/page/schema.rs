//! Content rules of the editor schema, enforced before anything is written so
//! the editor never meets a document it cannot build.

use super::model::{Block, BlockKind};
use crate::error::{Error, Result};

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::InvalidBlock(message.into()))
}

/// True for kinds that may sit directly in the page root or in a container
/// that accepts "any block". Unknown nodes already in the document are
/// tolerated so an old build can still edit a page a newer build touched.
fn is_free_block(kind: &BlockKind) -> bool {
    !kind.is_item()
}

/// Checks that `kinds`, in order, is a legal child sequence of `parent`.
/// `parent` is `None` for the page root.
pub(crate) fn check_children(parent: Option<&BlockKind>, kinds: &[&BlockKind]) -> Result<()> {
    let name = parent.map_or("page", BlockKind::node_name);
    let all = |pred: fn(&BlockKind) -> bool, what: &str| -> Result<()> {
        match kinds.iter().find(|k| !pred(k)) {
            Some(bad) => invalid(format!(
                "{} cannot hold {} (needs {what})",
                name,
                bad.node_name()
            )),
            None => Ok(()),
        }
    };
    match parent {
        None | Some(BlockKind::Quote | BlockKind::Callout { .. }) => {
            if kinds.is_empty() {
                return invalid(format!("{name} needs at least one block"));
            }
            all(is_free_block, "blocks")
        }
        Some(BlockKind::BulletList | BlockKind::OrderedList) => {
            if kinds.is_empty() {
                return invalid(format!("{name} needs at least one item"));
            }
            all(|k| matches!(k, BlockKind::ListItem), "list_item")
        }
        Some(BlockKind::TodoList) => {
            if kinds.is_empty() {
                return invalid("todo_list needs at least one item");
            }
            all(|k| matches!(k, BlockKind::TodoItem { .. }), "todo_item")
        }
        Some(BlockKind::ListItem | BlockKind::TodoItem { .. } | BlockKind::Toggle { .. }) => {
            if !matches!(kinds.first(), Some(BlockKind::Paragraph)) {
                return invalid(format!("{name} must start with a paragraph"));
            }
            all(is_free_block, "blocks")
        }
        Some(_) if kinds.is_empty() => Ok(()),
        Some(_) => invalid(format!("{name} cannot hold children")),
    }
}

/// Validates a block and everything below it.
pub(crate) fn validate_block(block: &Block) -> Result<()> {
    match &block.kind {
        BlockKind::Unknown { node_name } => {
            return invalid(format!("cannot write unknown node {node_name:?}"));
        }
        BlockKind::Heading { level } if !(1..=3).contains(level) => {
            return invalid(format!("heading level {level} is not 1, 2 or 3"));
        }
        _ => {}
    }
    if !block.kind.is_text_block() && !block.inline.is_empty() {
        return invalid(format!("{} has no inline content", block.kind.node_name()));
    }
    let kinds: Vec<&BlockKind> = block.children.iter().map(|c| &c.kind).collect();
    check_children(Some(&block.kind), &kinds)?;
    block.children.iter().try_for_each(validate_block)
}

/// Checks that `block` may be inserted at `index` among children whose kinds
/// are `existing`, below `parent`.
pub(crate) fn check_insert(
    parent: Option<&BlockKind>,
    existing: &[BlockKind],
    index: usize,
    block: &Block,
) -> Result<()> {
    validate_block(block)?;
    let mut kinds: Vec<&BlockKind> = existing.iter().collect();
    kinds.insert(index.min(kinds.len()), &block.kind);
    check_children(parent, &kinds)
}

/// Checks that removing the child at `index` leaves a legal sequence.
pub(crate) fn check_remove(
    parent: Option<&BlockKind>,
    existing: &[BlockKind],
    index: usize,
) -> Result<()> {
    let kinds: Vec<&BlockKind> = existing
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != index)
        .map(|(_, k)| k)
        .collect();
    check_children(parent, &kinds)
}
