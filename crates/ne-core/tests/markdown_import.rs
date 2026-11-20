//! Markdown import: block mapping, graceful degradation of unknown constructs
//! and the callout and toggle conventions.

mod page_support;

use ne_core::markdown::{StandaloneImport, import_blocks, write_imported};
use ne_core::page::{Block, BlockKind, Inline, PageDoc};
use page_support::replica;

fn import(markdown: &str) -> Vec<Block> {
    import_blocks(markdown, &mut StandaloneImport).blocks
}

fn kinds(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Heading { level } => format!("h{level}"),
            other => other.node_name().to_owned(),
        })
        .collect()
}

#[test]
fn headings_paragraphs_and_marks() {
    let imported = import_blocks(
        "# Tiêu đề\n\nXin **chào** *thế* ~~giới~~ `code` [link](https://e.org).\n\n### Mục",
        &mut StandaloneImport,
    );
    assert_eq!(imported.title.as_deref(), Some("Tiêu đề"));
    assert_eq!(kinds(&imported.blocks), ["paragraph", "h3"]);
    let Inline::Text(bold) = &imported.blocks[0].inline[1] else {
        panic!("text run");
    };
    assert!(bold.marks.bold && bold.text == "chào");
    let last = imported.blocks[0].inline.last();
    assert!(matches!(last, Some(Inline::Text(r)) if r.text == "."));
    assert!(imported.blocks[0].inline.iter().any(|i| matches!(
        i,
        Inline::Text(r) if r.marks.link.as_deref() == Some("https://e.org") && r.text == "link"
    )));
}

#[test]
fn only_a_leading_level_one_heading_is_the_title() {
    let imported = import_blocks("Intro\n\n# Later\n", &mut StandaloneImport);
    assert_eq!(imported.title, None);
    assert_eq!(kinds(&imported.blocks), ["paragraph", "h1"]);
    let imported = import_blocks("# One\n\n# Two\n", &mut StandaloneImport);
    assert_eq!(imported.title.as_deref(), Some("One"));
    assert_eq!(kinds(&imported.blocks), ["h1"]);
}

#[test]
fn lists_nest_and_split_by_kind() {
    let blocks =
        import("- a\n  - b\n    1. deep\n- [ ] open\n- [x] done\n- plain\n\n1. one\n2. two\n");
    assert_eq!(
        kinds(&blocks),
        ["bullet_list", "todo_list", "bullet_list", "ordered_list"]
    );
    // First list: item "a" holds a paragraph and a nested bullet list whose item holds an ordered list.
    let first = &blocks[0].children[0];
    assert_eq!(first.kind, BlockKind::ListItem);
    assert_eq!(kinds(&first.children), ["paragraph", "bullet_list"]);
    let nested_item = &first.children[1].children[0];
    assert_eq!(kinds(&nested_item.children), ["paragraph", "ordered_list"]);
    assert_eq!(
        blocks[1]
            .children
            .iter()
            .map(|i| i.kind.clone())
            .collect::<Vec<_>>(),
        [
            BlockKind::TodoItem { checked: false },
            BlockKind::TodoItem { checked: true }
        ]
    );
    assert_eq!(blocks[2].children[0].plain_text(), "plain");
}

#[test]
fn quote_is_a_quote_and_never_a_toggle() {
    let blocks = import("> one\n> two\n\n> > nested\n\n> ▸ Toggle-looking text\n> hidden?");
    assert_eq!(kinds(&blocks), ["quote", "quote", "quote"]);
    assert_eq!(blocks[0].plain_text(), "one two");
    assert_eq!(kinds(&blocks[1].children), ["quote"]);
    assert_eq!(blocks[2].plain_text(), "▸ Toggle-looking text hidden?");
}

#[test]
fn callout_and_toggle_markers() {
    let blocks = import(
        "> [!callout 💡]\n>\n> Body one\n>\n> - item\n\n\
         > [!callout]\n>\n> No icon\n\n\
         > [!toggle open] Title with **bold**\n>\n> Hidden paragraph\n\n\
         > [!TOGGLE]\n>\n> Empty summary\n",
    );
    assert_eq!(kinds(&blocks), ["callout", "callout", "toggle", "toggle"]);
    assert_eq!(
        blocks[0].kind,
        BlockKind::Callout {
            icon: "💡".into()
        }
    );
    assert_eq!(kinds(&blocks[0].children), ["paragraph", "bullet_list"]);
    assert_eq!(
        blocks[1].kind,
        BlockKind::Callout {
            icon: String::new()
        }
    );
    assert_eq!(blocks[2].kind, BlockKind::Toggle { open: true });
    assert_eq!(blocks[2].children[0].plain_text(), "Title with bold");
    assert!(matches!(&blocks[2].children[0].inline[1], Inline::Text(r) if r.marks.bold));
    assert_eq!(blocks[2].children[1].plain_text(), "Hidden paragraph");
    assert_eq!(blocks[3].kind, BlockKind::Toggle { open: false });
    assert_eq!(
        blocks[3].children[0].plain_text(),
        "",
        "empty summary paragraph"
    );
}

#[test]
fn github_alerts_become_callouts() {
    let blocks = import("> [!WARNING]\n> Careful here.\n");
    assert_eq!(kinds(&blocks), ["callout"]);
    assert!(matches!(&blocks[0].kind, BlockKind::Callout { icon } if icon.contains('\u{26a0}')));
    assert_eq!(blocks[0].plain_text(), "Careful here.");
}

#[test]
fn unknown_markers_stay_quotes_with_their_text() {
    let blocks = import("> [!mystery] hello\n");
    assert_eq!(kinds(&blocks), ["quote"]);
    assert_eq!(blocks[0].plain_text(), "[!mystery] hello");
}

#[test]
fn code_blocks_keep_language_and_content() {
    let blocks = import(
        "```rust title=\"x\"\nfn a() {}\n\n```\n\n~~~\nplain\n~~~\n\n    indented\n    code\n",
    );
    assert_eq!(
        blocks[0].kind,
        BlockKind::CodeBlock {
            language: "rust".into()
        }
    );
    assert_eq!(blocks[0].plain_text(), "fn a() {}\n");
    assert_eq!(blocks[1].plain_text(), "plain");
    assert_eq!(blocks[2].plain_text(), "indented\ncode");
    assert!(
        import("```\n```")
            .first()
            .is_some_and(|b| b.inline.is_empty())
    );
}

#[test]
fn images_are_blocks_and_split_their_paragraph() {
    let blocks = import("Before ![Sơ đồ](<a b.png>) after\n\n- ![in list](x.png)\n");
    assert_eq!(
        kinds(&blocks),
        ["paragraph", "image", "paragraph", "bullet_list"]
    );
    assert_eq!(blocks[0].plain_text(), "Before ");
    assert_eq!(
        blocks[1].kind,
        BlockKind::Image {
            src: "a b.png".into(),
            alt: "Sơ đồ".into()
        }
    );
    assert_eq!(blocks[2].plain_text(), " after");
    let item = &blocks[3].children[0];
    assert_eq!(
        kinds(&item.children),
        ["paragraph", "image"],
        "a list item still starts with a paragraph"
    );
}

#[test]
fn hard_breaks_soft_breaks_and_entities() {
    let blocks = import("a  \nb\\\nc\nd &amp; &copy; &");
    assert_eq!(blocks[0].plain_text(), "a\nb\nc d & © &");
    assert_eq!(import("a<br>b<br/>c")[0].plain_text(), "a\nb\nc");
}

#[test]
fn unknown_constructs_degrade_without_losing_text() {
    let blocks = import(
        "---\ntitle: T\n---\n\n<div>\nRaw <b>HTML</b>\n</div>\n\n<!-- hidden -->\n\n\
         | A | B |\n|---|--:|\n| 1 | 2 |\n\n##### Deep\n\nText[^1]\n\n[^1]: Note\n",
    );
    assert_eq!(
        kinds(&blocks),
        [
            "code_block",
            "paragraph",
            "paragraph",
            "paragraph",
            "h3",
            "paragraph",
            "paragraph"
        ]
    );
    assert_eq!(
        blocks[0].kind,
        BlockKind::CodeBlock {
            language: "yaml".into()
        }
    );
    assert_eq!(blocks[0].plain_text(), "title: T");
    assert_eq!(blocks[1].plain_text(), "<div>\nRaw <b>HTML</b>\n</div>");
    assert_eq!(blocks[2].plain_text(), "A | B");
    assert_eq!(blocks[3].plain_text(), "1 | 2");
    assert_eq!(blocks[4].plain_text(), "Deep");
    assert_eq!(blocks[5].plain_text(), "Text[^1]");
    assert_eq!(blocks[6].plain_text(), "[^1]: Note");
}

#[test]
fn empty_and_blank_input_give_no_blocks() {
    assert!(import("").is_empty());
    assert!(import("\n\n   \n").is_empty());
    assert!(import("<!-- only a comment -->").is_empty());
}

#[test]
fn crlf_and_byte_order_mark() {
    let blocks = import("\u{feff}# T\r\n\r\nline\r\nnext\r\n\r\n- a\r\n- b\r\n");
    assert_eq!(kinds(&blocks), ["paragraph", "bullet_list"]);
    assert_eq!(blocks[0].plain_text(), "line next");
}

#[test]
fn imported_blocks_write_into_a_page_in_the_schema() {
    let doc = replica(1);
    let blocks = import(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/messy.md"),
        )
        .expect("fixture"),
    );
    write_imported(&doc, &blocks).expect("imported blocks are schema-valid");
    assert_eq!(doc.blocks().expect("read"), blocks);

    // An empty import leaves the skeleton paragraph.
    let empty = PageDoc::new().expect("page");
    write_imported(&empty, &[]).expect("empty import");
    assert_eq!(empty.blocks().expect("read").len(), 1);
}

#[test]
fn messy_document_keeps_all_its_text() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/messy.md"),
    )
    .expect("fixture");
    let imported = import_blocks(&source, &mut StandaloneImport);
    let text = ne_core::page::page_plain_text(&imported.blocks);
    for fragment in [
        "title: Ghi chú họp tuần",
        "Ghi chú họp tuần",
        "Intro paragraph with emphasis, strong, inline code, struck text and a soft-wrapped line.",
        "force a break here\nand the next line follows.",
        "https://example.org/docs",
        "the site",
        "Mục lớn",
        "Heading four becomes level three",
        "Nested with four spaces",
        "Plus bullet after a marker change",
        "Numbered starting at three",
        "Next number continued lazily",
        "Open task",
        "Nested task",
        "Plain item in a task list",
        "A quote with bold and a second line.",
        "Nested quote",
        "A GitHub style alert.",
        "Raw <b>HTML</b> block",
        "Tên | Số lượng",
        "Táo | 3",
        "Cam | 12",
        "Footnote style text[^1] stays as it was written.",
        "[^1]: The footnote body stays too.",
        "def chào(tên):",
        "tilde fence",
        "line two after a backslash break",
        "&",
        "© and a lone &.",
        "Ends with an unfinished *emphasis and a [broken link(",
    ] {
        assert!(text.contains(fragment), "missing {fragment:?} in:\n{text}");
    }
    assert!(imported.blocks.iter().any(|b| matches!(
        &b.kind,
        BlockKind::Image { src, alt } if src == "images/so-do.png" && alt == "Sơ đồ"
    )));
    assert!(!text.contains("a comment that should vanish"));
}
