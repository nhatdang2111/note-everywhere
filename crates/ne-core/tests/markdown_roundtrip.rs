//! export -> import -> export is stable, and callout and toggle survive it
//! with their attributes and nested content.

mod page_support;

use ne_core::markdown::{StandaloneEnv, StandaloneImport, export_blocks, import_blocks};
use ne_core::page::{Block, BlockKind, Inline, Marks};
use page_support::{bold, every_kind, item, link, list_item, run};

fn export(blocks: &[Block]) -> String {
    export_blocks(blocks, &mut StandaloneEnv)
}

fn import(markdown: &str) -> Vec<Block> {
    import_blocks(markdown, &mut StandaloneImport).blocks
}

fn assert_stable(blocks: &[Block]) -> String {
    let first = export(blocks);
    let second = export(&import(&first));
    assert_eq!(first, second, "export is not stable");
    first
}

#[test]
fn every_block_kind_round_trips_through_markdown() {
    let original = every_kind();
    let markdown = export(&original);
    let imported = import_blocks(&markdown, &mut StandaloneImport);
    assert_eq!(imported.title.as_deref(), Some("Tiêu đề một"));
    // The title heading was taken out; putting it back reproduces the export.
    let mut again = vec![Block::heading(1, "Tiêu đề một")];
    again.extend(imported.blocks.clone());
    assert_eq!(export(&again), markdown, "export is not stable");

    // Everything but the title heading comes back as it was, except that a
    // standalone page reference carries no hint of being a child page.
    let mut expected: Vec<Block> = original[1..].to_vec();
    for block in &mut expected {
        if let BlockKind::ChildPage { page_id, title } = &block.kind {
            block.kind = BlockKind::PageLink {
                page_id: page_id.clone(),
                title: title.clone(),
            };
        }
    }
    assert_eq!(imported.blocks, expected);
}

fn quote_of(children: Vec<Block>) -> Block {
    item(BlockKind::Quote, children)
}

fn toggle(open: bool, children: Vec<Block>) -> Block {
    item(BlockKind::Toggle { open }, children)
}

fn callout(icon: &str, children: Vec<Block>) -> Block {
    item(BlockKind::Callout { icon: icon.into() }, children)
}

#[test]
fn toggle_round_trips_losslessly() {
    let cases = vec![
        toggle(true, vec![Block::paragraph("Open summary")]),
        toggle(
            false,
            vec![Block::paragraph("Closed"), Block::paragraph("Body")],
        ),
        // Marks and a soft line break in the summary.
        toggle(
            true,
            vec![
                Block::new(BlockKind::Paragraph).with_inline(vec![
                    Inline::text("Plain "),
                    run("bold", bold()),
                    Inline::Break,
                    run("link", link("https://e.org")),
                ]),
                Block::paragraph("Body"),
            ],
        ),
        // A summary that starts with marked text.
        toggle(
            false,
            vec![Block::new(BlockKind::Paragraph).with_inline(vec![run("bold first", bold())])],
        ),
        // An empty summary with a body.
        toggle(
            false,
            vec![
                Block::new(BlockKind::Paragraph),
                Block::paragraph("Body only"),
            ],
        ),
        // Rich body: list, code, nested toggle and callout.
        toggle(
            true,
            vec![
                Block::paragraph("Rich"),
                item(BlockKind::BulletList, vec![list_item("one", vec![])]),
                Block::text(
                    BlockKind::CodeBlock {
                        language: "rs".into(),
                    },
                    "let a = 1;",
                ),
                toggle(
                    false,
                    vec![Block::paragraph("Inner"), Block::paragraph("Inner body")],
                ),
                callout("⚠️", vec![Block::paragraph("Careful")]),
            ],
        ),
    ];
    for case in cases {
        let markdown = assert_stable(std::slice::from_ref(&case));
        assert_eq!(
            import(&markdown),
            std::slice::from_ref(&case),
            "lost data in:\n{markdown}"
        );
    }
}

#[test]
fn callout_round_trips_losslessly() {
    let cases = vec![
        callout("💡", vec![Block::paragraph("Idea")]),
        callout("", vec![Block::paragraph("No icon")]),
        callout(
            "👨‍👩‍👧",
            vec![
                Block::paragraph("Joined emoji"),
                Block::paragraph("Two paragraphs"),
            ],
        ),
        callout(
            "📌",
            vec![
                Block::heading(2, "Heading inside"),
                item(
                    BlockKind::TodoList,
                    vec![item(
                        BlockKind::TodoItem { checked: true },
                        vec![Block::paragraph("done")],
                    )],
                ),
                quote_of(vec![Block::paragraph("quoted")]),
                Block::new(BlockKind::Divider),
            ],
        ),
        // Containers inside containers.
        quote_of(vec![
            Block::paragraph("Outer"),
            callout("💡", vec![Block::paragraph("Inner callout")]),
            toggle(
                true,
                vec![Block::paragraph("Inner toggle"), Block::paragraph("Body")],
            ),
        ]),
        callout(
            "🔥",
            vec![toggle(
                false,
                vec![Block::paragraph("T"), Block::paragraph("B")],
            )],
        ),
    ];
    for case in cases {
        let markdown = assert_stable(std::slice::from_ref(&case));
        assert_eq!(
            import(&markdown),
            std::slice::from_ref(&case),
            "lost data in:\n{markdown}"
        );
    }
}

#[test]
fn callouts_and_toggles_inside_lists_round_trip() {
    let list = item(
        BlockKind::BulletList,
        vec![list_item(
            "item",
            vec![
                callout("💡", vec![Block::paragraph("in item")]),
                toggle(
                    true,
                    vec![Block::paragraph("sum"), Block::paragraph("body")],
                ),
            ],
        )],
    );
    let markdown = assert_stable(std::slice::from_ref(&list));
    assert_eq!(import(&markdown), [list]);
}

#[test]
fn plain_quotes_stay_quotes() {
    let cases = vec![
        quote_of(vec![Block::paragraph("one"), Block::paragraph("two")]),
        quote_of(vec![quote_of(vec![Block::paragraph("nested")])]),
        // Text that merely resembles a marker is escaped on export.
        quote_of(vec![Block::paragraph("[!callout] not a marker")]),
        quote_of(vec![Block::paragraph("[!toggle open] not a marker")]),
    ];
    for case in cases {
        let markdown = assert_stable(std::slice::from_ref(&case));
        assert_eq!(
            import(&markdown),
            std::slice::from_ref(&case),
            "in:\n{markdown}"
        );
    }
}

#[test]
fn adjacent_lists_of_one_kind_stay_separate() {
    let bullet = |text: &str| item(BlockKind::BulletList, vec![list_item(text, vec![])]);
    let todo = |text: &str| {
        item(
            BlockKind::TodoList,
            vec![item(
                BlockKind::TodoItem { checked: false },
                vec![Block::paragraph(text)],
            )],
        )
    };
    let ordered = |text: &str| item(BlockKind::OrderedList, vec![list_item(text, vec![])]);
    let blocks = vec![
        bullet("a"),
        bullet("b"),
        todo("c"),
        todo("d"),
        bullet("e"),
        ordered("f"),
        ordered("g"),
    ];
    let markdown = assert_stable(&blocks);
    assert_eq!(import(&markdown), blocks);
}

#[test]
fn text_with_markdown_syntax_round_trips() {
    let texts = [
        "plain",
        "*stars* and _underscores_ and `ticks` and ~tildes~",
        "# hash at start",
        "1. number at start",
        "- dash at start",
        "+ plus at start",
        "> angle at start",
        "[bracket](not-a-link) and ![not image](x)",
        "<b>not html</b> &amp; &lt;",
        "back\\slash and trailing backslash\\",
        "snake_case_name and 2*3*4",
        "emoji 😀 and tiếng Việt: ghi chú, đường",
        "a | b | c",
        "=== and --- inline",
    ];
    for text in texts {
        let block = Block::paragraph(text);
        let markdown = assert_stable(std::slice::from_ref(&block));
        assert_eq!(
            import(&markdown),
            [block],
            "text {text:?} came back changed from:\n{markdown}"
        );
    }
}

#[test]
fn marks_round_trip_in_combination() {
    let inline = vec![
        run("one", bold()),
        Inline::text(" "),
        run(
            "two",
            Marks {
                italic: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run(
            "three",
            Marks {
                bold: true,
                italic: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run(
            "four",
            Marks {
                strike: true,
                bold: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run(
            "five",
            Marks {
                code: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run(
            "six",
            Marks {
                link: Some("https://e.org/a(b)".into()),
                bold: true,
                ..Marks::default()
            },
        ),
        Inline::text(" "),
        run(
            "seven",
            Marks {
                link: Some("rel/path with space.md".into()),
                code: true,
                ..Marks::default()
            },
        ),
    ];
    let block = Block::new(BlockKind::Paragraph).with_inline(inline);
    let markdown = assert_stable(std::slice::from_ref(&block));
    assert_eq!(import(&markdown), [block], "from:\n{markdown}");
}

#[test]
fn messy_fixture_reaches_a_fixed_point() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/messy.md"),
    )
    .expect("fixture");
    let blocks = import(&source);
    let once = export(&blocks);
    let twice = export(&import(&once));
    assert_eq!(once, twice);
    // The model also survives: importing the export gives the same blocks.
    assert_eq!(import(&once), blocks);
}
