//! Backlinks: the link index behind "pages that link here".

mod store_support;

use ne_core::NodeId;
use ne_core::page::{Block, BlockKind, Inline, LinkKind, Marks, Run, page_href};
use ne_core::tree::Position;
use store_support::*;

fn page_link(target: NodeId) -> Block {
    Block::new(BlockKind::PageLink {
        page_id: target.to_string(),
        title: String::new(),
    })
}

fn inline_link(target: NodeId, text: &str) -> Block {
    Block::new(BlockKind::Paragraph).with_inline(vec![
        Inline::text("xem "),
        Inline::Text(Run {
            text: text.into(),
            marks: Marks {
                link: Some(page_href(target)),
                ..Marks::default()
            },
        }),
    ])
}

#[test]
fn backlinks_list_pages_that_link_with_how() {
    let (_dir, mut ws) = new_workspace();
    let target = page_with(&mut ws, "Đích", &paragraphs(&["nội dung"]));
    let block_only = page_with(&mut ws, "Bê", &[page_link(target)]);
    let inline_only = page_with(&mut ws, "Ác", &[inline_link(target, "đích")]);
    let both = page_with(
        &mut ws,
        "Cê",
        &[page_link(target), inline_link(target, "lại")],
    );
    page_with(
        &mut ws,
        "Không liên quan",
        &paragraphs(&["không có liên kết"]),
    );

    let backlinks = ws.backlinks(target).expect("backlinks");
    let summary: Vec<_> = backlinks
        .iter()
        .map(|b| (b.source, b.title.as_str(), b.kinds.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            (inline_only, "Ác", vec![LinkKind::Inline]),
            (block_only, "Bê", vec![LinkKind::PageLink]),
            (both, "Cê", vec![LinkKind::PageLink, LinkKind::Inline]),
        ],
        "sorted by title"
    );
    assert!(ws.backlinks(block_only).expect("backlinks").is_empty());
}

#[test]
fn child_page_blocks_and_self_links_are_not_backlinks() {
    let (_dir, mut ws) = new_workspace();
    let parent = ws.create_page(None, Position::End, "Cha").expect("page");
    let child = ws
        .create_page(Some(parent), Position::End, "Con")
        .expect("page");
    fill_page(
        &mut ws,
        parent,
        &[
            Block::new(BlockKind::ChildPage {
                page_id: child.to_string(),
                title: "Con".into(),
            }),
            page_link(parent),
        ],
    );
    assert!(
        ws.backlinks(child).expect("backlinks").is_empty(),
        "the parent shows its child, that is not a backlink"
    );
    assert!(
        ws.backlinks(parent).expect("backlinks").is_empty(),
        "a page does not link to itself"
    );
}

#[test]
fn backlinks_follow_edits_and_hide_trashed_sources() {
    let (dir, mut ws) = new_workspace();
    let target = page_with(&mut ws, "Đích", &paragraphs(&["x"]));
    let source = page_with(&mut ws, "Nguồn", &[page_link(target)]);
    assert_eq!(ws.backlinks(target).expect("backlinks").len(), 1);

    ws.edit_page(source, |d| {
        d.set_attrs(
            &[0],
            &BlockKind::PageLink {
                page_id: "5@5".into(),
                title: String::new(),
            },
        )
    })
    .expect("retarget the link");
    assert!(ws.backlinks(target).expect("backlinks").is_empty());
    ws.edit_page(source, |d| {
        d.set_attrs(
            &[0],
            &BlockKind::PageLink {
                page_id: target.to_string(),
                title: String::new(),
            },
        )
    })
    .expect("restore the link");
    assert_eq!(ws.backlinks(target).expect("backlinks").len(), 1);

    ws.trash_node(source).expect("trash");
    assert!(
        ws.backlinks(target).expect("backlinks").is_empty(),
        "a trashed page's links are hidden"
    );
    ws.restore(source).expect("restore");
    assert_eq!(ws.backlinks(target).expect("backlinks").len(), 1);

    // A link to a trashed page still lists its source, and survives a restart.
    ws.trash_node(target).expect("trash target");
    assert_eq!(ws.backlinks(target).expect("backlinks").len(), 1);
    drop(ws);
    let mut ws = reopen(dir.path());
    assert_eq!(ws.backlinks(target).expect("backlinks")[0].source, source);
    ws.rename(source, "Nguồn đã đổi").expect("rename");
    assert_eq!(
        ws.backlinks(target).expect("backlinks")[0].title,
        "Nguồn đã đổi"
    );
}

#[test]
fn links_to_pages_that_do_not_exist_are_kept_harmlessly() {
    let (_dir, mut ws) = new_workspace();
    let source = page_with(
        &mut ws,
        "S",
        &[Block::new(BlockKind::PageLink {
            page_id: "not-an-id".into(),
            title: String::new(),
        })],
    );
    assert!(ws.node(source).expect("node").is_some());
    let ghost: NodeId = "99@12345".parse().expect("id");
    assert!(ws.backlinks(ghost).expect("backlinks").is_empty());
}
