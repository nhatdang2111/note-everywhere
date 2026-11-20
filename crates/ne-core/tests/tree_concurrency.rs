//! Concurrent edits on two or three replicas, merged in every delivery order.

mod tree_support;

use ne_core::NodeId;
use ne_core::tree::{Position, WorkspaceTree};
use tree_support::{assert_converged, assert_forest, full_state, pull, replica, sync_all, titles};

const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

/// A shared starting point plus three replicas that start from it.
struct Fixture {
    base: WorkspaceTree,
    replicas: [WorkspaceTree; 3],
}

impl Fixture {
    /// `setup` builds the common history on a base replica; the three
    /// replicas (peer ids 1 to 3) then copy it.
    fn new(setup: impl FnOnce(&WorkspaceTree)) -> Self {
        let base = replica(100);
        setup(&base);
        let replicas = [replica(1), replica(2), replica(3)];
        for r in &replicas {
            pull(r, &base);
        }
        Self { base, replicas }
    }

    fn refs(&self) -> [&WorkspaceTree; 3] {
        [&self.replicas[0], &self.replicas[1], &self.replicas[2]]
    }

    /// Delivers the three replicas' concurrent edits to fresh observers in
    /// every order, then to the replicas themselves, and requires everyone to
    /// end in the same acyclic state. Returns that state's replica.
    fn merge_in_every_order(&self) -> &WorkspaceTree {
        for (n, order) in ORDERS.iter().enumerate() {
            let observer = replica(10 + u64::try_from(n).expect("small index"));
            pull(&observer, &self.base);
            for &i in order {
                pull(&observer, &self.replicas[i]);
            }
            assert_forest(&observer);
            sync_all(&self.refs());
            assert_converged(&[&observer, self.refs()[0], self.refs()[1], self.refs()[2]]);
        }
        for r in self.refs() {
            assert_forest(r);
        }
        self.refs()[0]
    }
}

fn ids(nodes: &[ne_core::tree::NodeInfo]) -> Vec<NodeId> {
    nodes.iter().map(|n| n.id).collect()
}

#[test]
fn concurrent_moves_forming_a_cycle_converge_without_losing_nodes() {
    let mut x = None;
    let mut y = None;
    let f = Fixture::new(|b| {
        x = Some(b.create_folder(None, Position::End, "x").expect("x"));
        y = Some(b.create_folder(None, Position::End, "y").expect("y"));
    });
    let (x, y) = (x.expect("x"), y.expect("y"));
    f.replicas[0]
        .move_node(x, Some(y), Position::End)
        .expect("x under y");
    f.replicas[1]
        .move_node(y, Some(x), Position::End)
        .expect("y under x");

    let merged = f.merge_in_every_order();
    let state = full_state(merged);
    assert_eq!(state.len(), 2, "no node lost");
    let roots = state.iter().filter(|n| n.parent.is_none()).count();
    assert_eq!(roots, 1, "exactly one move won");
}

#[test]
fn three_way_move_cycle_converges_with_every_node_reachable() {
    let mut nodes = Vec::new();
    let f = Fixture::new(|b| {
        for title in ["x", "y", "z"] {
            nodes.push(b.create_folder(None, Position::End, title).expect("node"));
        }
    });
    // x under y, y under z, z under x: a cycle only visible after merging.
    for (i, replica) in f.replicas.iter().enumerate() {
        replica
            .move_node(nodes[i], Some(nodes[(i + 1) % 3]), Position::End)
            .expect("move");
    }

    let merged = f.merge_in_every_order();
    assert_eq!(full_state(merged).len(), 3);
    for id in &nodes {
        assert!(merged.contains(*id));
        assert!(
            merged
                .snapshot()
                .expect("snapshot")
                .iter()
                .any(|n| n.id == *id)
        );
    }
}

#[test]
fn concurrent_rename_and_move_both_survive() {
    let mut p = None;
    let mut folder = None;
    let f = Fixture::new(|b| {
        folder = Some(
            b.create_folder(None, Position::End, "folder")
                .expect("folder"),
        );
        p = Some(b.create_page(None, Position::End, "page").expect("page"));
    });
    let (p, folder) = (p.expect("p"), folder.expect("folder"));
    f.replicas[0].rename(p, "renamed").expect("rename");
    f.replicas[1]
        .move_node(p, Some(folder), Position::End)
        .expect("move");

    let merged = f.merge_in_every_order();
    let info = merged.get(p).expect("page");
    assert_eq!(info.title, "renamed");
    assert_eq!(info.parent, Some(folder));
}

#[test]
fn concurrent_edits_to_the_same_title_converge_to_one_value() {
    let mut p = None;
    let f = Fixture::new(|b| p = Some(b.create_page(None, Position::End, "orig").expect("p")));
    let p = p.expect("p");
    let candidates = ["from one", "from two", "from three"];
    for (replica, title) in f.replicas.iter().zip(candidates) {
        replica.rename(p, title).expect("rename");
    }

    let merged = f.merge_in_every_order();
    let title = merged.get(p).expect("page").title;
    assert!(candidates.contains(&title.as_str()), "got {title:?}");
}

#[test]
fn trash_on_one_replica_and_new_child_on_another_leaves_no_visible_orphan() {
    let mut folder = None;
    let mut old_child = None;
    let f = Fixture::new(|b| {
        let fo = b
            .create_folder(None, Position::End, "folder")
            .expect("folder");
        old_child = Some(b.create_page(Some(fo), Position::End, "old").expect("old"));
        b.create_page(None, Position::End, "other").expect("other");
        folder = Some(fo);
    });
    let folder = folder.expect("folder");
    f.replicas[0].trash_node(folder).expect("trash");
    let new_child = f.replicas[1]
        .create_page(Some(folder), Position::End, "new")
        .expect("child");
    let first_child = f.replicas[2]
        .create_page(Some(folder), Position::Index(0), "first")
        .expect("child");

    let merged = f.merge_in_every_order();
    assert_eq!(titles(&merged.snapshot().expect("snapshot")), ["other"]);
    let trash = merged.trash().expect("trash");
    assert_eq!(ids(&trash), vec![folder], "only the folder is a trash item");
    // The late children are attached under the trashed folder, hidden with it.
    let below = merged.descendants(folder).expect("descendants");
    for child in [new_child, first_child, old_child.expect("old")] {
        assert!(below.contains(&child));
        assert_eq!(merged.get(child).expect("child").parent, Some(folder));
    }

    // Restoring brings the late children back with the folder.
    merged.restore(folder).expect("restore");
    let visible = merged.snapshot().expect("snapshot");
    assert_eq!(visible.len(), 5);
    assert_forest(merged);
}

#[test]
fn trashing_an_ancestor_while_a_descendant_is_added_deeper() {
    let mut top = None;
    let mut mid = None;
    let f = Fixture::new(|b| {
        let t = b.create_folder(None, Position::End, "top").expect("top");
        mid = Some(b.create_folder(Some(t), Position::End, "mid").expect("mid"));
        top = Some(t);
    });
    let (top, mid) = (top.expect("top"), mid.expect("mid"));
    f.replicas[0].trash_node(top).expect("trash");
    f.replicas[1]
        .create_page(Some(mid), Position::End, "deep")
        .expect("deep");

    let merged = f.merge_in_every_order();
    assert!(merged.snapshot().expect("snapshot").is_empty());
    assert_eq!(ids(&merged.trash().expect("trash")), vec![top]);
    assert_eq!(merged.descendants(top).expect("descendants").len(), 2);
}

#[test]
fn concurrent_inserts_at_the_same_position_converge_to_one_order() {
    let f = Fixture::new(|b| {
        b.create_page(None, Position::End, "first").expect("first");
        b.create_page(None, Position::End, "last").expect("last");
    });
    for (replica, title) in f.replicas.iter().zip(["a", "b", "c"]) {
        replica
            .create_page(None, Position::Index(1), title)
            .expect("insert");
    }

    let merged = f.merge_in_every_order();
    let order = titles(&merged.snapshot().expect("snapshot")).join(",");
    let all = merged.snapshot().expect("snapshot");
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].title, "first");
    assert_eq!(all[4].title, "last");
    for other in f.refs() {
        assert_eq!(
            titles(&other.snapshot().expect("snapshot")).join(","),
            order
        );
    }
}

#[test]
fn concurrent_appends_and_first_inserts_in_an_empty_folder_converge() {
    let mut folder = None;
    let f = Fixture::new(|b| {
        folder = Some(b.create_folder(None, Position::End, "f").expect("f"));
    });
    let folder = folder.expect("f");
    for (replica, title) in f.replicas.iter().zip(["a", "b", "c"]) {
        replica
            .create_page(Some(folder), Position::End, title)
            .expect("append");
    }
    let merged = f.merge_in_every_order();
    assert_eq!(merged.children(Some(folder), false).expect("kids").len(), 3);
}

#[test]
fn concurrent_reorder_of_the_same_node_converges() {
    let mut nodes = Vec::new();
    let f = Fixture::new(|b| {
        for title in ["a", "b", "c", "d"] {
            nodes.push(b.create_page(None, Position::End, title).expect("n"));
        }
    });
    f.replicas[0]
        .reorder(nodes[0], Position::End)
        .expect("a last");
    f.replicas[1]
        .reorder(nodes[0], Position::Index(2))
        .expect("a mid");
    f.replicas[2]
        .reorder(nodes[3], Position::Index(0))
        .expect("d first");

    let merged = f.merge_in_every_order();
    assert_eq!(merged.snapshot().expect("snapshot").len(), 4);
}
