//! Write-then-read round trips, skeleton creation, edits and extraction.

mod page_support;

use ne_core::Error;
use ne_core::page::{
    Block, BlockKind, Inline, LinkKind, PageDoc, SKELETON_PEER_ID, attachment_src, page_href,
    parse_attachment_src, parse_page_href,
};
use page_support::{bold, every_kind, fill, item, list_item, node_id, pull, replica, run};

#[test]
fn new_page_holds_one_empty_paragraph() {
    let doc = PageDoc::new().expect("new page");
    assert_eq!(
        doc.blocks().expect("blocks"),
        [Block::new(BlockKind::Paragraph)]
    );
    assert_eq!(doc.plain_text().expect("text"), "");
    assert_ne!(doc.peer_id(), SKELETON_PEER_ID);
}

#[test]
fn skeleton_peer_id_is_reserved() {
    assert!(PageDoc::with_peer_id(SKELETON_PEER_ID).is_err());
    let doc = replica(5);
    assert!(doc.set_peer_id(SKELETON_PEER_ID).is_err());
}

#[test]
fn skeletons_created_independently_are_the_same_operations() {
    let a = replica(1);
    let b = replica(2);
    a.insert_text(&[0], 0, "from a").expect("insert");
    b.insert_text(&[0], 0, "from b").expect("insert");
    page_support::sync_all(&[&a, &b]);

    let blocks = a.blocks().expect("blocks");
    assert_eq!(blocks, b.blocks().expect("blocks"));
    assert_eq!(blocks.len(), 1, "one skeleton paragraph, not two roots");
    let text = blocks[0].plain_text();
    assert!(
        text == "from afrom b" || text == "from bfrom a",
        "both inserts survive in the one paragraph: {text:?}"
    );
}

#[test]
fn skeleton_update_is_identical_across_devices() {
    let a = replica(1).export_all_updates().expect("export a");
    let b = replica(2).export_all_updates().expect("export b");
    assert_eq!(a, b, "the skeleton must not depend on the device");
}

#[test]
fn every_block_kind_round_trips() {
    let doc = replica(1);
    let expected = every_kind();
    fill(&doc, &expected);
    assert_eq!(doc.blocks().expect("read"), expected);

    // The same content survives a snapshot, a reload and an update import.
    let reloaded = PageDoc::from_snapshot(&doc.export_snapshot().expect("snapshot")).expect("load");
    assert_eq!(reloaded.blocks().expect("read"), expected);
    let replica_2 = replica(2);
    replica_2
        .import(&doc.export_all_updates().expect("updates"))
        .expect("import");
    assert_eq!(replica_2.blocks().expect("read"), expected);
}

#[test]
fn shallow_snapshot_keeps_the_content() {
    let doc = replica(1);
    fill(&doc, &every_kind());
    let shallow = PageDoc::from_snapshot(&doc.export_shallow_snapshot().expect("shallow"))
        .expect("load shallow");
    assert_eq!(shallow.blocks().expect("read"), every_kind());
}

#[test]
fn block_reads_a_nested_block_by_path() {
    let doc = replica(1);
    fill(&doc, &every_kind());
    let nested = doc.block(&[4, 0, 1, 0, 0]).expect("nested paragraph");
    assert_eq!(nested.plain_text(), "Mục con");
    assert_eq!(
        doc.block(&[]).expect_err("empty path"),
        Error::NoSuchBlock(vec![])
    );
    assert_eq!(
        doc.block(&[99]).expect_err("missing block"),
        Error::NoSuchBlock(vec![99])
    );
}

#[test]
fn set_inline_replaces_text_and_marks() {
    let doc = replica(1);
    doc.set_inline(
        &[0],
        &[
            Inline::text("một "),
            run("hai", bold()),
            Inline::Break,
            Inline::text("ba"),
        ],
    )
    .expect("set inline");
    let block = doc.block(&[0]).expect("read");
    assert_eq!(block.plain_text(), "một hai\nba");
    assert_eq!(block.inline.len(), 4);
    doc.set_inline(&[0], &[]).expect("clear");
    assert!(doc.block(&[0]).expect("read").inline.is_empty());
}

#[test]
fn insert_text_counts_unicode_scalars_with_combining_marks() {
    // "u" + combining acute is two scalars; the precomposed form is one. Text
    // is stored as given, never normalised, and offsets count scalars.
    let decomposed = "Ghi chu\u{0301}: ne\u{0302}n";
    let precomposed = "Ghi chú: nên";
    for text in [decomposed, precomposed] {
        let doc = replica(1);
        doc.insert_text(&[0], 0, text).expect("insert");
        doc.insert_text(&[0], text.chars().count(), " đây")
            .expect("append");
        doc.insert_text(&[0], 3, "-")
            .expect("insert inside the first word");
        let expected = format!("Ghi-{} đây", text.chars().skip(3).collect::<String>());
        assert_eq!(doc.block(&[0]).expect("read").plain_text(), expected);
    }
    assert_eq!(decomposed.chars().count(), precomposed.chars().count() + 2);
}

#[test]
fn marks_land_on_the_right_characters_after_combining_text() {
    let doc = replica(1);
    let text = "a\u{0302}\u{0301} béo";
    doc.set_inline(&[0], &[Inline::text(text), run(" đậm", bold())])
        .expect("set inline");
    let block = doc.block(&[0]).expect("read");
    assert_eq!(block.inline.len(), 2);
    let Inline::Text(second) = &block.inline[1] else {
        panic!("second inline is text");
    };
    assert_eq!(second.text, " đậm");
    assert!(second.marks.bold);
}

#[test]
fn insert_text_positions_around_breaks() {
    let doc = replica(1);
    doc.set_inline(
        &[0],
        &[Inline::text("ab"), Inline::Break, Inline::text("cd")],
    )
    .expect("set inline");
    doc.insert_text(&[0], 3, "X").expect("after the break");
    doc.insert_text(&[0], 2, "Y").expect("at the break");
    assert_eq!(doc.block(&[0]).expect("read").plain_text(), "abY\nXcd");
    assert!(doc.insert_text(&[0], 50, "z").is_err());

    // A block that is only a break gets a fresh text container before it.
    doc.set_inline(&[0], &[Inline::Break]).expect("set inline");
    doc.insert_text(&[0], 0, "first").expect("insert");
    doc.insert_text(&[0], 6, "last").expect("insert");
    assert_eq!(doc.block(&[0]).expect("read").plain_text(), "first\nlast");
}

#[test]
fn insert_text_rejects_non_text_blocks() {
    let doc = replica(1);
    fill(&doc, &every_kind());
    let divider = [10];
    assert!(matches!(
        doc.insert_text(&divider, 0, "x"),
        Err(Error::InvalidBlock(_))
    ));
}

#[test]
fn set_attrs_changes_level_and_checked_state() {
    let doc = replica(1);
    fill(&doc, &every_kind());
    doc.set_attrs(&[0], &BlockKind::Heading { level: 3 })
        .expect("level");
    assert_eq!(
        doc.block(&[0]).expect("read").kind,
        BlockKind::Heading { level: 3 }
    );
    assert!(
        doc.set_attrs(&[0], &BlockKind::Heading { level: 4 })
            .is_err()
    );
    assert!(doc.set_attrs(&[0], &BlockKind::Paragraph).is_err());

    doc.set_todo_checked(&[6, 1], true).expect("tick");
    assert_eq!(
        doc.block(&[6, 1]).expect("read").kind,
        BlockKind::TodoItem { checked: true }
    );
    doc.set_todo_checked(&[6, 0], false).expect("untick");
    assert_eq!(
        doc.block(&[6, 0]).expect("read").kind,
        BlockKind::TodoItem { checked: false }
    );
    assert!(
        doc.set_todo_checked(&[0], true).is_err(),
        "a heading is not a to-do"
    );
}

#[test]
fn schema_violations_are_rejected_and_write_nothing() {
    let doc = replica(1);
    let before = doc.export_snapshot().expect("snapshot");
    let bad_cases = [
        // A list item outside a list.
        (
            vec![],
            Block::new(BlockKind::ListItem).with_children(vec![Block::paragraph("x")]),
        ),
        // A bullet list without items.
        (vec![], Block::new(BlockKind::BulletList)),
        // A to-do list holding list items.
        (
            vec![],
            item(BlockKind::TodoList, vec![list_item("x", vec![])]),
        ),
        // A list item that does not start with a paragraph.
        (
            vec![],
            item(
                BlockKind::BulletList,
                vec![item(
                    BlockKind::ListItem,
                    vec![Block::new(BlockKind::Divider)],
                )],
            ),
        ),
        // Heading level out of range.
        (vec![], Block::heading(4, "x")),
        // Children inside a divider.
        (
            vec![],
            Block::new(BlockKind::Divider).with_children(vec![Block::paragraph("x")]),
        ),
        // Inline content in a quote.
        (
            vec![],
            Block::new(BlockKind::Quote).with_inline(vec![Inline::text("x")]),
        ),
        // A block inside a paragraph.
        (vec![0], Block::paragraph("x")),
    ];
    for (parent, block) in bad_cases {
        let err = doc
            .insert_block(&parent, 0, &block)
            .expect_err("must be rejected");
        assert!(
            matches!(err, Error::InvalidBlock(_)),
            "{block:?} gave {err:?}"
        );
    }
    assert!(matches!(
        doc.insert_block(&[], 5, &Block::paragraph("x")),
        Err(Error::NoSuchBlock(_))
    ));
    assert_eq!(doc.export_snapshot().expect("snapshot"), before);
}

#[test]
fn delete_keeps_required_content() {
    let doc = replica(1);
    assert!(
        matches!(doc.delete_block(&[0]), Err(Error::InvalidBlock(_))),
        "a page keeps at least one block"
    );
    fill(&doc, &every_kind());
    // The paragraph that starts a list item cannot go while more follows.
    assert!(doc.delete_block(&[4, 0, 0]).is_err());
    // The only item of a list cannot go either.
    assert!(doc.delete_block(&[4, 0, 1, 0]).is_err());
    // A nested block after the first paragraph can.
    doc.delete_block(&[4, 0, 1]).expect("delete nested list");
    assert_eq!(doc.block(&[4, 0]).expect("read").children.len(), 1);
    assert!(matches!(doc.delete_block(&[]), Err(Error::NoSuchBlock(_))));
    assert!(matches!(
        doc.delete_block(&[200]),
        Err(Error::NoSuchBlock(_))
    ));
}

#[test]
fn code_block_ignores_marks_and_turns_breaks_into_newlines() {
    let doc = replica(1);
    let code = Block::new(BlockKind::CodeBlock {
        language: "txt".into(),
    })
    .with_inline(vec![run("a", bold()), Inline::Break, Inline::text("b")]);
    doc.append_block(&[], &code).expect("append");
    let read = doc.block(&[1]).expect("read");
    assert_eq!(read.inline, [Inline::text("a\nb")]);
}

#[test]
fn edit_returns_an_update_with_only_its_own_changes() {
    let doc = replica(1);
    let replica_2 = replica(2);
    pull(&replica_2, &doc);
    let (index, update) = doc
        .edit(|d| d.append_block(&[], &Block::paragraph("mới")))
        .expect("edit");
    assert_eq!(index, 1);
    replica_2.import(&update).expect("import update");
    assert_eq!(
        replica_2.blocks().expect("read"),
        doc.blocks().expect("read")
    );
}

#[test]
fn summary_extracts_text_links_attachments_and_words() {
    let hash = "a".repeat(64);
    let doc = replica(1);
    let mut blocks = every_kind();
    blocks.push(Block::new(BlockKind::Image {
        src: attachment_src(&hash),
        alt: "ảnh".into(),
    }));
    blocks.push(item(
        BlockKind::Quote,
        vec![Block::new(BlockKind::Image {
            src: attachment_src(&hash),
            alt: String::new(),
        })],
    ));
    blocks.push(Block::new(BlockKind::Image {
        src: "https://example.org/x.png".into(),
        alt: String::new(),
    }));
    // A reference that is not a node id is ignored.
    blocks.push(Block::new(BlockKind::PageLink {
        page_id: "page-target-0002".into(),
        title: String::new(),
    }));
    fill(&doc, &blocks);

    let summary = doc.summary().expect("summary");
    assert_eq!(summary.block_texts.len(), blocks.len());
    assert_eq!(summary.block_texts[0], "Tiêu đề một");
    assert_eq!(summary.block_texts[4], "Mục một\nMục con\nMục hai");
    assert_eq!(summary.block_texts[10], "", "divider has no text");
    assert!(
        summary
            .text
            .starts_with("Tiêu đề một\nMục hai\nMục ba\nXin chào, đậm và nghiêng\ncode()")
    );
    assert!(
        summary
            .text
            .contains("fn main() {\n    println!(\"Xin chào\");\n}")
    );
    assert!(
        !summary.text.contains("Trang con"),
        "titles of page blocks are not text"
    );
    assert_eq!(summary.text, doc.plain_text().expect("text"));

    assert_eq!(summary.attachments, [hash]);
    let links: Vec<_> = summary.links.iter().map(|l| (l.target, l.kind)).collect();
    assert_eq!(
        links,
        [
            (node_id("3@42"), LinkKind::ChildPage),
            (node_id("7@42"), LinkKind::PageLink),
            (node_id("7@42"), LinkKind::Inline),
        ]
    );
    assert!(summary.word_count > 20);
}

#[test]
fn word_count_ignores_punctuation_only_tokens() {
    use ne_core::page::word_count;
    assert_eq!(word_count("Xin chào, thế giới!"), 4);
    assert_eq!(word_count("-- *** …"), 0);
    assert_eq!(word_count("  a\nb\tc  "), 3);
    assert_eq!(word_count(""), 0);
}

#[test]
fn href_conventions_round_trip() {
    let id = node_id("12@345");
    assert_eq!(parse_page_href(&page_href(id)), Some(id));
    assert_eq!(parse_page_href("https://example.org"), None);
    assert_eq!(parse_page_href("ne-page:not-an-id"), None);

    let hash = "0123456789abcdef".repeat(4);
    assert_eq!(
        parse_attachment_src(&attachment_src(&hash)),
        Some(hash.as_str())
    );
    assert_eq!(parse_attachment_src("attachment:short"), None);
    assert_eq!(
        parse_attachment_src(&attachment_src(&hash.to_uppercase())),
        None
    );
    assert_eq!(parse_attachment_src("assets/x.png"), None);
}

#[test]
fn unknown_nodes_are_kept_and_skipped() {
    // A newer editor build may add node kinds; they must not break reading and
    // must not stop edits to the rest of the page.
    use loro::{LoroDoc, LoroList, LoroMap};
    let source = replica(1);
    fill(&source, &[Block::paragraph("giữ lại")]);
    let snapshot = source.export_snapshot().expect("snapshot");

    let raw = LoroDoc::new();
    raw.import(&snapshot).expect("import");
    let children = match raw
        .get_map("doc")
        .get("children")
        .expect("children")
        .into_container()
    {
        Ok(loro::Container::List(list)) => list,
        other => panic!("children is a list, got {other:?}"),
    };
    let node = children.push_container(LoroMap::new()).expect("node");
    node.insert("nodeName", "table").expect("name");
    node.insert_container("attributes", LoroMap::new())
        .expect("attrs");
    node.insert_container("children", LoroList::new())
        .expect("children");
    raw.commit();

    let doc = PageDoc::from_snapshot(&raw.export(loro::ExportMode::Snapshot).expect("export"))
        .expect("load");
    let blocks = doc.blocks().expect("read");
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[1].kind,
        BlockKind::Unknown {
            node_name: "table".into()
        }
    );
    doc.append_block(&[], &Block::paragraph("sau bảng"))
        .expect("edit next to unknown");
    doc.delete_block(&[0]).expect("delete next to unknown");
    assert!(matches!(
        doc.blocks().expect("read")[0].kind,
        BlockKind::Unknown { .. }
    ));
}

#[test]
fn corrupt_roots_report_errors() {
    let empty = loro::LoroDoc::new();
    let doc = PageDoc::from_snapshot(&empty.export(loro::ExportMode::Snapshot).expect("export"))
        .expect("load");
    assert!(matches!(doc.blocks(), Err(Error::InvalidPage(_))));
    assert!(PageDoc::from_snapshot(b"not a loro snapshot").is_err());
}
