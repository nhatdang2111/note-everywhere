//! Plain data types of a page: blocks, inline content, marks and the link
//! conventions stored in attributes.

use crate::ids::NodeId;

/// Prefix of a link target that points at another page: `ne-page:<node id>`.
/// It is used in the `href` of a `link` mark and nowhere else; the block kinds
/// `child_page` and `page_link` carry the bare node id in `pageId`.
pub const PAGE_HREF_PREFIX: &str = "ne-page:";

/// Prefix of an image source that points at a stored attachment:
/// `attachment:<sha-256 hex>`. Any other source (a relative path, a URL) is
/// kept as written and is not an attachment reference.
pub const ATTACHMENT_SRC_PREFIX: &str = "attachment:";

/// Length of an attachment content hash in lowercase hex characters.
pub const ATTACHMENT_HASH_LEN: usize = 64;

/// The `href` of a link mark that points at page `id`.
pub fn page_href(id: NodeId) -> String {
    format!("{PAGE_HREF_PREFIX}{id}")
}

/// The page a link `href` points at, if it uses the page link convention.
pub fn parse_page_href(href: &str) -> Option<NodeId> {
    href.strip_prefix(PAGE_HREF_PREFIX)?.parse().ok()
}

/// The image `src` that points at the attachment with content hash `hash`.
pub fn attachment_src(hash: &str) -> String {
    format!("{ATTACHMENT_SRC_PREFIX}{hash}")
}

/// The attachment content hash an image `src` refers to, if it uses the
/// attachment convention and the hash is well formed.
pub fn parse_attachment_src(src: &str) -> Option<&str> {
    let hash = src.strip_prefix(ATTACHMENT_SRC_PREFIX)?;
    let valid = hash.len() == ATTACHMENT_HASH_LEN
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    valid.then_some(hash)
}

/// The kind of a block together with the attributes that belong to the kind.
///
/// The 15 block kinds of the product spec are [`Paragraph`](Self::Paragraph),
/// three heading levels, both list kinds, [`TodoList`](Self::TodoList),
/// [`Toggle`](Self::Toggle), [`Quote`](Self::Quote), [`Callout`](Self::Callout),
/// [`Divider`](Self::Divider), [`CodeBlock`](Self::CodeBlock),
/// [`Image`](Self::Image), [`ChildPage`](Self::ChildPage) and
/// [`PageLink`](Self::PageLink). [`ListItem`](Self::ListItem) and
/// [`TodoItem`](Self::TodoItem) are the item wrappers inside lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// A paragraph of inline content.
    Paragraph,
    /// A heading of level 1 to 3.
    Heading {
        /// Heading level, 1 to 3.
        level: u8,
    },
    /// Bulleted list; its children are [`ListItem`](Self::ListItem)s.
    BulletList,
    /// Numbered list; its children are [`ListItem`](Self::ListItem)s.
    OrderedList,
    /// One list entry: a paragraph, then optional nested blocks.
    ListItem,
    /// To-do list; its children are [`TodoItem`](Self::TodoItem)s.
    TodoList,
    /// One to-do entry: a paragraph, then optional nested blocks.
    TodoItem {
        /// Whether the box is ticked.
        checked: bool,
    },
    /// Collapsible block: the first child paragraph is the always visible
    /// summary, the rest is the collapsible body.
    Toggle {
        /// Whether the body is currently shown.
        open: bool,
    },
    /// A quotation holding one or more blocks.
    Quote,
    /// A highlighted box holding one or more blocks.
    Callout {
        /// Icon shown beside the body; may be empty.
        icon: String,
    },
    /// A horizontal rule.
    Divider,
    /// Preformatted code. Its text is one unmarked run.
    CodeBlock {
        /// Language name; empty when unspecified.
        language: String,
    },
    /// An image.
    Image {
        /// Where the image comes from, see [`parse_attachment_src`].
        src: String,
        /// Alternative text.
        alt: String,
    },
    /// Reference to a page that is a child of this page in the tree.
    ChildPage {
        /// Node id of the child page as written in the document.
        page_id: String,
        /// Display copy of the title; the tree node is the source of truth.
        title: String,
    },
    /// Link to any page. It creates no parent and child relation.
    PageLink {
        /// Node id of the target page as written in the document.
        page_id: String,
        /// Display copy of the title; the tree node is the source of truth.
        title: String,
    },
    /// A node name this version does not know. It is kept in place and never
    /// written or removed by this crate.
    Unknown {
        /// The stored `nodeName`.
        node_name: String,
    },
}

impl BlockKind {
    /// The `nodeName` stored in the document.
    pub fn node_name(&self) -> &str {
        match self {
            Self::Paragraph => "paragraph",
            Self::Heading { .. } => "heading",
            Self::BulletList => "bullet_list",
            Self::OrderedList => "ordered_list",
            Self::ListItem => "list_item",
            Self::TodoList => "todo_list",
            Self::TodoItem { .. } => "todo_item",
            Self::Toggle { .. } => "toggle",
            Self::Quote => "quote",
            Self::Callout { .. } => "callout",
            Self::Divider => "divider",
            Self::CodeBlock { .. } => "code_block",
            Self::Image { .. } => "image",
            Self::ChildPage { .. } => "child_page",
            Self::PageLink { .. } => "page_link",
            Self::Unknown { node_name } => node_name,
        }
    }

    /// True when the block holds inline content (text and line breaks).
    pub fn is_text_block(&self) -> bool {
        matches!(
            self,
            Self::Paragraph | Self::Heading { .. } | Self::CodeBlock { .. }
        )
    }

    /// True for the item wrappers that only live directly inside a list.
    pub fn is_item(&self) -> bool {
        matches!(self, Self::ListItem | Self::TodoItem { .. })
    }

    /// True for blocks that hold neither inline content nor children.
    pub fn is_atom(&self) -> bool {
        matches!(
            self,
            Self::Divider | Self::Image { .. } | Self::ChildPage { .. } | Self::PageLink { .. }
        )
    }
}

/// Inline formatting of a run of text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Inline code.
    pub code: bool,
    /// Strike-through.
    pub strike: bool,
    /// Link target. A page link has the form described at [`PAGE_HREF_PREFIX`].
    pub link: Option<String>,
}

impl Marks {
    /// True when no mark is set.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

/// A stretch of text that shares one set of marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The text.
    pub text: String,
    /// Its formatting.
    pub marks: Marks,
}

impl Run {
    /// A run without marks.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            marks: Marks::default(),
        }
    }

    /// The page this run links to, when it carries a page link mark.
    pub fn page_link(&self) -> Option<NodeId> {
        parse_page_href(self.marks.link.as_deref()?)
    }
}

/// One element of a text block's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// Text with marks.
    Text(Run),
    /// A soft line break inside the block (Shift+Enter).
    Break,
}

impl Inline {
    /// Plain unmarked text.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(Run::plain(text))
    }
}

/// Drops empty runs and merges neighbouring runs that have equal marks. The
/// document stores each stretch of consecutive text in one container, so this
/// is the canonical form of inline content.
pub fn normalize_inline(inline: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inline.len());
    for item in inline {
        match item {
            Inline::Text(run) if run.text.is_empty() => {}
            Inline::Text(run) => match out.last_mut() {
                Some(Inline::Text(prev)) if prev.marks == run.marks => {
                    prev.text.push_str(&run.text);
                }
                _ => out.push(Inline::Text(run)),
            },
            Inline::Break => out.push(Inline::Break),
        }
    }
    out
}

/// A block as read from or written to a page.
///
/// Text blocks (paragraph, heading, code) fill [`inline`](Self::inline) and
/// have no children; container blocks fill [`children`](Self::children) and
/// have no inline content. A path to a block is the list of child indexes from
/// the page root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Kind and attributes.
    pub kind: BlockKind,
    /// Inline content of a text block.
    pub inline: Vec<Inline>,
    /// Nested blocks of a container block.
    pub children: Vec<Block>,
}

impl Block {
    /// A block with no content.
    pub fn new(kind: BlockKind) -> Self {
        Self {
            kind,
            inline: Vec::new(),
            children: Vec::new(),
        }
    }

    /// A text block holding one plain run.
    pub fn text(kind: BlockKind, text: &str) -> Self {
        Self::new(kind).with_inline(vec![Inline::text(text)])
    }

    /// A paragraph holding one plain run.
    pub fn paragraph(text: &str) -> Self {
        Self::text(BlockKind::Paragraph, text)
    }

    /// A heading holding one plain run.
    pub fn heading(level: u8, text: &str) -> Self {
        Self::text(BlockKind::Heading { level }, text)
    }

    /// Replaces the inline content.
    pub fn with_inline(mut self, inline: Vec<Inline>) -> Self {
        self.inline = normalize_inline(inline);
        self
    }

    /// Replaces the nested blocks.
    pub fn with_children(mut self, children: Vec<Block>) -> Self {
        self.children = children;
        self
    }

    /// Text of the block as the search index sees it, see
    /// [`crate::page::block_plain_text`].
    pub fn plain_text(&self) -> String {
        super::extract::block_plain_text(self)
    }
}
