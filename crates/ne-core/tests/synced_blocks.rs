//! The page-side operations of synced blocks.

use ne_core::NodeId;
use ne_core::page::{Block, BlockKind, PageDoc, summarize};
use ne_core::roles::{Role, role_of};
use ne_core::synced::{
    insert_placeholder, new_shared_doc, parse_sync_id, replace_with_placeholder, selection,
    sync_ids, unsync_at,
};

fn page(texts: &[&str]) -> PageDoc {
    let doc = PageDoc::with_peer_id(5).expect("page");
    doc.replace_blocks(
        &texts
            .iter()
            .map(|t| Block::paragraph(t))
            .collect::<Vec<_>>(),
    )
    .expect("content");
    doc
}

fn texts(doc: &PageDoc) -> Vec<String> {
    doc.blocks()
        .expect("blocks")
        .iter()
        .map(|b| match &b.kind {
            BlockKind::SyncedBlock { sync_id } => format!("[{sync_id}]"),
            _ => b.plain_text(),
        })
        .collect()
}

#[test]
fn a_shared_document_is_laid_out_like_a_page_and_tagged() {
    let shared = new_shared_doc(&[Block::paragraph("shared")], 7).expect("shared");
    let bytes = shared.export_snapshot().expect("snapshot");
    assert_eq!(role_of(&bytes), Some(Role::Synced));
    assert_eq!(shared.plain_text().expect("text"), "shared");
    // A plain page has no role.
    assert_eq!(
        role_of(&page(&["x"]).export_snapshot().expect("snapshot")),
        None
    );
    let nested =
        Block::new(BlockKind::Quote).with_children(vec![ne_core::synced::placeholder("1@2")]);
    assert!(
        new_shared_doc(&[nested], 7).is_err(),
        "no nesting of synced blocks"
    );
}

#[test]
fn selected_blocks_become_one_placeholder() {
    let doc = page(&["a", "b", "c", "d"]);
    let chosen = selection(&doc, 1, 3).expect("selection");
    assert_eq!(chosen.len(), 2);
    assert!(selection(&doc, 2, 2).is_err());
    assert!(selection(&doc, 3, 9).is_err());
    replace_with_placeholder(&doc, 1, 3, "1@9").expect("replace");
    assert_eq!(texts(&doc), ["a", "[1@9]", "d"]);
    assert!(
        selection(&doc, 1, 2).is_err(),
        "a placeholder is not selectable again"
    );
    // Everything selected still leaves the page with a block.
    let all = page(&["x", "y"]);
    replace_with_placeholder(&all, 0, 2, "1@8").expect("replace all");
    assert_eq!(texts(&all), ["[1@8]"]);
}

#[test]
fn placeholders_are_found_in_the_page_summary() {
    let doc = page(&["a"]);
    insert_placeholder(&doc, &[], 1, "2@3").expect("insert");
    let quote = Block::new(BlockKind::Quote).with_children(vec![Block::paragraph("q")]);
    doc.append_block(&[], &quote).expect("quote");
    insert_placeholder(&doc, &[2], 1, "1@2").expect("nested");
    insert_placeholder(&doc, &[], 0, "2@3").expect("same id twice");
    let blocks = doc.blocks().expect("blocks");
    assert_eq!(sync_ids(&blocks), ["1@2", "2@3"]);
    assert_eq!(summarize(&blocks).synced, ["1@2", "2@3"]);
    assert!(summarize(&blocks).text.contains('a'));
}

#[test]
fn unsyncing_copies_the_content_back_in_place() {
    let shared =
        new_shared_doc(&[Block::paragraph("one"), Block::paragraph("two")], 7).expect("shared");
    let doc = page(&["before", "after"]);
    insert_placeholder(&doc, &[], 1, "1@9").expect("insert");
    let id = unsync_at(&doc, &[1], &shared.blocks().expect("content")).expect("unsync");
    assert_eq!(id, "1@9");
    assert_eq!(texts(&doc), ["before", "one", "two", "after"]);
    assert!(
        unsync_at(&doc, &[0], &shared.blocks().expect("content")).is_err(),
        "not a placeholder"
    );
    // Inside a container too.
    let quote = Block::new(BlockKind::Quote).with_children(vec![Block::paragraph("q")]);
    doc.append_block(&[], &quote).expect("quote");
    insert_placeholder(&doc, &[4], 0, "1@9").expect("in quote");
    unsync_at(&doc, &[4, 0], &shared.blocks().expect("content")).expect("unsync nested");
    let nested = doc.block(&[4]).expect("quote");
    assert_eq!(nested.children.len(), 3);
}

#[test]
fn a_placeholder_survives_the_page_document_round_trip() {
    let doc = page(&["a"]);
    insert_placeholder(&doc, &[], 1, "3@4").expect("insert");
    let loaded = PageDoc::from_snapshot(&doc.export_snapshot().expect("snapshot")).expect("load");
    assert_eq!(texts(&loaded), ["a", "[3@4]"]);
    assert!(parse_sync_id("3@4").is_ok());
    assert!(parse_sync_id("nope").is_err());
    let fresh = NodeId::fresh_document().to_string();
    assert!(parse_sync_id(&fresh).is_ok());
    assert_ne!(fresh, NodeId::fresh_document().to_string());
}
