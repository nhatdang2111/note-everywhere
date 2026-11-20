//! Shared helpers for the `tree_*` integration tests. Included with
//! `mod tree_support;` from each test file.
#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use ne_core::tree::{NodeInfo, WorkspaceTree};

/// A replica with a fixed peer id and a deterministic clock (one tick per call).
pub fn replica(peer: u64) -> WorkspaceTree {
    let ticks = AtomicI64::new(i64::try_from(peer).expect("small peer id") * 1_000_000);
    WorkspaceTree::with_peer_id(peer, Arc::new(move || ticks.fetch_add(1, Ordering::SeqCst)))
        .expect("valid peer id")
}

/// Applies everything `from` has that `to` lacks.
pub fn pull(to: &WorkspaceTree, from: &WorkspaceTree) {
    let bytes = from
        .export_updates_since(&to.version())
        .expect("export updates");
    let outcome = to.import(&bytes).expect("import updates");
    assert!(!outcome.has_missing_dependencies, "update had a gap");
}

/// Two-way exchange between two replicas.
pub fn sync(a: &WorkspaceTree, b: &WorkspaceTree) {
    pull(a, b);
    pull(b, a);
}

/// Brings every replica to the union of all histories.
pub fn sync_all(replicas: &[&WorkspaceTree]) {
    for _ in 0..2 {
        for (i, to) in replicas.iter().enumerate() {
            for (j, from) in replicas.iter().enumerate() {
                if i != j {
                    pull(to, from);
                }
            }
        }
    }
}

/// Visible nodes followed by every trashed subtree, in a deterministic order,
/// so two replicas can be compared field by field.
pub fn full_state(tree: &WorkspaceTree) -> Vec<NodeInfo> {
    let mut state = tree.snapshot().expect("snapshot");
    for top in tree.trash().expect("trash") {
        state.push(top.clone());
        for id in tree.descendants(top.id).expect("descendants") {
            state.push(tree.get(id).expect("descendant exists"));
        }
    }
    state
}

pub fn assert_converged(replicas: &[&WorkspaceTree]) {
    let first = full_state(replicas[0]);
    for other in &replicas[1..] {
        assert_eq!(first, full_state(other), "replicas diverged");
    }
}

/// Every node's parent chain ends at the root and every parent is itself a
/// listed node: a forest with no cycles and no orphans.
pub fn assert_forest(tree: &WorkspaceTree) {
    let state = full_state(tree);
    let known: std::collections::HashSet<_> = state.iter().map(|n| n.id).collect();
    for node in &state {
        if let Some(parent) = node.parent {
            assert!(known.contains(&parent), "orphan {:?}", node.id);
        }
        // `ancestors` fails on a cycle instead of looping.
        let chain = tree.ancestors(node.id).expect("acyclic ancestors");
        assert!(chain.len() < state.len(), "chain longer than the tree");
    }
}

pub fn titles(nodes: &[NodeInfo]) -> Vec<&str> {
    nodes.iter().map(|n| n.title.as_str()).collect()
}
