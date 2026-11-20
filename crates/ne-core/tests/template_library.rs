//! The template library: create, list, rename, delete, and merging.

use std::collections::BTreeMap;

use ne_core::database::CellValue;
use ne_core::page::{Block, BlockKind};
use ne_core::template::{LibraryDoc, TemplateKind, template_blocks};

fn library() -> LibraryDoc {
    let doc = LibraryDoc::new().expect("library");
    doc.set_peer_id(1).expect("peer");
    doc
}

fn blocks() -> Vec<Block> {
    vec![
        Block::heading(1, "Họp tuần"),
        Block::new(BlockKind::ChildPage {
            page_id: "1@2".into(),
            title: "Child".into(),
        }),
        Block::new(BlockKind::Quote).with_children(vec![
            Block::paragraph("giữ lại"),
            Block::new(BlockKind::ChildPage {
                page_id: "3@4".into(),
                title: "Nested".into(),
            }),
        ]),
        Block::new(BlockKind::PageLink {
            page_id: "5@6".into(),
            title: "Link".into(),
        }),
    ]
}

#[test]
fn a_page_template_keeps_content_but_not_child_pages() {
    let doc = library();
    let id = doc
        .create(
            "Biên bản họp",
            TemplateKind::Page,
            None,
            &blocks(),
            &BTreeMap::new(),
            10,
        )
        .expect("create");
    let template = doc.get(&id).expect("get");
    assert_eq!(template.name, "Biên bản họp");
    assert_eq!(template.kind, TemplateKind::Page);
    assert_eq!(template.blocks.len(), 3, "the child page block is dropped");
    assert_eq!(
        template.blocks[1].children.len(),
        1,
        "also from nested blocks"
    );
    assert!(matches!(
        template.blocks[2].kind,
        BlockKind::PageLink { .. }
    ));
    assert_eq!(template_blocks(&blocks()), template.blocks);
}

#[test]
fn row_templates_belong_to_a_database_and_carry_defaults() {
    let doc = library();
    let defaults: BTreeMap<String, CellValue> = [("p1".to_owned(), CellValue::Number(3.0))].into();
    assert!(
        doc.create("x", TemplateKind::Row, None, &[], &defaults, 1)
            .is_err(),
        "a row template needs a database"
    );
    assert!(
        doc.create("x", TemplateKind::Page, Some("1@1"), &[], &defaults, 1)
            .is_err()
    );
    let row = doc
        .create(
            "Mẫu hàng",
            TemplateKind::Row,
            Some("1@7"),
            &[],
            &defaults,
            2,
        )
        .expect("row");
    let page = doc
        .create(
            "Mẫu trang",
            TemplateKind::Page,
            None,
            &[],
            &BTreeMap::new(),
            3,
        )
        .expect("page");
    assert_eq!(doc.get(&row).expect("row").defaults, defaults);
    let page_names: Vec<_> = doc.list(None).into_iter().map(|t| t.id).collect();
    assert_eq!(page_names, std::slice::from_ref(&page));
    let row_names: Vec<_> = doc.list(Some("1@7")).into_iter().map(|t| t.id).collect();
    assert_eq!(row_names, std::slice::from_ref(&row));
    assert!(doc.list(Some("1@8")).is_empty());
    doc.set_defaults(&row, &BTreeMap::new()).expect("defaults");
    assert!(doc.get(&row).expect("row").defaults.is_empty());
    assert!(doc.set_defaults(&page, &BTreeMap::new()).is_err());
}

#[test]
fn templates_rename_and_delete() {
    let doc = library();
    let id = doc
        .create("A", TemplateKind::Page, None, &[], &BTreeMap::new(), 1)
        .expect("create");
    doc.rename(&id, "B").expect("rename");
    assert_eq!(doc.get(&id).expect("get").name, "B");
    assert!(doc.rename(&id, " ").is_err());
    doc.delete(&id).expect("delete");
    assert!(doc.get(&id).is_err());
    assert!(doc.delete(&id).is_err());
    assert!(doc.list(None).is_empty());
}

#[test]
fn libraries_created_on_two_devices_merge_and_deletes_win() {
    let a = library();
    let b = LibraryDoc::new().expect("library");
    b.set_peer_id(2).expect("peer");
    let from_a = a
        .create("From a", TemplateKind::Page, None, &[], &BTreeMap::new(), 1)
        .expect("a");
    let from_b = b
        .create("From b", TemplateKind::Page, None, &[], &BTreeMap::new(), 2)
        .expect("b");
    a.import(&b.export_all_updates().expect("b updates"))
        .expect("into a");
    b.import(&a.export_all_updates().expect("a updates"))
        .expect("into b");
    assert_eq!(a.list(None).len(), 2);
    assert_eq!(a.list(None), b.list(None));
    // A rename on one device against a delete on the other: the delete wins.
    let (_, rename) = a.edit(|d| d.rename(&from_b, "renamed")).expect("rename");
    let (_, delete) = b.edit(|d| d.delete(&from_b)).expect("delete");
    a.import(&delete).expect("into a");
    b.import(&rename).expect("into b");
    assert_eq!(a.list(None).len(), 1);
    assert_eq!(a.list(None), b.list(None));
    assert_eq!(a.list(None)[0].id, from_a);
}
