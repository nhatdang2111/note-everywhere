//! Reads the container layout of ADR 0006 into the [`Block`] model.

use loro::{Container, LoroList, LoroMap, LoroText, LoroValue, TextDelta, ValueOrContainer};

use super::model::{Block, BlockKind, Inline, Marks, Run, normalize_inline};
use crate::error::{Error, Result};

pub(crate) const KEY_NODE_NAME: &str = "nodeName";
pub(crate) const KEY_ATTRIBUTES: &str = "attributes";
pub(crate) const KEY_CHILDREN: &str = "children";
pub(crate) const HARD_BREAK: &str = "hard_break";

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidPage(message.into())
}

pub(crate) fn node_name(node: &LoroMap) -> Result<String> {
    match node.get(KEY_NODE_NAME) {
        Some(ValueOrContainer::Value(LoroValue::String(name))) => Ok(name.to_string()),
        _ => Err(invalid("node has no string nodeName")),
    }
}

pub(crate) fn attributes_map(node: &LoroMap) -> Result<LoroMap> {
    match node
        .get(KEY_ATTRIBUTES)
        .map(ValueOrContainer::into_container)
    {
        Some(Ok(Container::Map(map))) => Ok(map),
        _ => Err(invalid("node has no attributes map")),
    }
}

pub(crate) fn children_list(node: &LoroMap) -> Result<LoroList> {
    match node.get(KEY_CHILDREN).map(ValueOrContainer::into_container) {
        Some(Ok(Container::List(list))) => Ok(list),
        _ => Err(invalid("node has no children list")),
    }
}

/// One child of a node: a nested node or a text container.
pub(crate) enum Child {
    Node(LoroMap),
    Text(LoroText),
}

pub(crate) fn child_at(list: &LoroList, index: usize) -> Result<Child> {
    match list.get(index).map(ValueOrContainer::into_container) {
        Some(Ok(Container::Map(map))) => Ok(Child::Node(map)),
        Some(Ok(Container::Text(text))) => Ok(Child::Text(text)),
        Some(_) => Err(invalid("a child is neither a node nor a text container")),
        None => Err(invalid("children list ended early")),
    }
}

fn read_string(attrs: &LoroMap, key: &str) -> String {
    match attrs.get(key) {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => s.to_string(),
        _ => String::new(),
    }
}

fn read_bool(attrs: &LoroMap, key: &str, default: bool) -> bool {
    match attrs.get(key) {
        Some(ValueOrContainer::Value(LoroValue::Bool(b))) => b,
        _ => default,
    }
}

/// Numbers arrive as `I64` from Rust and as `Double` from the JS binding.
fn read_level(attrs: &LoroMap) -> u8 {
    let level = match attrs.get("level") {
        Some(ValueOrContainer::Value(LoroValue::I64(n))) => n,
        Some(ValueOrContainer::Value(LoroValue::Double(d))) => d as i64,
        _ => 1,
    };
    level.clamp(1, 3) as u8
}

/// Builds the kind from a node name, reading attributes only for kinds that
/// have them. Unknown names are kept as [`BlockKind::Unknown`].
pub(crate) fn kind_from_node(
    name: &str,
    attrs: impl FnOnce() -> Result<LoroMap>,
) -> Result<BlockKind> {
    Ok(match name {
        "paragraph" => BlockKind::Paragraph,
        "bullet_list" => BlockKind::BulletList,
        "ordered_list" => BlockKind::OrderedList,
        "list_item" => BlockKind::ListItem,
        "todo_list" => BlockKind::TodoList,
        "divider" => BlockKind::Divider,
        "quote" => BlockKind::Quote,
        "heading" => BlockKind::Heading {
            level: read_level(&attrs()?),
        },
        "todo_item" => BlockKind::TodoItem {
            checked: read_bool(&attrs()?, "checked", false),
        },
        "toggle" => BlockKind::Toggle {
            open: read_bool(&attrs()?, "open", true),
        },
        "callout" => BlockKind::Callout {
            icon: read_string(&attrs()?, "icon"),
        },
        "code_block" => BlockKind::CodeBlock {
            language: read_string(&attrs()?, "language"),
        },
        "image" => {
            let attrs = attrs()?;
            BlockKind::Image {
                src: read_string(&attrs, "src"),
                alt: read_string(&attrs, "alt"),
            }
        }
        "child_page" => {
            let attrs = attrs()?;
            BlockKind::ChildPage {
                page_id: read_string(&attrs, "pageId"),
                title: read_string(&attrs, "title"),
            }
        }
        "page_link" => {
            let attrs = attrs()?;
            BlockKind::PageLink {
                page_id: read_string(&attrs, "pageId"),
                title: read_string(&attrs, "title"),
            }
        }
        other => BlockKind::Unknown {
            node_name: other.to_owned(),
        },
    })
}

fn read_marks(attributes: Option<impl IntoIterator<Item = (String, LoroValue)>>) -> Marks {
    let mut marks = Marks::default();
    for (key, value) in attributes.into_iter().flatten() {
        if value == LoroValue::Null {
            continue;
        }
        match key.as_str() {
            "bold" => marks.bold = true,
            "italic" => marks.italic = true,
            "code" => marks.code = true,
            "strike" => marks.strike = true,
            "link" => {
                marks.link = Some(match &value {
                    LoroValue::Map(map) => match map.get("href") {
                        Some(LoroValue::String(href)) => href.to_string(),
                        _ => String::new(),
                    },
                    LoroValue::String(href) => href.to_string(),
                    _ => String::new(),
                });
            }
            _ => {}
        }
    }
    marks
}

pub(crate) fn read_runs(text: &LoroText, out: &mut Vec<Inline>) {
    for delta in text.to_delta() {
        if let TextDelta::Insert { insert, attributes } = delta {
            out.push(Inline::Text(Run {
                text: insert,
                marks: read_marks(attributes),
            }));
        }
    }
}

/// Reads one node and everything below it.
pub(crate) fn read_block(node: &LoroMap) -> Result<Block> {
    let name = node_name(node)?;
    let kind = kind_from_node(&name, || attributes_map(node))?;
    let mut block = Block::new(kind);
    if matches!(block.kind, BlockKind::Unknown { .. }) {
        return Ok(block);
    }
    let children = children_list(node)?;
    let mut inline = Vec::new();
    for index in 0..children.len() {
        match child_at(&children, index)? {
            Child::Text(text) => read_runs(&text, &mut inline),
            Child::Node(child) if block.kind.is_text_block() => {
                // The only inline node of the schema is the soft line break;
                // anything else inside a text block is ignored.
                if node_name(&child)? == HARD_BREAK {
                    inline.push(Inline::Break);
                }
            }
            Child::Node(child) => block.children.push(read_block(&child)?),
        }
    }
    block.inline = normalize_inline(inline);
    Ok(block)
}

/// Reads the top-level blocks of the page root.
pub(crate) fn read_root(root: &LoroMap) -> Result<Vec<Block>> {
    if node_name(root)? != "doc" {
        return Err(invalid("root node is not a doc"));
    }
    let children = children_list(root)?;
    let mut blocks = Vec::with_capacity(children.len());
    for index in 0..children.len() {
        match child_at(&children, index)? {
            Child::Node(node) => blocks.push(read_block(&node)?),
            Child::Text(_) => return Err(invalid("the page root holds a text container")),
        }
    }
    Ok(blocks)
}
