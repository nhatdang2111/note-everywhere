//! Shared helpers for the `page_*` and `markdown_*` integration tests.
//! Included with `mod page_support;` from each test file.
#![allow(dead_code)]

use ne_core::NodeId;
use ne_core::page::{Block, BlockKind, Inline, Marks, PageDoc, Run, page_href};

/// A replica with a fixed peer id.
pub fn replica(peer: u64) -> PageDoc {
    PageDoc::with_peer_id(peer).expect("valid peer id")
}

/// Applies everything `from` has that `to` lacks.
pub fn pull(to: &PageDoc, from: &PageDoc) {
    let bytes = from
        .export_updates_since(&to.version())
        .expect("export updates");
    let outcome = to.import(&bytes).expect("import updates");
    assert!(!outcome.has_missing_dependencies, "update had a gap");
}

/// Brings every replica to the union of all histories.
pub fn sync_all(replicas: &[&PageDoc]) {
    for _ in 0..2 {
        for (i, to) in replicas.iter().enumerate() {
            for (j, from) in replicas.iter().enumerate() {
                if i != j {
                    pull(to, from);
                }
            }
        }
    }
}

pub fn node_id(text: &str) -> NodeId {
    text.parse().expect("valid node id")
}

pub fn run(text: &str, marks: Marks) -> Inline {
    Inline::Text(Run {
        text: text.into(),
        marks,
    })
}

pub fn bold() -> Marks {
    Marks {
        bold: true,
        ..Marks::default()
    }
}

pub fn link(href: &str) -> Marks {
    Marks {
        link: Some(href.into()),
        ..Marks::default()
    }
}

pub fn item(kind: BlockKind, children: Vec<Block>) -> Block {
    Block::new(kind).with_children(children)
}

pub fn list_item(text: &str, nested: Vec<Block>) -> Block {
    let mut children = vec![Block::paragraph(text)];
    children.extend(nested);
    item(BlockKind::ListItem, children)
}

/// One block (with nesting) of every kind the page schema offers.
pub fn every_kind() -> Vec<Block> {
    let page_id = node_id("7@42");
    let rich = Block::new(BlockKind::Paragraph).with_inline(vec![
        Inline::text("Xin chào, "),
        run("đậm", bold()),
        Inline::text(" và "),
        run(
            "nghiêng",
            Marks {
                italic: true,
                ..Marks::default()
            },
        ),
        Inline::Break,
        run(
            "code()",
            Marks {
                code: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run(
            "gạch",
            Marks {
                strike: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run("liên kết", link("https://example.org/vi")),
        Inline::text(" "),
        run(
            "cả hai",
            Marks {
                bold: true,
                italic: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run("trang khác", link(&page_href(page_id))),
    ]);
    vec![
        Block::heading(1, "Tiêu đề một"),
        Block::heading(2, "Mục hai"),
        Block::heading(3, "Mục ba"),
        rich,
        item(
            BlockKind::BulletList,
            vec![
                list_item(
                    "Mục một",
                    vec![item(
                        BlockKind::BulletList,
                        vec![list_item("Mục con", vec![])],
                    )],
                ),
                list_item("Mục hai", vec![]),
            ],
        ),
        item(
            BlockKind::OrderedList,
            vec![list_item("Bước một", vec![]), list_item("Step two", vec![])],
        ),
        item(
            BlockKind::TodoList,
            vec![
                item(
                    BlockKind::TodoItem { checked: true },
                    vec![Block::paragraph("Đã xong")],
                ),
                item(
                    BlockKind::TodoItem { checked: false },
                    vec![Block::paragraph("Chưa xong")],
                ),
            ],
        ),
        item(
            BlockKind::Toggle { open: false },
            vec![Block::paragraph("Tóm tắt"), Block::paragraph("Nội dung ẩn")],
        ),
        item(
            BlockKind::Quote,
            vec![
                Block::paragraph("Có công mài sắt"),
                item(
                    BlockKind::BulletList,
                    vec![list_item("trong trích dẫn", vec![])],
                ),
            ],
        ),
        item(
            BlockKind::Callout {
                icon: "💡".into()
            },
            vec![Block::paragraph("Lưu ý quan trọng")],
        ),
        Block::new(BlockKind::Divider),
        Block::text(
            BlockKind::CodeBlock {
                language: "rust".into(),
            },
            "fn main() {\n    println!(\"Xin chào\");\n}",
        ),
        Block::new(BlockKind::Image {
            src: "assets/so-do.png".into(),
            alt: "Sơ đồ".into(),
        }),
        Block::new(BlockKind::ChildPage {
            page_id: "3@42".into(),
            title: "Trang con".into(),
        }),
        Block::new(BlockKind::PageLink {
            page_id: page_id.to_string(),
            title: "Trang đích".into(),
        }),
    ]
}

/// Writes `blocks` as the whole page body, replacing the skeleton paragraph.
pub fn fill(doc: &PageDoc, blocks: &[Block]) {
    for block in blocks {
        doc.append_block(&[], block).expect("append block");
    }
    doc.delete_block(&[0]).expect("remove skeleton paragraph");
}

/// Compares `actual` with `tests/golden/<name>.md`. Set `NE_UPDATE_GOLDEN=1`
/// to rewrite the file from the current output, then review the diff.
pub fn assert_golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.md"));
    if std::env::var_os("NE_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden file");
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("golden file exists");
    assert_eq!(actual, expected, "output differs from {name}.md");
}
