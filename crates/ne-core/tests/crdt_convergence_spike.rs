//! Checks the two CRDT behaviours the data model depends on: concurrent text
//! edits merge without loss, and concurrent moves in the page tree can never
//! leave a cycle.

use loro::{ExportMode, LoroDoc, TreeParentId};

fn sync(a: &LoroDoc, b: &LoroDoc) {
    let from_a = a.export(ExportMode::all_updates()).expect("export a");
    let from_b = b.export(ExportMode::all_updates()).expect("export b");
    a.import(&from_b).expect("import into a");
    b.import(&from_a).expect("import into b");
}

#[test]
fn concurrent_text_edits_converge_without_losing_either_side() {
    let a = LoroDoc::new();
    a.set_peer_id(1).expect("peer id");
    let b = LoroDoc::new();
    b.set_peer_id(2).expect("peer id");

    a.get_text("body").insert(0, "ghi chú").expect("insert");
    a.commit();
    sync(&a, &b);

    // Both devices edit while offline.
    a.get_text("body").insert(0, "A: ").expect("insert");
    a.commit();
    let end = b.get_text("body").len_unicode();
    b.get_text("body").insert(end, " :B").expect("insert");
    b.commit();
    sync(&a, &b);

    let text_a = a.get_text("body").to_string();
    let text_b = b.get_text("body").to_string();
    assert_eq!(text_a, text_b);
    assert_eq!(text_a, "A: ghi chú :B");
}

#[test]
fn concurrent_moves_into_each_other_never_create_a_cycle() {
    let a = LoroDoc::new();
    a.set_peer_id(1).expect("peer id");
    let b = LoroDoc::new();
    b.set_peer_id(2).expect("peer id");

    let tree_a = a.get_tree("pages");
    let work = tree_a.create(TreeParentId::Root).expect("create");
    let personal = tree_a.create(TreeParentId::Root).expect("create");
    a.commit();
    sync(&a, &b);

    // Device A moves "work" under "personal" while device B does the opposite.
    tree_a.mov(work, personal).expect("move on a");
    a.commit();
    let tree_b = b.get_tree("pages");
    tree_b.mov(personal, work).expect("move on b");
    b.commit();
    sync(&a, &b);

    assert_eq!(a.get_deep_value(), b.get_deep_value());

    // Both pages are still reachable from the root, so no cycle was formed.
    let parent_of_work = tree_a.parent(work).expect("work exists");
    let parent_of_personal = tree_a.parent(personal).expect("personal exists");
    let roots = [parent_of_work, parent_of_personal]
        .iter()
        .filter(|parent| matches!(parent, TreeParentId::Root))
        .count();
    assert_eq!(roots, 1, "exactly one of the two moves should win");
}
