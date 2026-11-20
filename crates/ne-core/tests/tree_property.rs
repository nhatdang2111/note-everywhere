//! Random operation sequences on three replicas with random sync
//! interleavings must always converge to identical forests.

mod tree_support;

use ne_core::Error;
use ne_core::tree::{Position, WorkspaceTree};
use proptest::prelude::*;
use tree_support::{assert_converged, assert_forest, full_state, pull, replica, sync_all};

const REPLICAS: usize = 3;

#[derive(Debug, Clone)]
enum Action {
    Create {
        page: bool,
        parent: usize,
        pos: usize,
    },
    Rename {
        node: usize,
        title: u8,
    },
    Icon {
        node: usize,
        on: bool,
    },
    Favorite {
        node: usize,
        on: bool,
    },
    Move {
        node: usize,
        parent: usize,
        pos: usize,
    },
    Reorder {
        node: usize,
        pos: usize,
    },
    Trash {
        node: usize,
    },
    Restore {
        node: usize,
    },
    Delete {
        node: usize,
    },
    EmptyTrash,
    Sync {
        from: usize,
    },
}

#[derive(Debug, Clone)]
struct Step {
    replica: usize,
    action: Action,
}

fn action() -> impl Strategy<Value = Action> {
    let idx = 0..64usize;
    prop_oneof![
        7 => (any::<bool>(), idx.clone(), idx.clone())
            .prop_map(|(page, parent, pos)| Action::Create { page, parent, pos }),
        2 => (idx.clone(), any::<u8>()).prop_map(|(node, title)| Action::Rename { node, title }),
        1 => (idx.clone(), any::<bool>()).prop_map(|(node, on)| Action::Icon { node, on }),
        1 => (idx.clone(), any::<bool>()).prop_map(|(node, on)| Action::Favorite { node, on }),
        4 => (idx.clone(), idx.clone(), idx.clone())
            .prop_map(|(node, parent, pos)| Action::Move { node, parent, pos }),
        2 => (idx.clone(), idx.clone()).prop_map(|(node, pos)| Action::Reorder { node, pos }),
        2 => idx.clone().prop_map(|node| Action::Trash { node }),
        2 => idx.clone().prop_map(|node| Action::Restore { node }),
        1 => idx.clone().prop_map(|node| Action::Delete { node }),
        1 => Just(Action::EmptyTrash),
        3 => (0..REPLICAS).prop_map(|from| Action::Sync { from }),
    ]
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        (0..REPLICAS, action()).prop_map(|(replica, action)| Step { replica, action }),
        1..60,
    )
}

fn position(n: usize) -> Position {
    if n % 5 == 4 {
        Position::End
    } else {
        Position::Index(n % 5)
    }
}

/// Failures caused by the random input (stale ids, hidden nodes, bad
/// positions) are fine; internal inconsistencies are not.
fn check<T>(result: Result<T, Error>) {
    if let Err(err) = result {
        assert!(
            !matches!(err, Error::Corrupt(_) | Error::Crdt(_)),
            "internal failure: {err}"
        );
    }
}

fn apply(replicas: &[WorkspaceTree], step: &Step) {
    let tree = &replicas[step.replica];
    let known: Vec<_> = full_state(tree).iter().map(|n| n.id).collect();
    let pick = |n: usize| (!known.is_empty()).then(|| known[n % known.len()]);
    // 0 selects the root, anything else a known node.
    let parent = |n: usize| {
        if n.is_multiple_of(known.len() + 1) {
            None
        } else {
            pick(n)
        }
    };
    match &step.action {
        Action::Create {
            page,
            parent: p,
            pos,
        } => {
            let (title, parent) = ("n", parent(*p));
            check(if *page {
                tree.create_page(parent, position(*pos), title)
            } else {
                tree.create_folder(parent, position(*pos), title)
            });
        }
        Action::Move {
            node,
            parent: p,
            pos,
        } => {
            if let Some(id) = pick(*node) {
                check(tree.move_node(id, parent(*p), position(*pos)));
            }
        }
        Action::Rename { node, title } => {
            if let Some(id) = pick(*node) {
                check(tree.rename(id, &format!("t{title}")));
            }
        }
        Action::Icon { node, on } => {
            if let Some(id) = pick(*node) {
                check(tree.set_icon(id, on.then_some("*")));
            }
        }
        Action::Favorite { node, on } => {
            if let Some(id) = pick(*node) {
                check(tree.set_favorite(id, *on));
            }
        }
        Action::Reorder { node, pos } => {
            if let Some(id) = pick(*node) {
                check(tree.reorder(id, position(*pos)));
            }
        }
        Action::Trash { node } => {
            if let Some(id) = pick(*node) {
                check(tree.trash_node(id));
            }
        }
        Action::Restore { node } => {
            if let Some(id) = pick(*node) {
                check(tree.restore(id));
            }
        }
        Action::Delete { node } => {
            if let Some(id) = pick(*node) {
                check(tree.delete_permanently(id));
            }
        }
        Action::EmptyTrash => check(tree.empty_trash()),
        Action::Sync { from } => {
            if *from != step.replica {
                pull(tree, &replicas[*from]);
                assert_forest(tree);
            }
        }
    }
}

/// A small shared hierarchy so structural operations have material to work on.
fn seed_shared_nodes(replicas: &[WorkspaceTree]) {
    let first = &replicas[0];
    let work = first
        .create_folder(None, Position::End, "work")
        .expect("work");
    let home = first
        .create_folder(None, Position::End, "home")
        .expect("home");
    let plan = first
        .create_page(Some(work), Position::End, "plan")
        .expect("plan");
    first
        .create_page(Some(plan), Position::End, "notes")
        .expect("notes");
    first
        .create_page(Some(home), Position::End, "list")
        .expect("list");
    first
        .create_page(None, Position::End, "inbox")
        .expect("inbox");
    for other in &replicas[1..] {
        pull(other, first);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn replicas_converge_to_the_same_forest(steps in steps()) {
        let replicas: Vec<WorkspaceTree> = (1..=REPLICAS as u64).map(replica).collect();
        seed_shared_nodes(&replicas);
        for step in &steps {
            apply(&replicas, step);
        }
        let all: Vec<&WorkspaceTree> = replicas.iter().collect();
        sync_all(&all);
        assert_converged(&all);
        for tree in &all {
            assert_forest(tree);
        }
    }
}
