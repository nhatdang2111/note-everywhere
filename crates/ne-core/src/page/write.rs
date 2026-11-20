//! Programmatic edits in the binding's container layout.
//!
//! Every edit validates against the page schema first, writes nodes with all
//! three keys (`nodeName`, `attributes` with every schema attribute, and
//! `children`) and commits. Text offsets count Unicode scalar values, which is
//! what the native text container uses; the editor and ProseMirror count UTF-16
//! units, so offsets must never be exchanged with them (send update bytes).

use std::collections::HashMap;

use loro::{
    ExpandType, LoroDoc, LoroList, LoroMap, LoroText, LoroValue, StyleConfig, StyleConfigMap,
};

use super::PageDoc;
use super::model::{Block, BlockKind, Inline, Marks, normalize_inline};
use super::read::{
    Child, HARD_BREAK, KEY_ATTRIBUTES, KEY_CHILDREN, KEY_NODE_NAME, attributes_map, child_at,
    children_list, kind_from_node, node_name,
};
use super::schema::{check_children, check_insert, check_remove, validate_block};
use crate::error::{Error, Result};

/// Mirrors the style configuration of the editor binding: marks grow when text
/// is typed right after them, except links (`inclusive: false` in the schema).
/// The configuration is not part of the document; it only decides whether text
/// inserted at a mark's edge joins the mark.
pub(crate) fn configure_text_styles(doc: &LoroDoc) {
    let mut styles = StyleConfigMap::new();
    for name in ["bold", "italic", "code", "strike"] {
        styles.insert(
            name.into(),
            StyleConfig {
                expand: ExpandType::After,
            },
        );
    }
    styles.insert(
        "link".into(),
        StyleConfig {
            expand: ExpandType::None,
        },
    );
    doc.config_text_style(styles);
}

fn attribute_pairs(kind: &BlockKind) -> Vec<(&'static str, LoroValue)> {
    match kind {
        BlockKind::Heading { level } => vec![("level", LoroValue::I64(i64::from(*level)))],
        BlockKind::TodoItem { checked } => vec![("checked", LoroValue::Bool(*checked))],
        BlockKind::Toggle { open } => vec![("open", LoroValue::Bool(*open))],
        BlockKind::Callout { icon } => vec![("icon", LoroValue::from(icon.as_str()))],
        BlockKind::CodeBlock { language } => {
            vec![("language", LoroValue::from(language.as_str()))]
        }
        BlockKind::Image { src, alt } => vec![
            ("src", LoroValue::from(src.as_str())),
            ("alt", LoroValue::from(alt.as_str())),
        ],
        BlockKind::ChildPage { page_id, title } | BlockKind::PageLink { page_id, title } => vec![
            ("pageId", LoroValue::from(page_id.as_str())),
            ("title", LoroValue::from(title.as_str())),
        ],
        BlockKind::SyncedBlock { sync_id } => vec![("syncId", LoroValue::from(sync_id.as_str()))],
        _ => Vec::new(),
    }
}

/// Creates a node with all three keys at `index` (or the end) of `list`.
/// Returns the node's `children` list.
fn create_node(
    list: &LoroList,
    index: Option<usize>,
    name: &str,
    attrs: &[(&'static str, LoroValue)],
) -> Result<LoroList> {
    let node = match index {
        Some(i) => list.insert_container(i, LoroMap::new())?,
        None => list.push_container(LoroMap::new())?,
    };
    node.insert(KEY_NODE_NAME, name)?;
    let attributes = node.insert_container(KEY_ATTRIBUTES, LoroMap::new())?;
    for (key, value) in attrs {
        attributes.insert(key, value.clone())?;
    }
    Ok(node.insert_container(KEY_CHILDREN, LoroList::new())?)
}

/// Writes a validated block and everything below it.
pub(crate) fn write_block(list: &LoroList, index: Option<usize>, block: &Block) -> Result<()> {
    let children = create_node(
        list,
        index,
        block.kind.node_name(),
        &attribute_pairs(&block.kind),
    )?;
    if block.kind.is_text_block() {
        let code = matches!(block.kind, BlockKind::CodeBlock { .. });
        write_inline(&children, code, &block.inline)
    } else {
        block
            .children
            .iter()
            .try_for_each(|child| write_block(&children, None, child))
    }
}

fn mark_value(entries: &[(&str, &str)]) -> LoroValue {
    let map: HashMap<String, LoroValue> = entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), LoroValue::from(*v)))
        .collect();
    LoroValue::from(map)
}

/// Writes inline content: each stretch of text between line breaks becomes one
/// text container with marks applied by character range.
pub(crate) fn write_inline(children: &LoroList, code: bool, inline: &[Inline]) -> Result<()> {
    let inline = if code {
        // Code blocks hold one unmarked text; a break is a newline.
        let text: String = inline
            .iter()
            .map(|item| match item {
                Inline::Text(run) => run.text.as_str(),
                Inline::Break => "\n",
            })
            .collect();
        normalize_inline(vec![Inline::text(text)])
    } else {
        normalize_inline(inline.to_vec())
    };
    let mut group = Vec::new();
    for item in &inline {
        match item {
            Inline::Text(run) => group.push(run),
            Inline::Break => {
                write_text_group(children, &group)?;
                group.clear();
                create_node(children, None, HARD_BREAK, &[])?;
            }
        }
    }
    write_text_group(children, &group)
}

fn write_text_group(children: &LoroList, group: &[&super::model::Run]) -> Result<()> {
    if group.is_empty() {
        return Ok(());
    }
    let whole: String = group.iter().map(|run| run.text.as_str()).collect();
    let text = children.push_container(LoroText::new())?;
    text.insert(0, &whole)?;
    let mut start = 0;
    for run in group {
        let end = start + run.text.chars().count();
        apply_marks(&text, start..end, &run.marks)?;
        start = end;
    }
    Ok(())
}

fn apply_marks(text: &LoroText, range: std::ops::Range<usize>, marks: &Marks) -> Result<()> {
    for (on, name) in [
        (marks.bold, "bold"),
        (marks.italic, "italic"),
        (marks.code, "code"),
        (marks.strike, "strike"),
    ] {
        if on {
            text.mark(range.clone(), name, mark_value(&[]))?;
        }
    }
    if let Some(href) = &marks.link {
        text.mark(range, "link", mark_value(&[("href", href)]))?;
    }
    Ok(())
}

impl PageDoc {
    /// Resolves a path of child indexes to a node map. The empty path is the
    /// page root.
    pub(crate) fn node_at(&self, path: &[usize]) -> Result<LoroMap> {
        let mut node = self.root.clone();
        for depth in 0..path.len() {
            let children = children_list(&node)?;
            node = match child_at(&children, path[depth]) {
                Ok(Child::Node(next)) => next,
                _ => return Err(Error::NoSuchBlock(path[..=depth].to_vec())),
            };
        }
        Ok(node)
    }

    fn kind_of(&self, node: &LoroMap) -> Result<BlockKind> {
        let name = node_name(node)?;
        kind_from_node(&name, || attributes_map(node))
    }

    /// Kind of the node at `path`; `None` for the page root.
    fn parent_kind(&self, path: &[usize]) -> Result<Option<BlockKind>> {
        if path.is_empty() {
            return Ok(None);
        }
        self.kind_of(&self.node_at(path)?).map(Some)
    }

    fn child_kinds(list: &LoroList) -> Result<Vec<BlockKind>> {
        let mut kinds = Vec::with_capacity(list.len());
        for index in 0..list.len() {
            if let Child::Node(node) = child_at(list, index)? {
                let name = node_name(&node)?;
                kinds.push(kind_from_node(&name, || attributes_map(&node))?);
            }
        }
        Ok(kinds)
    }

    /// Inserts `block` (with everything below it) as child `index` of the
    /// block at `parent` (the empty path is the page root). Fails, writing
    /// nothing, when the result would break the page schema.
    pub fn insert_block(&self, parent: &[usize], index: usize, block: &Block) -> Result<()> {
        let parent_node = self.node_at(parent)?;
        let list = children_list(&parent_node)?;
        let existing = Self::child_kinds(&list)?;
        if index > existing.len() {
            let mut path = parent.to_vec();
            path.push(index);
            return Err(Error::NoSuchBlock(path));
        }
        let parent_kind = self.parent_kind(parent)?;
        check_insert(parent_kind.as_ref(), &existing, index, block)?;
        let result = write_block(&list, Some(index), block);
        self.commit();
        result
    }

    /// Appends `block` as the last child of the block at `parent` and returns
    /// its index.
    pub fn append_block(&self, parent: &[usize], block: &Block) -> Result<usize> {
        self.append_blocks(parent, std::slice::from_ref(block))
    }

    /// Appends several blocks as the last children of the block at `parent`
    /// in one validated, committed step and returns the index of the first.
    /// Prefer it to repeated [`Self::append_block`] calls for bulk writes such
    /// as an import: validation reads the existing children once.
    pub fn append_blocks(&self, parent: &[usize], blocks: &[Block]) -> Result<usize> {
        let parent_node = self.node_at(parent)?;
        let list = children_list(&parent_node)?;
        let existing = Self::child_kinds(&list)?;
        let first = existing.len();
        blocks.iter().try_for_each(validate_block)?;
        let mut kinds: Vec<&BlockKind> = existing.iter().collect();
        kinds.extend(blocks.iter().map(|b| &b.kind));
        check_children(self.parent_kind(parent)?.as_ref(), &kinds)?;
        let result = blocks
            .iter()
            .try_for_each(|block| write_block(&list, None, block));
        self.commit();
        result.map(|()| first)
    }

    /// Deletes the block at `path` together with everything below it. Fails
    /// when the parent would be left without content its kind requires (the
    /// last block of a page, the paragraph that starts a list item, ...).
    pub fn delete_block(&self, path: &[usize]) -> Result<()> {
        let Some((&index, parent)) = path.split_last() else {
            return Err(Error::NoSuchBlock(Vec::new()));
        };
        self.node_at(path)?;
        let list = children_list(&self.node_at(parent)?)?;
        let existing = Self::child_kinds(&list)?;
        check_remove(self.parent_kind(parent)?.as_ref(), &existing, index)?;
        let result = list.delete(index, 1).map_err(Error::from);
        self.commit();
        result
    }

    /// Replaces the inline content of the text block at `path`.
    pub fn set_inline(&self, path: &[usize], inline: &[Inline]) -> Result<()> {
        let node = self.node_at(path)?;
        let kind = self.kind_of(&node)?;
        if !kind.is_text_block() {
            return Err(Error::InvalidBlock(format!(
                "{} has no inline content",
                kind.node_name()
            )));
        }
        let children = children_list(&node)?;
        let result = children
            .delete(0, children.len())
            .map_err(Error::from)
            .and_then(|()| {
                write_inline(
                    &children,
                    matches!(kind, BlockKind::CodeBlock { .. }),
                    inline,
                )
            });
        self.commit();
        result
    }

    /// Inserts `text` into the text block at `path`, `offset` Unicode scalar
    /// values from its start (a soft line break counts as one). Text inserted
    /// at the edge of a bold, italic, code or strike run joins that run, like
    /// typing; text next to a link does not.
    pub fn insert_text(&self, path: &[usize], offset: usize, text: &str) -> Result<()> {
        let node = self.node_at(path)?;
        let kind = self.kind_of(&node)?;
        if !kind.is_text_block() {
            return Err(Error::InvalidBlock(format!(
                "{} has no inline content",
                kind.node_name()
            )));
        }
        if text.is_empty() {
            return Ok(());
        }
        let children = children_list(&node)?;
        let result = insert_text_at(&children, offset, text);
        self.commit();
        result
    }

    /// Replaces the attributes of the block at `path` with those of `kind`,
    /// which must be the same node kind (a heading can change level, a to-do
    /// item can be ticked). Every schema attribute is rewritten.
    pub fn set_attrs(&self, path: &[usize], kind: &BlockKind) -> Result<()> {
        let node = self.node_at(path)?;
        let current = node_name(&node)?;
        if current != kind.node_name() || matches!(kind, BlockKind::Unknown { .. }) {
            return Err(Error::InvalidBlock(format!(
                "cannot turn {current} into {}",
                kind.node_name()
            )));
        }
        if let BlockKind::Heading { level } = kind
            && !(1..=3).contains(level)
        {
            return Err(Error::InvalidBlock(format!(
                "heading level {level} is not 1, 2 or 3"
            )));
        }
        let attributes = attributes_map(&node)?;
        let result = attribute_pairs(kind)
            .into_iter()
            .try_for_each(|(key, value)| attributes.insert(key, value).map_err(Error::from));
        self.commit();
        result
    }

    /// Ticks or unticks the to-do item at `path`.
    pub fn set_todo_checked(&self, path: &[usize], checked: bool) -> Result<()> {
        self.set_attrs(path, &BlockKind::TodoItem { checked })
    }
}

fn insert_text_at(children: &LoroList, offset: usize, text: &str) -> Result<()> {
    let mut remaining = offset;
    for index in 0..children.len() {
        match child_at(children, index)? {
            Child::Text(container) => {
                let len = container.len_unicode();
                if remaining <= len {
                    return Ok(container.insert(remaining, text)?);
                }
                remaining -= len;
            }
            Child::Node(_) => {
                if remaining == 0 {
                    let fresh = children.insert_container(index, LoroText::new())?;
                    return Ok(fresh.insert(0, text)?);
                }
                remaining -= 1;
            }
        }
    }
    if remaining == 0 {
        let fresh = children.push_container(LoroText::new())?;
        return Ok(fresh.insert(0, text)?);
    }
    Err(Error::InvalidBlock(format!(
        "text offset {offset} is past the end of the block"
    )))
}
