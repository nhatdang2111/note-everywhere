//! Local edits: create, rename, icon, favourite, move, reorder.
//!
//! Every operation commits before returning, so each one is a single
//! undoable, syncable change. Edits only apply to visible nodes; a trashed
//! node must be restored first.

use super::*;

impl WorkspaceTree {
    /// Creates a folder under `parent` (`None` is the root) at `position`.
    pub fn create_folder(
        &self,
        parent: Option<NodeId>,
        position: Position,
        title: &str,
    ) -> Result<NodeId> {
        self.create(NodeKind::Folder, parent, position, title)
    }

    /// Creates a page node under `parent` (`None` is the root) at `position`.
    /// The page's content document is created separately, keyed by the id.
    pub fn create_page(
        &self,
        parent: Option<NodeId>,
        position: Position,
        title: &str,
    ) -> Result<NodeId> {
        self.create(NodeKind::Page, parent, position, title)
    }

    /// Changes a node's title.
    pub fn rename(&self, id: NodeId, title: &str) -> Result<()> {
        self.edit_meta(id, |meta| Ok(meta.insert(KEY_TITLE, title)?))
    }

    /// Sets or clears (`None`) a node's icon. An icon is 1 to
    /// [`MAX_ICON_CHARS`] characters.
    pub fn set_icon(&self, id: NodeId, icon: Option<&str>) -> Result<()> {
        if let Some(icon) = icon {
            let len = icon.chars().count();
            if len == 0 || len > MAX_ICON_CHARS {
                return Err(Error::InvalidIcon {
                    len,
                    max: MAX_ICON_CHARS,
                });
            }
        }
        self.edit_meta(id, |meta| {
            match icon {
                Some(icon) => meta.insert(KEY_ICON, icon)?,
                None => meta.delete(KEY_ICON)?,
            }
            Ok(())
        })
    }

    /// Pins or unpins a node as a favourite.
    pub fn set_favorite(&self, id: NodeId, favorite: bool) -> Result<()> {
        self.edit_meta(id, |meta| Ok(meta.insert(KEY_FAVORITE, favorite)?))
    }

    /// Moves a node under `new_parent` (`None` is the root) at `position`.
    /// Moving a node into itself or one of its descendants is rejected with
    /// [`Error::MoveIntoDescendant`].
    pub fn move_node(
        &self,
        id: NodeId,
        new_parent: Option<NodeId>,
        position: Position,
    ) -> Result<()> {
        let tid = self.require_visible(id)?;
        let parent = self.require_visible_parent(new_parent)?;
        if let Some(target) = new_parent {
            let target_tid = target.tree_id();
            if target_tid == tid || self.ancestor_chain(target_tid)?.contains(&tid) {
                return Err(Error::MoveIntoDescendant {
                    node: id,
                    new_parent: target,
                });
            }
        }
        let index = self.slot(parent, position, Some(tid))?;
        let result = self.tree.mov_to(tid, parent, index).map_err(Error::from);
        let result = result.and_then(|()| self.touch(&self.meta(tid)?));
        self.commit();
        result
    }

    /// Moves a node among its siblings, keeping its parent.
    pub fn reorder(&self, id: NodeId, position: Position) -> Result<()> {
        let tid = self.require_visible(id)?;
        let parent = match self.tree.parent(tid) {
            Some(parent @ (TreeParentId::Root | TreeParentId::Node(_))) => parent,
            _ => return Err(Error::NotFound(id)),
        };
        let index = self.slot(parent, position, Some(tid))?;
        let result = self.tree.mov_to(tid, parent, index).map_err(Error::from);
        let result = result.and_then(|()| self.touch(&self.meta(tid)?));
        self.commit();
        result
    }

    fn create(
        &self,
        kind: NodeKind,
        parent: Option<NodeId>,
        position: Position,
        title: &str,
    ) -> Result<NodeId> {
        let parent = self.require_visible_parent(parent)?;
        let index = self.slot(parent, position, None)?;
        let tid = self.tree.create_at(parent, index)?;
        let now = self.now();
        let result = self.meta(tid).and_then(|meta| {
            meta.insert(KEY_KIND, kind.as_str())?;
            meta.insert(KEY_TITLE, title)?;
            meta.insert(KEY_CREATED_AT, now)?;
            meta.insert(KEY_UPDATED_AT, now)?;
            Ok(())
        });
        self.commit();
        result.map(|()| NodeId::from_tree_id(tid))
    }

    /// Runs `edit` on the metadata of a visible node, bumps `updated_at`, and
    /// commits.
    fn edit_meta(&self, id: NodeId, edit: impl FnOnce(&LoroMap) -> Result<()>) -> Result<()> {
        let tid = self.require_visible(id)?;
        let meta = self.meta(tid)?;
        let result = edit(&meta).and_then(|()| self.touch(&meta));
        self.commit();
        result
    }

    /// Translates a position among visible children of `parent` into the
    /// index Loro expects among all its children. `moving` is a node about to
    /// leave its slot, so it is ignored when counting.
    pub(super) fn slot(
        &self,
        parent: TreeParentId,
        position: Position,
        moving: Option<TreeID>,
    ) -> Result<usize> {
        let siblings: Vec<TreeID> = self
            .child_ids(parent)
            .into_iter()
            .filter(|&child| Some(child) != moving)
            .collect();
        let visible: Vec<usize> = siblings
            .iter()
            .enumerate()
            .filter(|&(_, &child)| self.trashed_at(child).is_none())
            .map(|(index, _)| index)
            .collect();
        match position {
            Position::End => Ok(siblings.len()),
            Position::Index(i) if i == visible.len() => Ok(siblings.len()),
            Position::Index(i) => visible.get(i).copied().ok_or(Error::PositionOutOfRange {
                index: i,
                len: visible.len(),
            }),
        }
    }
}
