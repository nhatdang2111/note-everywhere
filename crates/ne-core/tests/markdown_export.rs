//! Markdown export: golden files for every block kind and edge cases of text
//! escaping.

mod page_support;

use ne_core::markdown::{StandaloneEnv, export_blocks};
use ne_core::page::{Block, BlockKind, Inline, Marks};
use page_support::{assert_golden, bold, every_kind, item, link, list_item, run};

fn export(blocks: &[Block]) -> String {
    export_blocks(blocks, &mut StandaloneEnv)
}

fn code() -> Marks {
    Marks {
        code: true,
        ..Marks::default()
    }
}

fn strike() -> Marks {
    Marks {
        strike: true,
        ..Marks::default()
    }
}

fn paragraph(inline: Vec<Inline>) -> Block {
    Block::new(BlockKind::Paragraph).with_inline(inline)
}

#[test]
fn golden_every_block_kind_in_one_page() {
    assert_golden("every-block", &export(&every_kind()));
}

#[test]
fn golden_per_block_kind() {
    let kinds = every_kind();
    let names = [
        ("heading-1", 0),
        ("heading-2", 1),
        ("heading-3", 2),
        ("paragraph-marks", 3),
        ("bullet-list-nested", 4),
        ("ordered-list", 5),
        ("todo-list", 6),
        ("toggle", 7),
        ("quote-with-list", 8),
        ("callout", 9),
        ("divider", 10),
        ("code-block", 11),
        ("image", 12),
        ("child-page", 13),
        ("page-link", 14),
    ];
    for (name, index) in names {
        assert_golden(name, &export(&kinds[index..=index]));
    }
}

#[test]
fn marks_nest_and_keep_whitespace_outside_emphasis() {
    let text = export(&[paragraph(vec![
        run("alpha ", bold()),
        run(
            "beta",
            Marks {
                bold: true,
                italic: true,
                ..Marks::default()
            },
        ),
        run(" gamma", bold()),
        Inline::text(" "),
        run("del", strike()),
        Inline::text(" "),
        run(
            "x",
            Marks {
                bold: true,
                link: Some("https://e.org/a b".into()),
                ..Marks::default()
            },
        ),
    ])]);
    assert_eq!(
        text,
        "**alpha** ***beta*** **gamma** ~~del~~ [**x**](<https://e.org/a b>)\n"
    );
}

#[test]
fn special_characters_are_escaped() {
    let cases = [
        ("a*b*c", "a\\*b\\*c"),
        ("snake_case and _lead_", "snake_case and \\_lead\\_"),
        ("# not a heading", "\\# not a heading"),
        ("- not a list", "\\- not a list"),
        ("1. not a list", "1\\. not a list"),
        ("2) not a list", "2\\) not a list"),
        ("> not a quote", "\\> not a quote"),
        ("[x](y) <b> `c` ~d~", "\\[x\\](y) \\<b> \\`c\\` \\~d\\~"),
        ("a\\b", "a\\\\b"),
        ("AT&T &amp; &#35;", "AT&T \\&amp; \\&#35;"),
        ("3 apples", "3 apples"),
        ("C# and F#", "C# and F#"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            export(&[Block::paragraph(input)]),
            format!("{expected}\n"),
            "{input}"
        );
    }
    // `#` also trails headings, where it could be read as a closing sequence.
    assert_eq!(export(&[Block::heading(2, "C#")]), "## C\\#\n");
}

#[test]
fn escaping_applies_after_a_hard_break() {
    let text = export(&[paragraph(vec![
        Inline::text("first"),
        Inline::Break,
        Inline::text("# second"),
        Inline::Break,
        Inline::text("==="),
    ])]);
    assert_eq!(text, "first\\\n\\# second\\\n\\===\n");
}

#[test]
fn bang_before_a_link_cannot_become_an_image() {
    let text = export(&[paragraph(vec![
        Inline::text("Wow!"),
        run("link", link("https://e.org")),
    ])]);
    assert_eq!(text, "Wow\\![link](https://e.org)\n");
}

#[test]
fn code_spans_and_fences_grow_around_backticks() {
    let spans = export(&[paragraph(vec![
        run("a`b", code()),
        Inline::text(" "),
        run("`edge`", code()),
        Inline::text(" "),
        run(" pad ", code()),
    ])]);
    assert_eq!(spans, "``a`b`` `` `edge` `` `  pad  `\n");

    let fenced = export(&[Block::text(
        BlockKind::CodeBlock {
            language: "md".into(),
        },
        "```rust\nlet x = 1;\n```",
    )]);
    assert_eq!(fenced, "````md\n```rust\nlet x = 1;\n```\n````\n");

    let empty = export(&[Block::new(BlockKind::CodeBlock {
        language: String::new(),
    })]);
    assert_eq!(empty, "```\n```\n");
}

#[test]
fn adjacent_lists_use_different_markers() {
    let bullet = |text: &str| item(BlockKind::BulletList, vec![list_item(text, vec![])]);
    let ordered = |text: &str| item(BlockKind::OrderedList, vec![list_item(text, vec![])]);
    assert_eq!(
        export(&[bullet("a"), bullet("b"), bullet("c")]),
        "- a\n\n* b\n\n- c\n"
    );
    assert_eq!(export(&[ordered("a"), ordered("b")]), "1. a\n\n1) b\n");
}

#[test]
fn list_items_with_extra_blocks_indent_to_the_text_column() {
    let list = item(
        BlockKind::OrderedList,
        vec![item(
            BlockKind::ListItem,
            vec![
                Block::paragraph("one"),
                Block::text(
                    BlockKind::CodeBlock {
                        language: "sh".into(),
                    },
                    "ls\n\nls -l",
                ),
                Block::paragraph("more"),
            ],
        )],
    );
    assert_eq!(
        export(&[list]),
        "1. one\n\n   ```sh\n   ls\n\n   ls -l\n   ```\n\n   more\n"
    );
}

#[test]
fn empty_paragraphs_are_dropped_and_empty_items_stay() {
    let list = item(
        BlockKind::BulletList,
        vec![item(
            BlockKind::ListItem,
            vec![Block::new(BlockKind::Paragraph)],
        )],
    );
    assert_eq!(
        export(&[
            Block::new(BlockKind::Paragraph),
            Block::paragraph("x"),
            list
        ]),
        "x\n\n-\n"
    );
    assert_eq!(export(&[Block::new(BlockKind::Paragraph)]), "");
}

#[test]
fn page_links_fall_back_to_the_page_scheme_without_a_folder() {
    let text = export(&[Block::new(BlockKind::PageLink {
        page_id: "7@42".into(),
        title: String::new(),
    })]);
    assert_eq!(text, "[Untitled](ne-page:7@42)\n");
}
