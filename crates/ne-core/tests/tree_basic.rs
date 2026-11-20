//! Single-replica behaviour of `WorkspaceTree`: structure, metadata, trash,
//! error handling and the sync surface.

mod tree_support;

use std::sync::Arc;

use ne_core::tree::{NodeKind, Position, SystemClock, WorkspaceTree};
use ne_core::{Error, NodeId};
use tree_support::{replica, titles};

#[test]
fn creates_ordered_children_and_reports_metadata() {
    let t = replica(1);
    let docs = t
        .create_folder(None, Position::End, "Docs")
        .expect("folder");
    let b = t.create_page(Some(docs), Position::End, "b").expect("b");
    let a = t
        .create_page(Some(docs), Position::Index(0), "a")
        .expect("a");
    let c = t
        .create_page(Some(docs), Position::Index(2), "c")
        .expect("c");

    let kids = t.children(Some(docs), false).expect("children");
    assert_eq!(titles(&kids), ["a", "b", "c"]);
    assert_eq!(kids[0].id, a);
    assert_eq!(kids[1].id, b);
    assert_eq!(kids[2].id, c);
    assert_eq!(kids[0].kind, NodeKind::Page);
    assert_eq!(kids[0].parent, Some(docs));
    assert_eq!(t.get(docs).expect("docs").kind, NodeKind::Folder);
    assert_eq!(t.get(docs).expect("docs").parent, None);
    assert!(kids[0].created_at > 0 && kids[0].updated_at >= kids[0].created_at);
    assert_eq!(kids[0].icon, None);
    assert!(!kids[0].favorite);

    let snapshot = t.snapshot().expect("snapshot");
    assert_eq!(titles(&snapshot), ["Docs", "a", "b", "c"]);
}

#[test]
fn rename_icon_favorite_update_metadata_and_timestamp() {
    let t = replica(1);
    let page = t.create_page(None, Position::End, "old").expect("page");
    let created = t.get(page).expect("get").created_at;

    t.rename(page, "new title").expect("rename");
    t.set_icon(page, Some("📝")).expect("icon");
    t.set_favorite(page, true).expect("favorite");
    let info = t.get(page).expect("get");
    assert_eq!(info.title, "new title");
    assert_eq!(info.icon.as_deref(), Some("📝"));
    assert!(info.favorite);
    assert_eq!(info.created_at, created);
    assert!(info.updated_at > created);
    assert_eq!(t.favorites().expect("favorites").len(), 1);

    t.set_icon(page, None).expect("clear icon");
    t.set_favorite(page, false).expect("unfavorite");
    let info = t.get(page).expect("get");
    assert_eq!(info.icon, None);
    assert!(t.favorites().expect("favorites").is_empty());
}

#[test]
fn rejects_invalid_icons_and_positions() {
    let t = replica(1);
    let page = t.create_page(None, Position::End, "p").expect("page");
    assert!(matches!(
        t.set_icon(page, Some("")),
        Err(Error::InvalidIcon { len: 0, .. })
    ));
    assert!(matches!(
        t.set_icon(page, Some(&"x".repeat(40))),
        Err(Error::InvalidIcon { len: 40, .. })
    ));
    assert_eq!(
        t.create_page(None, Position::Index(5), "x"),
        Err(Error::PositionOutOfRange { index: 5, len: 1 })
    );
    assert_eq!(
        t.reorder(page, Position::Index(3)),
        Err(Error::PositionOutOfRange { index: 3, len: 0 })
    );
}

#[test]
fn unknown_ids_return_errors_instead_of_panicking() {
    let t = replica(1);
    let other = replica(2);
    let ghost = other
        .create_page(None, Position::End, "ghost")
        .expect("page");

    assert_eq!(t.get(ghost), Err(Error::NotFound(ghost)));
    assert_eq!(t.rename(ghost, "x"), Err(Error::NotFound(ghost)));
    assert_eq!(t.trash_node(ghost), Err(Error::NotFound(ghost)));
    assert_eq!(t.restore(ghost), Err(Error::NotFound(ghost)));
    assert_eq!(t.delete_permanently(ghost), Err(Error::NotFound(ghost)));
    assert_eq!(t.descendants(ghost), Err(Error::NotFound(ghost)));
    assert_eq!(t.ancestors(ghost).map(|_| ()), Err(Error::NotFound(ghost)));
    assert_eq!(
        t.create_page(Some(ghost), Position::End, "x"),
        Err(Error::NotFound(ghost))
    );
    assert!(!t.contains(ghost));
    assert!(t.children(Some(ghost), false).is_err());
}

#[test]
fn node_id_string_form_round_trips_and_rejects_garbage() {
    let t = replica(7);
    let page = t.create_page(None, Position::End, "p").expect("page");
    let text = page.to_string();
    assert_eq!(text.parse::<NodeId>().expect("parse"), page);
    for bad in ["", "nope", "12@", "@12", "1@x", "x@1"] {
        assert!(
            matches!(bad.parse::<NodeId>(), Err(Error::InvalidNodeId(_))),
            "{bad:?} should be rejected"
        );
    }
}

#[test]
fn move_and_reorder_place_nodes_among_visible_siblings() {
    let t = replica(1);
    let a = t.create_folder(None, Position::End, "a").expect("a");
    let b = t.create_folder(None, Position::End, "b").expect("b");
    let c = t.create_folder(None, Position::End, "c").expect("c");

    t.reorder(c, Position::Index(0)).expect("c first");
    assert_eq!(
        titles(&t.children(None, false).expect("kids")),
        ["c", "a", "b"]
    );
    t.reorder(c, Position::End).expect("c last");
    assert_eq!(
        titles(&t.children(None, false).expect("kids")),
        ["a", "b", "c"]
    );
    t.move_node(c, Some(a), Position::End).expect("into a");
    t.move_node(b, Some(a), Position::Index(0)).expect("into a");
    assert_eq!(
        titles(&t.children(Some(a), false).expect("kids")),
        ["b", "c"]
    );
    assert_eq!(t.get(b).expect("b").parent, Some(a));
    let crumbs = t.ancestors(c).expect("ancestors");
    assert_eq!(titles(&crumbs), ["a"]);
}

#[test]
fn rejects_moving_a_node_into_itself_or_a_descendant() {
    let t = replica(1);
    let top = t.create_folder(None, Position::End, "top").expect("top");
    let mid = t
        .create_folder(Some(top), Position::End, "mid")
        .expect("mid");
    let leaf = t
        .create_page(Some(mid), Position::End, "leaf")
        .expect("leaf");

    for target in [top, mid, leaf] {
        assert_eq!(
            t.move_node(top, Some(target), Position::End),
            Err(Error::MoveIntoDescendant {
                node: top,
                new_parent: target
            })
        );
    }
    assert_eq!(t.get(leaf).expect("leaf").parent, Some(mid));
    // Moving a leaf up is fine.
    t.move_node(leaf, None, Position::End)
        .expect("leaf to root");
}

#[test]
fn trash_hides_subtree_and_restore_returns_it_in_place() {
    let t = replica(1);
    let a = t.create_folder(None, Position::End, "a").expect("a");
    let b = t.create_folder(None, Position::End, "b").expect("b");
    let c = t.create_folder(None, Position::End, "c").expect("c");
    let child = t
        .create_page(Some(b), Position::End, "child")
        .expect("child");

    t.trash_node(b).expect("trash b");
    assert_eq!(titles(&t.snapshot().expect("snapshot")), ["a", "c"]);
    assert_eq!(titles(&t.children(None, false).expect("kids")), ["a", "c"]);
    assert_eq!(
        titles(&t.children(None, true).expect("kids")),
        ["a", "b", "c"]
    );
    let trash = t.trash().expect("trash");
    assert_eq!(titles(&trash), ["b"]);
    assert!(trash[0].trashed_at.is_some());
    assert_eq!(t.descendants(b).expect("descendants"), vec![child]);
    // The hidden subtree is read-only until restored.
    assert_eq!(t.rename(child, "x"), Err(Error::Trashed(child)));
    assert_eq!(t.trash_node(b), Err(Error::Trashed(b)));
    assert_eq!(
        t.create_page(Some(b), Position::End, "x"),
        Err(Error::Trashed(b))
    );
    assert_eq!(t.restore(a), Err(Error::NotTrashed(a)));

    // Positions count only visible siblings; the trashed slot is skipped.
    t.reorder(c, Position::Index(0)).expect("c first");
    t.restore(b).expect("restore");
    assert!(t.trash().expect("trash").is_empty());
    assert_eq!(t.get(child).expect("child").parent, Some(b));
    assert_eq!(
        titles(&t.snapshot().expect("snapshot")),
        ["c", "a", "b", "child"]
    );
}

#[test]
fn restore_goes_to_root_when_an_ancestor_is_still_trashed() {
    let t = replica(1);
    let parent = t.create_folder(None, Position::End, "parent").expect("p");
    let child = t
        .create_page(Some(parent), Position::End, "child")
        .expect("c");

    t.trash_node(child).expect("trash child first");
    t.trash_node(parent).expect("trash parent");
    assert_eq!(titles(&t.trash().expect("trash")), ["parent"]);

    t.restore(child).expect("restore child");
    let info = t.get(child).expect("child");
    assert_eq!(info.parent, None);
    assert_eq!(titles(&t.snapshot().expect("snapshot")), ["child"]);
    assert_eq!(titles(&t.trash().expect("trash")), ["parent"]);
}

#[test]
fn delete_permanently_removes_the_subtree_and_reports_every_id() {
    let t = replica(1);
    let keep = t.create_folder(None, Position::End, "keep").expect("keep");
    let gone = t.create_folder(None, Position::End, "gone").expect("gone");
    let inner = t
        .create_page(Some(gone), Position::End, "inner")
        .expect("inner");

    assert_eq!(t.delete_permanently(gone), Err(Error::NotTrashed(gone)));
    t.trash_node(gone).expect("trash");
    // A node inside the trash can be deleted on its own too.
    let removed = t.delete_permanently(gone).expect("delete");
    assert_eq!(removed, vec![gone, inner]);
    for id in [gone, inner] {
        assert!(!t.contains(id));
        assert_eq!(t.get(id), Err(Error::NotFound(id)));
    }
    assert!(t.trash().expect("trash").is_empty());
    assert_eq!(titles(&t.snapshot().expect("snapshot")), ["keep"]);
    assert!(t.contains(keep));
}

#[test]
fn empty_trash_deletes_all_trashed_subtrees() {
    let t = replica(1);
    let a = t.create_folder(None, Position::End, "a").expect("a");
    let a_child = t.create_page(Some(a), Position::End, "a1").expect("a1");
    let b = t.create_page(None, Position::End, "b").expect("b");
    let c = t.create_page(None, Position::End, "c").expect("c");
    t.trash_node(a).expect("trash a");
    t.trash_node(b).expect("trash b");

    let mut removed = t.empty_trash().expect("empty");
    removed.sort();
    let mut expected = vec![a, a_child, b];
    expected.sort();
    assert_eq!(removed, expected);
    assert!(t.trash().expect("trash").is_empty());
    assert_eq!(titles(&t.snapshot().expect("snapshot")), ["c"]);
    assert!(t.contains(c));
    assert_eq!(t.empty_trash().expect("empty again"), vec![]);
}

#[test]
fn snapshot_round_trip_keeps_state_and_is_not_reported_as_a_change() {
    let t = replica(1);
    let a = t.create_folder(None, Position::End, "a").expect("a");
    t.create_page(Some(a), Position::End, "p").expect("p");
    t.set_favorite(a, true).expect("fav");
    let bytes = t.export_snapshot().expect("export");

    let loaded = WorkspaceTree::from_snapshot(&bytes, Arc::new(SystemClock)).expect("load");
    assert_eq!(
        loaded.snapshot().expect("snapshot"),
        t.snapshot().expect("snapshot")
    );
    assert!(loaded.take_changes().is_empty());
    assert_eq!(loaded.frontiers(), t.frontiers());
    assert!(WorkspaceTree::from_snapshot(b"garbage", Arc::new(SystemClock)).is_err());
    assert!(loaded.import(b"garbage").is_err());
    assert!(loaded.export_updates_since(b"garbage").is_err());
}

#[test]
fn take_changes_reports_local_edits_and_imports() {
    let a = replica(1);
    let b = replica(2);
    let folder = a.create_folder(None, Position::End, "f").expect("f");
    let page = a.create_page(Some(folder), Position::End, "p").expect("p");
    let changed = a.take_changes();
    assert!(changed.contains(&folder) && changed.contains(&page));
    assert!(a.take_changes().is_empty(), "drained");

    a.rename(page, "renamed").expect("rename");
    assert_eq!(a.take_changes().into_iter().collect::<Vec<_>>(), vec![page]);

    tree_support::pull(&b, &a);
    let imported = b.take_changes();
    assert!(imported.contains(&folder) && imported.contains(&page));

    a.rename(page, "again").expect("rename");
    a.trash_node(folder).expect("trash");
    a.take_changes();
    tree_support::pull(&b, &a);
    let imported = b.take_changes();
    assert!(imported.contains(&page), "metadata-only change is reported");
    assert!(imported.contains(&folder), "trash flag is reported");

    a.delete_permanently(folder).expect("delete");
    a.take_changes();
    tree_support::pull(&b, &a);
    let imported = b.take_changes();
    assert!(imported.contains(&folder), "deleted node is reported");
}

#[test]
fn updates_since_a_version_only_carry_the_missing_changes() {
    let a = replica(1);
    let b = replica(2);
    a.create_page(None, Position::End, "one").expect("one");
    tree_support::sync(&a, &b);
    a.create_page(None, Position::End, "two").expect("two");

    let delta = a.export_updates_since(&b.version()).expect("delta");
    let full = a.export_snapshot().expect("full");
    assert!(delta.len() < full.len());
    let outcome = b.import(&delta).expect("import");
    assert!(!outcome.has_missing_dependencies);
    assert_eq!(b.frontiers(), a.frontiers());
    assert_eq!(titles(&b.snapshot().expect("snapshot")), ["one", "two"]);
}
