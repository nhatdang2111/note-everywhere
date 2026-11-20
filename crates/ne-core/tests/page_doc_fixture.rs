//! Reads and writes page documents the way the editor binding lays them out.
//!
//! The fixtures are produced by `apps/desktop/spikes/page-doc-fixture.mjs`
//! (ProseMirror + `loro-prosemirror` + `loro-crdt` WASM). This test walks them
//! with the native `loro` crate, then edits natively and exports an update that
//! `apps/desktop/spikes/roundtrip-check.mjs` imports back into the editor.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use loro::{
    Container, ExpandType, ExportMode, LoroDoc, LoroList, LoroMap, LoroText, LoroValue,
    StyleConfig, StyleConfigMap, TextDelta, ValueOrContainer,
};

const SNAPSHOT: &[u8] = include_bytes!("fixtures/page-doc-snapshot.loro");
const UPDATE: &[u8] = include_bytes!("fixtures/page-doc-update.loro");

// ---------------------------------------------------------------------------
// Reader: Loro containers -> plain Rust tree
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Run {
    text: String,
    marks: BTreeMap<String, LoroValue>,
}

#[derive(Debug)]
enum Child {
    Node(Block),
    Text(Vec<Run>),
}

#[derive(Debug)]
struct Block {
    kind: String,
    attrs: BTreeMap<String, LoroValue>,
    children: Vec<Child>,
}

fn as_map(value: ValueOrContainer) -> LoroMap {
    match value.into_container() {
        Ok(Container::Map(map)) => map,
        other => panic!("expected a map container, got {other:?}"),
    }
}

fn as_list(value: ValueOrContainer) -> LoroList {
    match value.into_container() {
        Ok(Container::List(list)) => list,
        other => panic!("expected a list container, got {other:?}"),
    }
}

fn read_text(text: &LoroText) -> Vec<Run> {
    text.to_delta()
        .into_iter()
        .map(|delta| match delta {
            TextDelta::Insert { insert, attributes } => Run {
                text: insert,
                marks: attributes.unwrap_or_default().into_iter().collect(),
            },
            other => panic!("to_delta must only contain inserts, got {other:?}"),
        })
        .collect()
}

fn read_block(node: &LoroMap) -> Block {
    let kind = match node.get("nodeName").expect("nodeName") {
        ValueOrContainer::Value(LoroValue::String(name)) => name.to_string(),
        other => panic!("nodeName must be a string, got {other:?}"),
    };
    let attrs = match as_map(node.get("attributes").expect("attributes")).get_value() {
        LoroValue::Map(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        other => panic!("attributes must be a map, got {other:?}"),
    };
    let children_list = as_list(node.get("children").expect("children"));
    let children = (0..children_list.len())
        .map(
            |i| match children_list.get(i).expect("child").into_container() {
                Ok(Container::Map(map)) => Child::Node(read_block(&map)),
                Ok(Container::Text(text)) => Child::Text(read_text(&text)),
                other => panic!("unexpected child container {other:?}"),
            },
        )
        .collect();
    Block {
        kind,
        attrs,
        children,
    }
}

fn read_page(doc: &LoroDoc) -> Block {
    read_block(&doc.get_map("doc"))
}

/// Text of a block as the search index would see it: inline text concatenated,
/// `hard_break` as a newline, nested blocks separated by a newline.
fn plain_text(block: &Block) -> String {
    if block.kind == "hard_break" {
        return "\n".into();
    }
    let is_inline_parent = block.children.iter().any(|child| match child {
        Child::Text(_) => true,
        Child::Node(node) => node.kind == "hard_break",
    });
    if is_inline_parent {
        block
            .children
            .iter()
            .map(|child| match child {
                Child::Text(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
                Child::Node(node) => plain_text(node),
            })
            .collect()
    } else {
        block
            .children
            .iter()
            .filter_map(|child| match child {
                Child::Node(node) => Some(plain_text(node)),
                Child::Text(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn blocks(page: &Block) -> Vec<&Block> {
    page.children
        .iter()
        .map(|child| match child {
            Child::Node(node) => node,
            Child::Text(_) => panic!("the doc root only holds blocks"),
        })
        .collect()
}

fn children_of(block: &Block) -> Vec<&Block> {
    blocks(block)
}

fn load(with_update: bool) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(SNAPSHOT).expect("import snapshot");
    if with_update {
        doc.import(UPDATE).expect("import update");
    }
    doc
}

fn mark_names(run: &Run) -> Vec<&str> {
    let mut names: Vec<&str> = run.marks.keys().map(String::as_str).collect();
    names.sort_unstable();
    names
}

fn str_attr<'a>(block: &'a Block, key: &str) -> &'a str {
    match block.attrs.get(key) {
        Some(LoroValue::String(value)) => value.as_str(),
        other => panic!("attribute {key} is not a string: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Reader tests
// ---------------------------------------------------------------------------

const EXPECTED_KINDS: [&str; 17] = [
    "heading",
    "heading",
    "heading",
    "paragraph",
    "paragraph",
    "bullet_list",
    "ordered_list",
    "todo_list",
    "toggle",
    "quote",
    "callout",
    "divider",
    "code_block",
    "image",
    "child_page",
    "page_link",
    "paragraph",
];

#[test]
fn snapshot_has_every_block_kind_in_order() {
    let page = read_page(&load(false));
    assert_eq!(page.kind, "doc");
    let kinds: Vec<&str> = blocks(&page).iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(kinds, EXPECTED_KINDS);

    let top = blocks(&page);
    let levels: Vec<&LoroValue> = top[..3].iter().map(|b| &b.attrs["level"]).collect();
    assert_eq!(
        levels,
        [&LoroValue::I64(1), &LoroValue::I64(2), &LoroValue::I64(3)]
    );
}

#[test]
fn snapshot_plain_text_keeps_diacritics() {
    let page = read_page(&load(false));
    let texts: Vec<String> = blocks(&page).iter().map(|b| plain_text(b)).collect();
    let expected = [
        "Tiêu đề trang",
        "Mục lớn: Heading two",
        "Mục nhỏ",
        "Xin chào, thế giới and English code() bỏ đi liên kết đậm nghiêng",
        "Dòng một\nDòng hai",
        "Mục một\nMục con ở trong\nMục hai",
        "Bước một\nStep two",
        "Đã xong\nChưa xong",
        "Toggle tóm tắt\nNội dung ẩn bên trong",
        "Trích dẫn: người Việt nói “có công mài sắt”",
        "Lưu ý quan trọng / important note",
        "",
        "fn main() {\n    println!(\"Xin chào\");\n}",
        "",
        "",
        "",
        "",
    ];
    assert_eq!(texts, expected);
}

#[test]
fn list_items_nest_through_list_item_blocks() {
    let page = read_page(&load(false));
    let top = blocks(&page);
    let bullet = top[5];

    let items = children_of(bullet);
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| item.kind == "list_item"));

    // First item: its own paragraph, then a nested bullet_list with one item.
    let first = children_of(items[0]);
    assert_eq!(
        first.iter().map(|b| b.kind.as_str()).collect::<Vec<_>>(),
        ["paragraph", "bullet_list"]
    );
    let nested_items = children_of(first[1]);
    assert_eq!(nested_items.len(), 1);
    assert_eq!(plain_text(nested_items[0]), "Mục con ở trong");

    // Second item has no nesting.
    assert_eq!(children_of(items[1]).len(), 1);

    // Ordered list is a sibling kind with the same item shape.
    let ordered_items = children_of(top[6]);
    assert_eq!(ordered_items.len(), 2);
    assert!(ordered_items.iter().all(|item| item.kind == "list_item"));
}

#[test]
fn todo_checked_flag_is_a_boolean_attribute() {
    let page = read_page(&load(false));
    let items = children_of(blocks(&page)[7]);
    let flags: Vec<&LoroValue> = items.iter().map(|item| &item.attrs["checked"]).collect();
    assert_eq!(flags, [&LoroValue::Bool(true), &LoroValue::Bool(false)]);
}

#[test]
fn inline_marks_are_text_attributes_keyed_by_mark_name() {
    let page = read_page(&load(false));
    let paragraph = blocks(&page)[3];
    let Child::Text(runs) = &paragraph.children[0] else {
        panic!("first child of a text block is its text container");
    };
    let summary: Vec<(&str, Vec<&str>)> = runs
        .iter()
        .map(|run| (run.text.as_str(), mark_names(run)))
        .collect();
    assert_eq!(
        summary,
        [
            ("Xin chào, ", vec![]),
            ("thế giới", vec!["bold"]),
            (" and ", vec![]),
            ("English", vec!["italic"]),
            (" ", vec![]),
            ("code()", vec!["code"]),
            (" ", vec![]),
            ("bỏ đi", vec!["strike"]),
            (" ", vec![]),
            ("liên kết", vec!["link"]),
            (" ", vec![]),
            ("đậm nghiêng", vec!["bold", "italic"]),
        ]
    );

    // Marks without attributes carry an empty map; link carries its href.
    assert_eq!(runs[1].marks["bold"], LoroValue::Map(Default::default()));
    let LoroValue::Map(link) = &runs[9].marks["link"] else {
        panic!("link mark value must be a map");
    };
    assert_eq!(
        link.get("href"),
        Some(&LoroValue::from("https://example.org/vi"))
    );
}

#[test]
fn atom_blocks_keep_their_data_in_attributes() {
    let page = read_page(&load(false));
    let top = blocks(&page);
    assert_eq!(str_attr(top[13], "src"), "assets/so-do.png");
    assert_eq!(str_attr(top[13], "alt"), "Sơ đồ kiến trúc");
    assert_eq!(str_attr(top[14], "pageId"), "page-child-0001");
    assert_eq!(str_attr(top[14], "title"), "Trang con");
    assert_eq!(str_attr(top[15], "pageId"), "page-target-0002");
    assert_eq!(str_attr(top[10], "icon"), "💡");
    assert_eq!(top[8].attrs["open"], LoroValue::Bool(false));
    assert_eq!(str_attr(top[12], "language"), "rust");
    assert!(top[11].children.is_empty(), "divider has no children");
}

#[test]
fn incremental_update_from_the_editor_applies_on_top_of_the_snapshot() {
    let page = read_page(&load(true));
    let top = blocks(&page);
    assert_eq!(plain_text(top[0]), "Tiêu đề trang (đã sửa)");
    let flags: Vec<&LoroValue> = children_of(top[7])
        .iter()
        .map(|i| &i.attrs["checked"])
        .collect();
    assert_eq!(flags, [&LoroValue::Bool(true), &LoroValue::Bool(true)]);
}

// ---------------------------------------------------------------------------
// Writer: native edits that the editor must accept
// ---------------------------------------------------------------------------

/// Mirror of the binding's mark configuration (`inclusive: false` -> none).
fn configure_page_text_styles(doc: &LoroDoc) {
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

/// Append a block map with the three keys the binding requires. Returns its
/// `children` list.
fn append_block(parent: &LoroList, kind: &str, attrs: &[(&str, LoroValue)]) -> LoroList {
    let node = parent.push_container(LoroMap::new()).expect("push node");
    node.insert("nodeName", kind).expect("nodeName");
    let attributes = node
        .insert_container("attributes", LoroMap::new())
        .expect("attributes");
    for (key, value) in attrs {
        attributes.insert(key, value.clone()).expect("attribute");
    }
    node.insert_container("children", LoroList::new())
        .expect("children")
}

/// Walk to `doc.children[i0].children[i1]...`, returning the last node's map.
fn node_at(doc: &LoroDoc, path: &[usize]) -> LoroMap {
    let mut node = doc.get_map("doc");
    for &index in path {
        let children = as_list(node.get("children").expect("children"));
        node = as_map(children.get(index).expect("child"));
    }
    node
}

fn rust_update_path() -> PathBuf {
    std::env::var_os("NE_PAGE_DOC_RUST_UPDATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ne-page-doc-rust-update.loro"))
}

#[test]
fn native_edits_export_an_update_the_editor_can_import() {
    let doc = load(true);
    configure_page_text_styles(&doc);
    let before = doc.oplog_vv();

    // 1. Insert text into an existing block: bullet_list > list_item[1] > paragraph.
    let paragraph = node_at(&doc, &[5, 1, 0]);
    let children = as_list(paragraph.get("children").expect("children"));
    let Container::Text(text) = children
        .get(0)
        .expect("text")
        .into_container()
        .expect("container")
    else {
        panic!("paragraph holds a text container");
    };
    let end = text.len_unicode();
    text.insert(end, " (sửa từ Rust)").expect("insert text");

    // 2. Append a new paragraph holding one marked run, as Markdown import would.
    let root_children = as_list(doc.get_map("doc").get("children").expect("children"));
    let new_children = append_block(&root_children, "paragraph", &[]);
    let new_text = new_children
        .push_container(LoroText::new())
        .expect("push text");
    new_text.insert(0, "Thêm từ Rust: in đậm").expect("insert");
    let start = "Thêm từ Rust: ".chars().count();
    new_text
        .mark(
            start..new_text.len_unicode(),
            "bold",
            LoroValue::from(HashMap::<String, LoroValue>::new()),
        )
        .expect("mark");
    doc.commit();

    // Native readback sees both edits.
    let page = read_page(&doc);
    let top = blocks(&page);
    assert_eq!(
        plain_text(top[5]),
        "Mục một\nMục con ở trong\nMục hai (sửa từ Rust)"
    );
    assert_eq!(top.len(), EXPECTED_KINDS.len() + 1);
    assert_eq!(
        plain_text(top[EXPECTED_KINDS.len()]),
        "Thêm từ Rust: in đậm"
    );

    let update = doc
        .export(ExportMode::updates(&before))
        .expect("export update");
    assert!(!update.is_empty());
    let path = rust_update_path();
    std::fs::write(&path, &update).expect("write update");

    // The update must apply to a replica that only has the editor's history.
    let replica = load(true);
    replica.import(&update).expect("import rust update");
    assert_eq!(replica.get_deep_value(), doc.get_deep_value());
}

#[test]
fn independently_created_roots_do_not_merge() {
    // Two peers that each create the root skeleton for the same page: one
    // `children` list wins the map key and the other peer's blocks vanish. The
    // page document must therefore be created once and shared as a snapshot.
    let make = |peer: u64, text: &str| {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        let root = doc.get_map("doc");
        root.insert("nodeName", "doc").expect("nodeName");
        root.insert_container("attributes", LoroMap::new())
            .expect("attributes");
        let children = root
            .insert_container("children", LoroList::new())
            .expect("children");
        let paragraph = append_block(&children, "paragraph", &[]);
        let body = paragraph.push_container(LoroText::new()).expect("text");
        body.insert(0, text).expect("insert");
        doc.commit();
        doc
    };
    let a = make(1, "from a");
    let b = make(2, "from b");
    let from_a = a.export(ExportMode::all_updates()).expect("export a");
    let from_b = b.export(ExportMode::all_updates()).expect("export b");
    a.import(&from_b).expect("import into a");
    b.import(&from_a).expect("import into b");

    assert_eq!(a.get_deep_value(), b.get_deep_value());
    let page = read_page(&a);
    assert_eq!(
        blocks(&page).len(),
        1,
        "only one side's block list survives"
    );
}
