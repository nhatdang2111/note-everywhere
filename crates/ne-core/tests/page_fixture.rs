//! `PageDoc` against the documents produced by the real editor binding
//! (`apps/desktop/spikes/page-doc-fixture.mjs`), the proof that this crate and
//! the webview editor agree on the container layout.

use ne_core::page::{Block, BlockKind, Inline, PageDoc};

const SNAPSHOT: &[u8] = include_bytes!("fixtures/page-doc-snapshot.loro");
const UPDATE: &[u8] = include_bytes!("fixtures/page-doc-update.loro");

fn load(with_update: bool) -> PageDoc {
    let doc = PageDoc::from_snapshot(SNAPSHOT).expect("load snapshot");
    if with_update {
        doc.import(UPDATE).expect("import update");
    }
    doc
}

fn kind_names(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|b| b.kind.node_name()).collect()
}

#[test]
fn editor_fixture_reads_with_every_block_kind_in_order() {
    let blocks = load(false).blocks().expect("read fixture");
    assert_eq!(
        kind_names(&blocks),
        [
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
        ]
    );
    let levels: Vec<_> = blocks[..3].iter().map(|b| b.kind.clone()).collect();
    assert_eq!(
        levels,
        [
            BlockKind::Heading { level: 1 },
            BlockKind::Heading { level: 2 },
            BlockKind::Heading { level: 3 },
        ]
    );
}

#[test]
fn editor_fixture_attributes_and_text() {
    let blocks = load(false).blocks().expect("read fixture");
    let texts: Vec<String> = blocks.iter().map(Block::plain_text).collect();
    assert_eq!(texts[0], "Tiêu đề trang");
    assert_eq!(
        texts[3],
        "Xin chào, thế giới and English code() bỏ đi liên kết đậm nghiêng"
    );
    assert_eq!(texts[4], "Dòng một\nDòng hai");
    assert_eq!(texts[5], "Mục một\nMục con ở trong\nMục hai");
    assert_eq!(texts[7], "Đã xong\nChưa xong");
    assert_eq!(texts[8], "Toggle tóm tắt\nNội dung ẩn bên trong");
    assert_eq!(texts[12], "fn main() {\n    println!(\"Xin chào\");\n}");

    assert_eq!(blocks[8].kind, BlockKind::Toggle { open: false });
    assert_eq!(
        blocks[10].kind,
        BlockKind::Callout {
            icon: "💡".into()
        }
    );
    assert_eq!(
        blocks[12].kind,
        BlockKind::CodeBlock {
            language: "rust".into()
        }
    );
    assert_eq!(
        blocks[13].kind,
        BlockKind::Image {
            src: "assets/so-do.png".into(),
            alt: "Sơ đồ kiến trúc".into()
        }
    );
    assert_eq!(
        blocks[14].kind,
        BlockKind::ChildPage {
            page_id: "page-child-0001".into(),
            title: "Trang con".into()
        }
    );
    assert!(matches!(
        &blocks[15].kind,
        BlockKind::PageLink { page_id, .. } if page_id == "page-target-0002"
    ));
}

#[test]
fn editor_fixture_marks() {
    let blocks = load(false).blocks().expect("read fixture");
    let runs: Vec<(String, bool, bool, bool, bool, Option<String>)> = blocks[3]
        .inline
        .iter()
        .map(|item| match item {
            Inline::Text(run) => (
                run.text.clone(),
                run.marks.bold,
                run.marks.italic,
                run.marks.code,
                run.marks.strike,
                run.marks.link.clone(),
            ),
            Inline::Break => panic!("no break in this paragraph"),
        })
        .collect();
    let find = |text: &str| {
        runs.iter()
            .find(|r| r.0 == text)
            .expect("run exists")
            .clone()
    };
    assert!(find("thế giới").1);
    assert!(find("English").2);
    assert!(find("code()").3);
    assert!(find("bỏ đi").4);
    assert_eq!(
        find("liên kết").5.as_deref(),
        Some("https://example.org/vi")
    );
    let both = find("đậm nghiêng");
    assert!(both.1 && both.2);
    assert!(
        blocks[4].inline.contains(&Inline::Break),
        "hard_break is an inline break"
    );
}

#[test]
fn editor_update_applies_on_top_of_the_snapshot() {
    let blocks = load(true).blocks().expect("read fixture");
    assert_eq!(blocks[0].plain_text(), "Tiêu đề trang (đã sửa)");
    let flags: Vec<_> = blocks[7].children.iter().map(|i| i.kind.clone()).collect();
    assert_eq!(
        flags,
        [
            BlockKind::TodoItem { checked: true },
            BlockKind::TodoItem { checked: true }
        ]
    );
}

#[test]
fn native_edits_on_the_editor_document_export_an_update_the_editor_can_read() {
    let doc = load(true);
    let before = doc.blocks().expect("read");
    let (_, update) = doc
        .edit(|d| {
            d.insert_text(&[5, 1, 0], "Mục hai".chars().count(), " (sửa từ Rust)")?;
            d.append_block(&[], &Block::paragraph("Thêm từ Rust"))?;
            Ok(())
        })
        .expect("edit");

    // A replica that only has the editor's history accepts the update.
    let other = load(true);
    other.import(&update).expect("import");
    let after = other.blocks().expect("read");
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        after[5].plain_text(),
        "Mục một\nMục con ở trong\nMục hai (sửa từ Rust)"
    );
    assert_eq!(after.last().expect("last").plain_text(), "Thêm từ Rust");
    assert_eq!(after, doc.blocks().expect("read"));
}
