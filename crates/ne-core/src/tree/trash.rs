//! Soft delete, restore and permanent deletion.

use super::*;

impl WorkspaceTree {
    /// Moves a node and its whole subtree to the trash. The subtree stays
    /// attached and is hidden together with the node.
    pub fn trash_node(&self, id: NodeId) -> Result<()> {
        let tid = self.require_visible(id)?;
        let meta = self.meta(tid)?;
        let result = meta
            .insert(KEY_TRASHED_AT, self.now())
            .map_err(Error::from)
            .and_then(|()| self.touch(&meta));
        self.commit();
        result
    }

    /// Takes a trashed node (and its subtree) out of the trash. It returns to
    /// its old place; if an ancestor is still trashed, or was trashed in the
    /// meantime, it goes to the end of the root instead so it is visible.
    pub fn restore(&self, id: NodeId) -> Result<()> {
        let tid = self.require_exists(id)?;
        if self.trashed_at(tid).is_none() {
            return Err(Error::NotTrashed(id));
        }
        let meta = self.meta(tid)?;
        let mut result = meta.delete(KEY_TRASHED_AT).map_err(Error::from);
        if result.is_ok() {
            result = self.ensure_visible_place(tid);
        }
        let result = result.and_then(|()| self.touch(&meta));
        self.commit();
        result
    }

    /// Removes a trashed node and its whole subtree for good. Returns every
    /// removed id (the node first, then its descendants in pre-order) so the
    /// caller can delete the matching page documents.
    pub fn delete_permanently(&self, id: NodeId) -> Result<Vec<NodeId>> {
        let removed = self.collect_deletable(id)?;
        let result = self.tree.delete(id.tree_id()).map_err(Error::from);
        self.commit();
        result.map(|()| removed)
    }

    /// Permanently removes everything in the trash. Returns every removed id,
    /// descendants included.
    pub fn empty_trash(&self) -> Result<Vec<NodeId>> {
        let mut removed = Vec::new();
        let mut outcome = Ok(());
        for item in self.trash()? {
            let ids = self.collect_deletable(item.id).and_then(|ids| {
                self.tree.delete(item.id.tree_id())?;
                Ok(ids)
            });
            match ids {
                Ok(ids) => removed.extend(ids),
                Err(err) => {
                    outcome = Err(err);
                    break;
                }
            }
        }
        self.commit();
        outcome.map(|()| removed)
    }

    /// The node plus its descendants, if the node is in the trash.
    fn collect_deletable(&self, id: NodeId) -> Result<Vec<NodeId>> {
        let tid = self.require_exists(id)?;
        let hidden = self.trashed_at(tid).is_some()
            || self
                .ancestor_chain(tid)?
                .into_iter()
                .any(|a| self.trashed_at(a).is_some());
        if !hidden {
            return Err(Error::NotTrashed(id));
        }
        let mut ids = vec![id];
        ids.extend(self.descendants(id)?);
        Ok(ids)
    }

    /// Moves a just-restored node to the root when an ancestor is trashed.
    fn ensure_visible_place(&self, tid: TreeID) -> Result<()> {
        let blocked = self
            .ancestor_chain(tid)?
            .into_iter()
            .any(|a| self.trashed_at(a).is_some());
        if blocked {
            let end = self.slot(TreeParentId::Root, Position::End, Some(tid))?;
            self.tree.mov_to(tid, TreeParentId::Root, end)?;
        }
        Ok(())
    }
}
