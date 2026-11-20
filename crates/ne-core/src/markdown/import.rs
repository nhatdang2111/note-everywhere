//! Parses CommonMark/GFM into page blocks.

use std::collections::HashMap;

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser, Tag,
    TagEnd,
};

use crate::ids::NodeId;
use crate::page::{
    Block, BlockKind, Inline, Marks, PAGE_HREF_PREFIX, Run, normalize_inline, page_href,
    parse_page_href,
};

/// A page that a link in the imported text resolves to.
#[derive(Debug, Clone)]
pub struct PageTarget {
    /// The page's node id.
    pub id: NodeId,
    /// The page's title.
    pub title: String,
    /// True when the target is a child of the page being imported, which makes
    /// a paragraph holding just that link a `child_page` block.
    pub is_child: bool,
}

/// Resolves the references of imported text against what the import knows.
pub trait ImportEnv {
    /// The image source to store for the Markdown image destination `src`.
    /// Returning `src` unchanged keeps the image as written.
    fn image(&mut self, src: &str) -> String;
    /// The page a link destination points at, if any.
    fn page(&mut self, href: &str) -> Option<PageTarget>;
}

/// An environment that resolves nothing: text imported on its own.
pub struct StandaloneImport;

impl ImportEnv for StandaloneImport {
    fn image(&mut self, src: &str) -> String {
        src.to_owned()
    }

    fn page(&mut self, _href: &str) -> Option<PageTarget> {
        None
    }
}

/// The result of parsing one Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPage {
    /// Text of a leading level-1 heading, which is taken out of `blocks`
    /// because export writes the page title as that heading.
    pub title: Option<String>,
    /// The body. May be empty; a page needs at least one block, so callers
    /// writing it keep the skeleton paragraph in that case.
    pub blocks: Vec<Block>,
}

enum Kind {
    Root,
    Quote(Option<BlockQuoteKind>),
    List(bool),
    Item(Option<bool>),
    Paragraph,
    Heading(u8),
    Code(String),
    Html,
    Table,
    Row,
    Image(String),
    Footnote(String),
}

struct Frame {
    kind: Kind,
    blocks: Vec<Block>,
    inline: Vec<Inline>,
    cells: usize,
    /// For a quote: whether its first paragraph begins with a literal `[!` in
    /// the source. An escaped `\[!` parses to the same text but is not a marker.
    marker_in_source: bool,
}

impl Frame {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            blocks: Vec::new(),
            inline: Vec::new(),
            cells: 0,
            marker_in_source: false,
        }
    }

    /// Frames whose direct text belongs to an implicit paragraph.
    fn is_container(&self) -> bool {
        matches!(
            self.kind,
            Kind::Root | Kind::Quote(_) | Kind::Item(_) | Kind::Footnote(_)
        )
    }
}

struct Builder<'e> {
    env: &'e mut dyn ImportEnv,
    stack: Vec<Frame>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
    resolved: HashMap<String, PageTarget>,
}

fn paragraph_of(inline: Vec<Inline>) -> Block {
    Block::new(BlockKind::Paragraph).with_inline(inline)
}

fn plain_of(inline: &[Inline]) -> String {
    inline
        .iter()
        .map(|item| match item {
            Inline::Text(run) => run.text.as_str(),
            Inline::Break => "\n",
        })
        .collect()
}

fn alert_icon(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "\u{2139}\u{fe0f}",
        BlockQuoteKind::Tip => "\u{1f4a1}",
        BlockQuoteKind::Important => "\u{2757}",
        BlockQuoteKind::Warning => "\u{26a0}\u{fe0f}",
        BlockQuoteKind::Caution => "\u{1f6d1}",
    }
}

enum Marker {
    Callout { icon: String },
    Toggle { open: bool },
}

/// Recognises the `[!callout ICON]` / `[!toggle open]` marker at the start of
/// a quote's first paragraph. Returns the marker and the inline content that
/// follows it.
fn parse_marker(inline: &[Inline]) -> Option<(Marker, Vec<Inline>)> {
    let Some(Inline::Text(first)) = inline.first() else {
        return None;
    };
    if !first.marks.is_plain() {
        return None;
    }
    let after_bang = first.text.strip_prefix("[!")?;
    let close = after_bang.find(']')?;
    let inner = &after_bang[..close];
    let name = inner.split_whitespace().next()?;
    let arg = inner[inner.find(name)? + name.len()..].trim();
    let marker = match name.to_lowercase().as_str() {
        "callout" => Marker::Callout {
            icon: arg.to_owned(),
        },
        "toggle" => Marker::Toggle {
            open: arg.eq_ignore_ascii_case("open"),
        },
        _ => return None,
    };
    let rest = after_bang[close + 1..].trim_start();
    let mut tail = Vec::new();
    if !rest.is_empty() {
        tail.push(Inline::text(rest));
    }
    tail.extend_from_slice(&inline[1..]);
    Some((marker, tail))
}

fn with_content(mut blocks: Vec<Block>) -> Vec<Block> {
    if blocks.is_empty() {
        blocks.push(Block::new(BlockKind::Paragraph));
    }
    blocks
}

impl<'e> Builder<'e> {
    fn new(env: &'e mut dyn ImportEnv) -> Self {
        Self {
            env,
            stack: vec![Frame::new(Kind::Root)],
            bold: 0,
            italic: 0,
            strike: 0,
            links: Vec::new(),
            resolved: HashMap::new(),
        }
    }

    fn top(&mut self) -> &mut Frame {
        self.stack
            .last_mut()
            .expect("the root frame is never popped")
    }

    fn pop(&mut self) -> Frame {
        self.stack.pop().expect("events are balanced")
    }

    fn marks(&self) -> Marks {
        Marks {
            bold: self.bold > 0,
            italic: self.italic > 0,
            code: false,
            strike: self.strike > 0,
            link: self.links.last().cloned(),
        }
    }

    fn push_text(&mut self, text: &str, code: bool) {
        let mut marks = self.marks();
        marks.code = code;
        self.top().inline.push(Inline::Text(Run {
            text: text.to_owned(),
            marks,
        }));
    }

    /// A paragraph from collected inline content. A paragraph that is only a
    /// link to a known page becomes a page block when `allow_page_block`.
    fn paragraph(&self, inline: Vec<Inline>, allow_page_block: bool) -> Block {
        let inline = normalize_inline(inline);
        if allow_page_block && let [Inline::Text(run)] = &inline[..] {
            let link_only = Marks {
                link: run.marks.link.clone(),
                ..Marks::default()
            } == run.marks;
            if let (true, Some(id)) = (link_only, run.page_link()) {
                let target = run
                    .marks
                    .link
                    .as_ref()
                    .and_then(|href| self.resolved.get(href));
                let (title, is_child) = match target {
                    Some(t) => (t.title.clone(), t.is_child),
                    None => (run.text.clone(), false),
                };
                let page_id = id.to_string();
                let kind = if is_child {
                    BlockKind::ChildPage { page_id, title }
                } else {
                    BlockKind::PageLink { page_id, title }
                };
                return Block::new(kind);
            }
        }
        paragraph_of(inline)
    }

    /// True when the frame below the top accepts page blocks.
    fn parent_allows_page_block(&self) -> bool {
        let depth = self.stack.len();
        depth >= 2 && matches!(self.stack[depth - 2].kind, Kind::Root | Kind::Quote(_))
    }

    /// Turns pending direct text of the top frame into a paragraph, and splits
    /// a paragraph that a block-level element interrupts.
    fn flush_for_block(&mut self) {
        let allow = matches!(self.top().kind, Kind::Root | Kind::Quote(_));
        let depth = self.stack.len();
        if self.top().is_container() {
            let inline = std::mem::take(&mut self.top().inline);
            if !inline.is_empty() {
                let block = self.paragraph(inline, allow);
                self.top().blocks.push(block);
            }
        } else if matches!(self.top().kind, Kind::Paragraph) && depth >= 2 {
            let inline = std::mem::take(&mut self.top().inline);
            if !normalize_inline(inline.clone()).is_empty() {
                let block = self.paragraph(inline, self.parent_allows_page_block());
                self.stack[depth - 2].blocks.push(block);
            }
        }
    }

    /// Adds a finished block to the nearest frame that holds blocks.
    fn emit(&mut self, block: Block) {
        let depth = self.stack.len();
        if matches!(
            self.top().kind,
            Kind::Paragraph | Kind::Heading(_) | Kind::Row
        ) && depth >= 2
        {
            self.stack[depth - 2].blocks.push(block);
        } else {
            self.top().blocks.push(block);
        }
    }

    fn start(&mut self, tag: Tag<'_>, source: &str) {
        match tag {
            Tag::Paragraph => {
                self.flush_for_block();
                let parent = self.top();
                if matches!(parent.kind, Kind::Quote(_)) && parent.blocks.is_empty() {
                    parent.marker_in_source = source.starts_with("[!");
                }
                self.stack.push(Frame::new(Kind::Paragraph));
            }
            Tag::Heading { level, .. } => {
                self.flush_for_block();
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                };
                self.stack.push(Frame::new(Kind::Heading(level)));
            }
            Tag::BlockQuote(kind) => {
                self.flush_for_block();
                self.stack.push(Frame::new(Kind::Quote(kind)));
            }
            Tag::CodeBlock(kind) => {
                self.flush_for_block();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.stack.push(Frame::new(Kind::Code(language)));
            }
            Tag::MetadataBlock(kind) => {
                self.flush_for_block();
                let language = match kind {
                    MetadataBlockKind::YamlStyle => "yaml",
                    MetadataBlockKind::PlusesStyle => "toml",
                };
                self.stack.push(Frame::new(Kind::Code(language.to_owned())));
            }
            Tag::HtmlBlock => {
                self.flush_for_block();
                self.stack.push(Frame::new(Kind::Html));
            }
            Tag::List(start) => {
                self.flush_for_block();
                self.stack.push(Frame::new(Kind::List(start.is_some())));
            }
            Tag::Item => self.stack.push(Frame::new(Kind::Item(None))),
            Tag::FootnoteDefinition(label) => {
                self.flush_for_block();
                self.stack
                    .push(Frame::new(Kind::Footnote(label.to_string())));
            }
            Tag::Table(_) => {
                self.flush_for_block();
                self.stack.push(Frame::new(Kind::Table));
            }
            Tag::TableHead | Tag::TableRow => self.stack.push(Frame::new(Kind::Row)),
            Tag::TableCell => {
                if self.top().cells > 0 {
                    self.top().inline.push(Inline::text(" | "));
                }
                self.top().cells += 1;
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                let dest = dest_url.to_string();
                let href = if parse_page_href(&dest).is_some() {
                    dest
                } else if let Some(target) = self.env.page(&dest) {
                    let href = page_href(target.id);
                    self.resolved.insert(href.clone(), target);
                    href
                } else {
                    dest
                };
                self.links.push(href);
            }
            Tag::Image { dest_url, .. } => {
                if !matches!(self.top().kind, Kind::Heading(_) | Kind::Row) {
                    self.flush_for_block();
                }
                self.stack
                    .push(Frame::new(Kind::Image(dest_url.to_string())));
            }
            // Constructs whose options are off, plus footnotes and the like.
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                let frame = self.pop();
                let block = self.paragraph(
                    frame.inline,
                    matches!(
                        self.stack.last().map(|f| &f.kind),
                        Some(Kind::Root | Kind::Quote(_))
                    ),
                );
                if !block.inline.is_empty() || !matches!(block.kind, BlockKind::Paragraph) {
                    self.top().blocks.push(block);
                }
            }
            TagEnd::Heading(_) => {
                let frame = self.pop();
                let Kind::Heading(level) = frame.kind else {
                    return;
                };
                let block = Block::new(BlockKind::Heading { level }).with_inline(frame.inline);
                if !block.inline.is_empty() {
                    self.top().blocks.push(block);
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush_for_block();
                let frame = self.pop();
                let Kind::Quote(alert) = frame.kind else {
                    return;
                };
                let block = finish_quote(alert, frame.blocks, frame.marker_in_source);
                self.top().blocks.push(block);
            }
            TagEnd::CodeBlock | TagEnd::MetadataBlock(_) => {
                let frame = self.pop();
                let Kind::Code(language) = frame.kind else {
                    return;
                };
                let mut text = plain_of(&frame.inline);
                if text.ends_with('\n') {
                    text.pop();
                }
                let kind = BlockKind::CodeBlock { language };
                let block = if text.is_empty() {
                    Block::new(kind)
                } else {
                    Block::text(kind, &text)
                };
                self.top().blocks.push(block);
            }
            TagEnd::HtmlBlock => {
                let frame = self.pop();
                let text = plain_of(&frame.inline);
                let text = text.trim();
                if !text.is_empty() && !(text.starts_with("<!--") && text.ends_with("-->")) {
                    let mut inline = Vec::new();
                    for (i, line) in text.lines().enumerate() {
                        if i > 0 {
                            inline.push(Inline::Break);
                        }
                        inline.push(Inline::text(line.trim()));
                    }
                    self.top().blocks.push(paragraph_of(inline));
                }
            }
            TagEnd::Item => {
                self.flush_for_block();
                let frame = self.pop();
                let Kind::Item(task) = frame.kind else { return };
                let mut children = frame.blocks;
                if !matches!(
                    children.first().map(|b| &b.kind),
                    Some(BlockKind::Paragraph)
                ) {
                    children.insert(0, Block::new(BlockKind::Paragraph));
                }
                let kind = match task {
                    Some(checked) => BlockKind::TodoItem { checked },
                    None => BlockKind::ListItem,
                };
                self.top()
                    .blocks
                    .push(Block::new(kind).with_children(children));
            }
            TagEnd::FootnoteDefinition => {
                self.flush_for_block();
                let frame = self.pop();
                let Kind::Footnote(label) = frame.kind else {
                    return;
                };
                // A footnote has no block of its own: its body stays in the
                // page as paragraphs, the first one led by its label.
                let mut blocks = frame.blocks;
                let lead = Inline::text(format!("[^{label}]: "));
                match blocks.first_mut() {
                    Some(first) if first.kind == BlockKind::Paragraph => {
                        first.inline.insert(0, lead);
                        first.inline = normalize_inline(std::mem::take(&mut first.inline));
                    }
                    _ => blocks.insert(0, paragraph_of(vec![lead])),
                }
                self.top().blocks.extend(blocks);
            }
            TagEnd::List(_) => {
                let frame = self.pop();
                let Kind::List(ordered) = frame.kind else {
                    return;
                };
                let lists = group_items(frame.blocks, ordered);
                self.top().blocks.extend(lists);
            }
            TagEnd::Table => {
                let frame = self.pop();
                self.top().blocks.extend(frame.blocks);
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let frame = self.pop();
                let block = paragraph_of(frame.inline);
                if !block.inline.is_empty() {
                    self.top().blocks.push(block);
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                let frame = self.pop();
                let Kind::Image(dest) = frame.kind else {
                    return;
                };
                let alt = plain_of(&frame.inline).replace('\n', " ");
                if matches!(self.top().kind, Kind::Heading(_) | Kind::Row) {
                    self.push_text(&alt, false);
                } else {
                    let src = self.env.image(&dest);
                    self.emit(Block::new(BlockKind::Image { src, alt }));
                }
            }
            _ => {}
        }
    }

    fn event(&mut self, event: Event<'_>, source: &str) {
        match event {
            Event::Start(tag) => self.start(tag, source),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => {
                if matches!(self.top().kind, Kind::Code(_) | Kind::Html | Kind::Image(_)) {
                    self.top().inline.push(Inline::text(text.as_ref()));
                } else {
                    self.push_text(&text, false);
                }
            }
            Event::Html(html) => self.top().inline.push(Inline::text(html.as_ref())),
            Event::Code(code) => self.push_text(&code, true),
            Event::InlineHtml(html) => {
                let tag = html.trim().to_lowercase();
                if matches!(tag.as_str(), "<br>" | "<br/>" | "<br />") {
                    self.top().inline.push(Inline::Break);
                } else if !(tag.starts_with("<!--") && tag.ends_with("-->")) {
                    self.push_text(&html, false);
                }
            }
            Event::FootnoteReference(label) => self.push_text(&format!("[^{label}]"), false),
            Event::SoftBreak => self.push_text(" ", false),
            Event::HardBreak => self.top().inline.push(Inline::Break),
            Event::Rule => {
                self.flush_for_block();
                self.emit(Block::new(BlockKind::Divider));
            }
            Event::TaskListMarker(checked) => {
                if let Kind::Item(task) = &mut self.top().kind {
                    *task = Some(checked);
                }
            }
            _ => {}
        }
    }
}

/// Splits a list's items into lists of one kind each: to-do items and plain
/// items sharing a Markdown list become separate blocks.
fn group_items(items: Vec<Block>, ordered: bool) -> Vec<Block> {
    let mut lists: Vec<Block> = Vec::new();
    for item in items {
        let todo = matches!(item.kind, BlockKind::TodoItem { .. });
        let list_kind = match (todo, ordered) {
            (true, _) => BlockKind::TodoList,
            (false, true) => BlockKind::OrderedList,
            (false, false) => BlockKind::BulletList,
        };
        match lists.last_mut() {
            Some(last) if last.kind == list_kind => last.children.push(item),
            _ => lists.push(Block::new(list_kind).with_children(vec![item])),
        }
    }
    lists
}

/// Decides what a block quote is: a GFM alert or `[!callout]` becomes a
/// callout, `[!toggle]` a toggle, anything else a plain quote.
fn finish_quote(
    alert: Option<BlockQuoteKind>,
    blocks: Vec<Block>,
    marker_in_source: bool,
) -> Block {
    if let Some(kind) = alert {
        return Block::new(BlockKind::Callout {
            icon: alert_icon(kind).to_owned(),
        })
        .with_children(with_content(blocks));
    }
    let marker = match blocks.first() {
        Some(first) if marker_in_source && first.kind == BlockKind::Paragraph => {
            parse_marker(&first.inline)
        }
        _ => None,
    };
    let Some((marker, rest)) = marker else {
        return Block::new(BlockKind::Quote).with_children(with_content(blocks));
    };
    let mut body: Vec<Block> = blocks.into_iter().skip(1).collect();
    match marker {
        Marker::Callout { icon } => {
            if !rest.is_empty() {
                body.insert(0, paragraph_of(rest));
            }
            Block::new(BlockKind::Callout { icon }).with_children(with_content(body))
        }
        Marker::Toggle { open } => {
            body.insert(0, paragraph_of(rest));
            Block::new(BlockKind::Toggle { open }).with_children(body)
        }
    }
}

/// Parses Markdown into blocks. Constructs without a block equivalent keep
/// their text: raw HTML and table rows become paragraphs, front matter a code
/// block, footnotes plain paragraphs, headings deeper than 3 level-3 headings. A `>` quote is always a
/// quote, except for the callout and toggle conventions described in
/// [`crate::markdown`].
pub fn import_blocks(markdown: &str, env: &mut dyn ImportEnv) -> ImportedPage {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM;
    let markdown = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let mut builder = Builder::new(env);
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        builder.event(event, markdown.get(range.start..).unwrap_or_default());
    }
    builder.flush_for_block();
    let mut blocks = builder.stack.swap_remove(0).blocks;
    let mut title = None;
    if let Some(first) = blocks.first() {
        let text = plain_of(&first.inline).trim().to_owned();
        if first.kind == (BlockKind::Heading { level: 1 }) && !text.is_empty() {
            title = Some(text);
            blocks.remove(0);
        }
    }
    ImportedPage { title, blocks }
}

/// True when `href` looks like a link to something outside the page tree
/// (it has a URL scheme or is a fragment), as opposed to a relative path.
pub(crate) fn is_external(href: &str) -> bool {
    if href.starts_with('#') || href.starts_with("//") || href.starts_with(PAGE_HREF_PREFIX) {
        return true;
    }
    match href.split_once(':') {
        Some((scheme, _)) => {
            scheme.len() > 1
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}
