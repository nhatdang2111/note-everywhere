//! Collects the ids of nodes touched by local edits and imports.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use loro::event::Diff;
use loro::{ContainerID, LoroDoc, Subscription, TreeExternalDiff, TreeID, TreeParentId};

use crate::ids::NodeId;

/// Shared set of changed node ids, filled from the document's event stream.
#[derive(Clone, Default)]
pub(crate) struct ChangeLog(Arc<Mutex<BTreeSet<NodeId>>>);

impl ChangeLog {
    fn lock(&self) -> MutexGuard<'_, BTreeSet<NodeId>> {
        // A poisoned set is still a valid set of ids.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn add(&self, id: TreeID) {
        if !id.is_deleted_root() {
            self.lock().insert(NodeId::from_tree_id(id));
        }
    }

    fn add_parent(&self, parent: &TreeParentId) {
        if let TreeParentId::Node(id) = parent {
            self.add(*id);
        }
    }

    pub(crate) fn take(&self) -> BTreeSet<NodeId> {
        std::mem::take(&mut *self.lock())
    }

    /// Records every node id that the document's events mention. Node
    /// metadata lives in a map container whose id equals the node id, so map
    /// events map back to a node directly.
    pub(crate) fn subscribe(&self, doc: &LoroDoc) -> Subscription {
        let log = self.clone();
        doc.subscribe_root(Arc::new(move |event| {
            for container in &event.events {
                match &container.diff {
                    Diff::Tree(tree_diff) => {
                        for item in &tree_diff.diff {
                            log.add(item.target);
                            match &item.action {
                                TreeExternalDiff::Create { parent, .. } => log.add_parent(parent),
                                TreeExternalDiff::Move {
                                    parent, old_parent, ..
                                } => {
                                    log.add_parent(parent);
                                    log.add_parent(old_parent);
                                }
                                TreeExternalDiff::Delete { old_parent, .. } => {
                                    log.add_parent(old_parent);
                                }
                            }
                        }
                    }
                    Diff::Map(_) => {
                        if let ContainerID::Normal { peer, counter, .. } = container.target {
                            log.add(TreeID::new(*peer, *counter));
                        }
                    }
                    _ => {}
                }
            }
        }))
    }
}
