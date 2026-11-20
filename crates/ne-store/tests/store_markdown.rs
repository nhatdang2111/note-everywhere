//! Markdown export and import through the workspace.

mod store_support;

use std::fs;

use ne_core::page::{Block, BlockKind, attachment_src};
use ne_core::tree::{NodeKind, Position};
use ne_store::{DocRef, Workspace};
use store_support::*;

fn titles(ws: &Workspace, parent: Option<ne_core::NodeId>) -> Vec<(String, NodeKind)> {
    ws.children(parent)
        .expect("children")
        .into_iter()
        .map(|r| (r.title, r.kind))
        .collect()
}

#[test]
fn a_workspace_exports_and_imports_into_another_with_content_and_attachments() {
    let (_da, mut a) = new_workspace();
    let image = a
        .add_attachment("Sơ đồ.png", b"PNG-bytes-of-the-diagram")
        .expect("attachment");
    let folder = a
        .create_folder(None, Position::End, "Dự án")
        .expect("folder");
    let parent = page_in(
        &mut a,
        Some(folder),
        "Kế hoạch",
        &[
            Block::heading(2, "Mục tiêu"),
            Block::paragraph("Hoàn thành bản đầu tiên"),
            Block::new(BlockKind::Image {
                src: attachment_src(&image.hash),
                alt: "Sơ đồ".into(),
            }),
        ],
    );
    let child = page_in(
        &mut a,
        Some(parent),
        "Chi tiết",
        &paragraphs(&["các bước nhỏ"]),
    );
    a.edit_page(parent, |d| {
        d.append_block(
            &[],
            &Block::new(BlockKind::ChildPage {
                page_id: child.to_string(),
                title: "Chi tiết".into(),
            }),
        )
    })
    .expect("link the child");
    let trashed = page_with(&mut a, "Đã xoá", &paragraphs(&["không xuất"]));
    a.trash_node(trashed).expect("trash");

    let out = tempfile::tempdir().expect("tempdir");
    let report = a.export_markdown(None, out.path()).expect("export");
    assert_eq!(
        (report.folders, report.pages, report.attachments),
        (1, 2, 1)
    );
    assert!(out.path().join("Dự án/Kế hoạch.md").is_file());
    assert!(out.path().join("Dự án/Kế hoạch/Chi tiết.md").is_file());
    assert!(out.path().join("Dự án/assets/Sơ đồ.png").is_file());
    assert!(!out.path().join("Đã xoá.md").exists());

    let (db, mut b) = new_workspace();
    let report = b.import_markdown_dir(None, out.path()).expect("import");
    assert_eq!((report.folders, report.pages), (1, 2));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let top = titles(&b, None);
    assert_eq!(top, [("Dự án".to_owned(), NodeKind::Folder)]);
    let folder_b = b.children(None).expect("children")[0].id;
    let parent_b = b.children(Some(folder_b)).expect("children")[0].id;
    assert_eq!(
        titles(&b, Some(folder_b)),
        [("Kế hoạch".to_owned(), NodeKind::Page)]
    );
    assert_eq!(
        titles(&b, Some(parent_b)),
        [("Chi tiết".to_owned(), NodeKind::Page)]
    );
    let child_b = b.children(Some(parent_b)).expect("children")[0].id;

    // Content, links and the image all came across; the image is stored under
    // the same hash because the bytes are the same.
    let blocks = b
        .open_page(parent_b)
        .expect("open")
        .blocks()
        .expect("blocks");
    assert_eq!(blocks[0], Block::heading(2, "Mục tiêu"));
    assert_eq!(blocks[1], Block::paragraph("Hoàn thành bản đầu tiên"));
    assert_eq!(
        blocks[2].kind,
        BlockKind::Image {
            src: attachment_src(&image.hash),
            alt: "Sơ đồ".into()
        }
    );
    assert_eq!(
        blocks[3].kind,
        BlockKind::ChildPage {
            page_id: child_b.to_string(),
            title: "Chi tiết".into()
        }
    );
    assert_eq!(
        b.read_attachment(&image.hash).expect("read"),
        b"PNG-bytes-of-the-diagram"
    );
    assert_eq!(
        b.attachment(&image.hash)
            .expect("info")
            .expect("stored")
            .ref_count,
        1
    );
    assert_eq!(page_text(&mut b, child_b), "các bước nhỏ");

    // Everything is searchable, queued for sync and durable.
    assert_eq!(b.search("buoc nho", 5).expect("search")[0].id, child_b);
    assert_eq!(b.search("ke hoach", 5).expect("search")[0].id, parent_b);
    let queued = b.outbox_peek(100).expect("peek");
    assert_eq!(
        queued.iter().filter(|i| i.doc == DocRef::Tree).count(),
        1,
        "one tree update for the whole import"
    );
    assert!(queued.iter().any(|i| i.doc == DocRef::Page(parent_b)));
    assert!(queued.iter().any(|i| i.doc == DocRef::Page(child_b)));
    drop(b);
    let mut b = reopen(db.path());
    assert_eq!(page_text(&mut b, child_b), "các bước nhỏ");

    // The imported workspace can be synced to a third device.
    let (_dc, mut c) = new_workspace();
    deliver(&mut b, &mut c);
    assert_eq!(
        c.tree().snapshot().expect("c"),
        b.tree().snapshot().expect("b")
    );
    assert_eq!(page_text(&mut c, child_b), "các bước nhỏ");
    assert_eq!(
        c.dump_derived_indexes()
            .expect("c")
            .lines()
            .filter(|l| l.contains("các bước nhỏ"))
            .count(),
        1
    );
}

#[test]
fn importing_a_foreign_folder_under_a_parent() {
    let src = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(src.path().join("notes/sub")).expect("mkdir");
    fs::write(
        src.path().join("notes/Ý tưởng.md"),
        "# Ý tưởng lớn\n\n- một\n- hai\n\n> [!callout 💡]\n>\n> nhớ làm\n",
    )
    .expect("write");
    fs::write(src.path().join("notes/sub/Chi tiết.md"), "Chỉ có chữ\n").expect("write");

    let (dir, mut ws) = new_workspace();
    let anchor = ws
        .create_folder(None, Position::End, "Nhập")
        .expect("anchor");
    let report = ws
        .import_markdown_dir(Some(anchor), src.path())
        .expect("import");
    assert_eq!((report.folders, report.pages), (2, 2));

    let notes = ws.children(Some(anchor)).expect("children")[0].id;
    let idea = ws
        .children(Some(notes))
        .expect("children")
        .into_iter()
        .find(|r| r.kind == NodeKind::Page)
        .expect("page");
    assert_eq!(
        idea.title, "Ý tưởng lớn",
        "the leading heading is the title"
    );
    let kinds: Vec<_> = ws
        .open_page(idea.id)
        .expect("open")
        .blocks()
        .expect("blocks")
        .iter()
        .map(|b| b.kind.node_name().to_owned())
        .collect();
    assert_eq!(kinds, ["bullet_list", "callout"]);
    assert_eq!(ws.search("nho lam", 5).expect("search")[0].id, idea.id);
    drop(ws);
    let ws = reopen(dir.path());
    assert_eq!(ws.search("chi co chu", 5).expect("search").len(), 1);
    ws.check_integrity().expect("sound");
}

#[test]
fn a_failed_import_leaves_the_workspace_unchanged() {
    let (_dir, mut ws) = new_workspace();
    ws.create_folder(None, Position::End, "Có sẵn")
        .expect("folder");
    let before = ws.tree().snapshot().expect("snapshot");
    let queued = ws.outbox_len().expect("len");
    let missing = tempfile::tempdir()
        .expect("tempdir")
        .path()
        .join("không tồn tại");
    assert!(ws.import_markdown_dir(None, &missing).is_err());
    assert_eq!(ws.tree().snapshot().expect("snapshot"), before);
    assert_eq!(ws.outbox_len().expect("len"), queued);
    assert_eq!(ws.sidebar().expect("sidebar").len(), 1);
}

#[test]
fn exporting_a_single_page_writes_its_file_and_attachments() {
    let (_dir, mut ws) = new_workspace();
    let image = ws.add_attachment("a.png", b"abc").expect("attachment");
    let page = page_with(
        &mut ws,
        "Một trang",
        &[
            Block::paragraph("nội dung"),
            Block::new(BlockKind::Image {
                src: attachment_src(&image.hash),
                alt: String::new(),
            }),
        ],
    );
    let out = tempfile::tempdir().expect("tempdir");
    let report = ws.export_markdown(Some(page), out.path()).expect("export");
    assert_eq!(
        (report.folders, report.pages, report.attachments),
        (0, 1, 1)
    );
    let text = fs::read_to_string(out.path().join("Một trang.md")).expect("page file");
    assert_eq!(text, "# Một trang\n\nnội dung\n\n![](assets/a.png)\n");
    assert_eq!(
        fs::read(out.path().join("assets/a.png")).expect("attachment"),
        b"abc"
    );
}
