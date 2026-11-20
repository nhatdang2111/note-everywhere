//! Data derived from page content for indexing: plain text, outgoing page
//! links, referenced attachments and word count.

use std::collections::BTreeSet;

use super::model::{Block, BlockKind, Inline, parse_attachment_src};
use crate::ids::NodeId;

/// How a page refers to another page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    /// A `child_page` block: the target is a child of this page.
    ChildPage,
    /// A `page_link` block.
    PageLink,
    /// A link mark on text.
    Inline,
}

/// One reference from a page to another page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutgoingLink {
    /// The page referred to.
    pub target: NodeId,
    /// How it is referred to.
    pub kind: LinkKind,
}

/// Everything the store indexes about one page, computed in one pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageSummary {
    /// One entry per top-level block (empty for blocks without text), in
    /// page order.
    pub block_texts: Vec<String>,
    /// Plain text of the whole page: the non-empty block texts, one per line.
    pub text: String,
    /// Distinct outgoing page links, sorted.
    pub links: Vec<OutgoingLink>,
    /// Content hashes of the attachments the page shows, sorted.
    pub attachments: Vec<String>,
    /// Number of words in the body (the page title is not part of the body).
    pub word_count: usize,
}

/// Text of a block as search sees it. A text block yields its inline text with
/// a soft line break as `\n`; a container yields its children's texts, one per
/// line; blocks without text yield an empty string. Titles copied into
/// `child_page` and `page_link` blocks are display copies and are not text.
pub fn block_plain_text(block: &Block) -> String {
    if block.kind.is_text_block() {
        return block
            .inline
            .iter()
            .map(|item| match item {
                Inline::Text(run) => run.text.as_str(),
                Inline::Break => "\n",
            })
            .collect();
    }
    block
        .children
        .iter()
        .map(block_plain_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Joins block texts into the text of a page: the non-empty ones, one per line.
fn join_block_texts(texts: &[String]) -> String {
    texts
        .iter()
        .filter(|text| !text.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain text of a list of blocks, see [`PageSummary::text`].
pub fn page_plain_text(blocks: &[Block]) -> String {
    let texts: Vec<String> = blocks.iter().map(block_plain_text).collect();
    join_block_texts(&texts)
}

/// Counts whitespace separated words that contain at least one letter or
/// digit. Vietnamese syllables count as words, as in other word counters.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count()
}

/// Distinct outgoing page links of the blocks, sorted. References whose id
/// does not parse as a node id (a page that never existed) are skipped.
pub fn outgoing_links(blocks: &[Block]) -> Vec<OutgoingLink> {
    let mut found = BTreeSet::new();
    collect_links(blocks, &mut found);
    found.into_iter().collect()
}

fn collect_links(blocks: &[Block], found: &mut BTreeSet<OutgoingLink>) {
    for block in blocks {
        match &block.kind {
            BlockKind::ChildPage { page_id, .. } => {
                add_block_link(page_id, LinkKind::ChildPage, found)
            }
            BlockKind::PageLink { page_id, .. } => {
                add_block_link(page_id, LinkKind::PageLink, found)
            }
            _ => {}
        }
        for item in &block.inline {
            if let Inline::Text(run) = item
                && let Some(target) = run.page_link()
            {
                found.insert(OutgoingLink {
                    target,
                    kind: LinkKind::Inline,
                });
            }
        }
        collect_links(&block.children, found);
    }
}

fn add_block_link(page_id: &str, kind: LinkKind, found: &mut BTreeSet<OutgoingLink>) {
    if let Ok(target) = page_id.parse::<NodeId>() {
        found.insert(OutgoingLink { target, kind });
    }
}

/// Distinct content hashes of attachments shown by image blocks, sorted.
pub fn attachment_hashes(blocks: &[Block]) -> Vec<String> {
    let mut found = BTreeSet::new();
    collect_attachments(blocks, &mut found);
    found.into_iter().collect()
}

fn collect_attachments(blocks: &[Block], found: &mut BTreeSet<String>) {
    for block in blocks {
        if let BlockKind::Image { src, .. } = &block.kind
            && let Some(hash) = parse_attachment_src(src)
        {
            found.insert(hash.to_owned());
        }
        collect_attachments(&block.children, found);
    }
}

/// Computes the full [`PageSummary`] of a list of blocks.
pub fn summarize(blocks: &[Block]) -> PageSummary {
    let block_texts: Vec<String> = blocks.iter().map(block_plain_text).collect();
    let text = join_block_texts(&block_texts);
    PageSummary {
        word_count: word_count(&text),
        links: outgoing_links(blocks),
        attachments: attachment_hashes(blocks),
        block_texts,
        text,
    }
}
