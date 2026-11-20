//! Renders page blocks as CommonMark/GFM.

use super::inline::{Context, format_destination, render_inline};
use crate::page::{
    Block, BlockKind, Inline, attachment_src, normalize_inline, parse_attachment_src,
};

/// What a page link or attachment turns into in the exported text.
pub struct PageRef {
    /// Link text.
    pub title: String,
    /// Link destination, relative to the exported file.
    pub href: String,
}

/// Resolves the references that only make sense in a folder layout. Every
/// method may decline, in which case the page keeps its own form
/// (`ne-page:<id>` links, `attachment:<hash>` image sources).
pub trait ExportEnv {
    /// Where a `child_page` or `page_link` block pointing at `page_id` goes.
    fn page_link(&mut self, page_id: &str) -> Option<PageRef>;
    /// The relative path an attachment is written to, for an image source.
    fn attachment_path(&mut self, hash: &str) -> Option<String>;
}

/// An environment that resolves nothing: a single page exported on its own.
pub struct StandaloneEnv;

impl ExportEnv for StandaloneEnv {
    fn page_link(&mut self, _page_id: &str) -> Option<PageRef> {
        None
    }

    fn attachment_path(&mut self, _hash: &str) -> Option<String> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListStyle {
    Bullet(char),
    Ordered(char),
}

/// Renders blocks as Markdown text ending in a newline (empty for no content).
pub fn export_blocks(blocks: &[Block], env: &mut dyn ExportEnv) -> String {
    let lines = render_blocks(blocks, env);
    if lines.is_empty() {
        String::new()
    } else {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// Renders blocks as lines, one blank line between blocks. Blocks without
/// visible content (empty paragraphs) are dropped.
fn render_blocks(blocks: &[Block], env: &mut dyn ExportEnv) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut previous: Option<ListStyle> = None;
    for block in blocks {
        let (rendered, style) = render_block(block, previous, env);
        if rendered.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(rendered);
        previous = style;
    }
    lines
}

fn prefix_lines(lines: Vec<String>, first: &str, rest: &str) -> Vec<String> {
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let prefix = if i == 0 { first } else { rest };
            if line.is_empty() {
                prefix.trim_end().to_owned()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect()
}

/// A single line holding `text`, or the lines of multi-line text.
fn text_lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_owned).collect()
}

fn render_block(
    block: &Block,
    previous: Option<ListStyle>,
    env: &mut dyn ExportEnv,
) -> (Vec<String>, Option<ListStyle>) {
    let plain = |lines| (lines, None);
    match &block.kind {
        BlockKind::Paragraph => {
            if block.inline.is_empty() {
                return plain(Vec::new());
            }
            plain(text_lines(&render_inline(
                &block.inline,
                Context::Paragraph,
                true,
            )))
        }
        BlockKind::Heading { level } => {
            let text = render_inline(&block.inline, Context::Heading, false);
            if text.is_empty() {
                return plain(Vec::new());
            }
            plain(vec![format!("{} {text}", "#".repeat(usize::from(*level)))])
        }
        BlockKind::BulletList | BlockKind::TodoList => {
            let marker = match previous {
                Some(ListStyle::Bullet('-')) => '*',
                _ => '-',
            };
            let lines = block
                .children
                .iter()
                .flat_map(|item| render_item(item, &format!("{marker} "), env))
                .collect();
            (lines, Some(ListStyle::Bullet(marker)))
        }
        BlockKind::OrderedList => {
            let delimiter = match previous {
                Some(ListStyle::Ordered('.')) => ')',
                _ => '.',
            };
            let lines = block
                .children
                .iter()
                .enumerate()
                .flat_map(|(i, item)| render_item(item, &format!("{}{delimiter} ", i + 1), env))
                .collect();
            (lines, Some(ListStyle::Ordered(delimiter)))
        }
        // Items are rendered by their list; one found elsewhere is skipped.
        BlockKind::ListItem | BlockKind::TodoItem { .. } | BlockKind::Unknown { .. } => {
            plain(Vec::new())
        }
        BlockKind::Quote => {
            let lines = render_blocks(&block.children, env);
            plain(prefix_lines(
                if lines.is_empty() {
                    vec![String::new()]
                } else {
                    lines
                },
                "> ",
                "> ",
            ))
        }
        BlockKind::Callout { icon } => {
            let icon: String = icon
                .chars()
                .filter(|c| !c.is_control() && *c != ']')
                .collect();
            let icon = icon.trim();
            let marker = if icon.is_empty() {
                "[!callout]".to_owned()
            } else {
                format!("[!callout {icon}]")
            };
            let mut lines = vec![marker];
            let body = render_blocks(&block.children, env);
            if !body.is_empty() {
                lines.push(String::new());
                lines.extend(body);
            }
            plain(prefix_lines(lines, "> ", "> "))
        }
        BlockKind::Toggle { open } => {
            let marker = if *open { "[!toggle open]" } else { "[!toggle]" };
            let (title, rest) = match block.children.split_first() {
                Some((first, rest)) if first.kind == BlockKind::Paragraph => {
                    (&first.inline[..], rest)
                }
                _ => (&[][..], &block.children[..]),
            };
            let title = render_inline(title, Context::Paragraph, false);
            let head = if title.is_empty() {
                marker.to_owned()
            } else {
                format!("{marker} {title}")
            };
            let mut lines = text_lines(&head);
            let body = render_blocks(rest, env);
            if !body.is_empty() {
                lines.push(String::new());
                lines.extend(body);
            }
            plain(prefix_lines(lines, "> ", "> "))
        }
        BlockKind::Divider => plain(vec!["---".to_owned()]),
        BlockKind::CodeBlock { language } => plain(render_code(language, &block.plain_text())),
        BlockKind::Image { src, alt } => {
            let target = match parse_attachment_src(src) {
                Some(hash) => env
                    .attachment_path(hash)
                    .unwrap_or_else(|| attachment_src(hash)),
                None => src.clone(),
            };
            let alt = render_inline(
                &[Inline::text(alt.replace('\n', " "))],
                Context::Heading,
                false,
            );
            plain(vec![format!("![{alt}]({})", format_destination(&target))])
        }
        BlockKind::ChildPage { page_id, title } | BlockKind::PageLink { page_id, title } => {
            let (title, href) = match env.page_link(page_id) {
                Some(found) => (found.title, found.href),
                None => (
                    title.clone(),
                    format!("{}{page_id}", crate::page::PAGE_HREF_PREFIX),
                ),
            };
            let title = if title.trim().is_empty() {
                "Untitled".to_owned()
            } else {
                title
            };
            let text = render_inline(
                &normalize_inline(vec![Inline::text(title)]),
                Context::Heading,
                false,
            );
            plain(vec![format!("[{text}]({})", format_destination(&href))])
        }
    }
}

/// One list entry: its first paragraph on the marker line, nested blocks
/// below it indented to the text column.
fn render_item(item: &Block, marker: &str, env: &mut dyn ExportEnv) -> Vec<String> {
    let task = match item.kind {
        BlockKind::TodoItem { checked: true } => "[x] ",
        BlockKind::TodoItem { checked: false } => "[ ] ",
        _ => "",
    };
    let indent = match item.kind {
        BlockKind::TodoItem { .. } => "  ".to_owned(),
        _ => " ".repeat(marker.chars().count()),
    };
    let (head, rest) = match item.children.split_first() {
        Some((first, rest)) if first.kind == BlockKind::Paragraph => (
            text_lines(&render_inline(&first.inline, Context::Paragraph, true)),
            rest,
        ),
        _ => (vec![String::new()], &item.children[..]),
    };
    let mut lines = head;
    let body = render_blocks(rest, env);
    if !body.is_empty() {
        // A nested list sits directly under the item text; anything else is
        // set apart by a blank line.
        let nested_list = matches!(
            rest.iter()
                .find(|b| !b.plain_text().is_empty() || !b.kind.is_text_block()),
            Some(Block {
                kind: BlockKind::BulletList | BlockKind::OrderedList | BlockKind::TodoList,
                ..
            })
        );
        if !nested_list {
            lines.push(String::new());
        }
        lines.extend(body);
    }
    if let Some(first) = lines.first_mut() {
        if first.is_empty() {
            first.push_str(task.trim_end());
        } else {
            first.insert_str(0, task);
        }
    }
    prefix_lines(lines, marker, &indent)
}

/// A fenced code block whose fence is longer than any backtick run inside.
fn render_code(language: &str, text: &str) -> Vec<String> {
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat((longest + 1).max(3));
    let language: String = language
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| *c != '`')
        .collect();
    let mut lines = vec![format!("{fence}{language}")];
    if !text.is_empty() {
        lines.extend(text.split('\n').map(str::to_owned));
    }
    lines.push(fence);
    lines
}
