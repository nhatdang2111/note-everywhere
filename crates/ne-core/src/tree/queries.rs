//! Read-only views of the tree.

use loro::{TreeID, TreeParentId};

use super::*;

/// What the walker does after visiting a node.
enum Walk {
    Descend,
    Skip,
}

impl WorkspaceTree {
    /// Looks a node up by id, trashed or not.
    pub fn get(&self, id: NodeId) -> Result<NodeInfo> {
        let tid = self.require_exists(id)?;
        self.info(tid)
    }

    /// Whether the node exists (it may be trashed).
    pub fn contains(&self, id: NodeId) -> bool {
        self.exists(id.tree_id())
    }

    /// Ordered children of `parent` (`None` is the root). Children that are
    /// themselves trashed are left out unless `include_trashed` is set. The
    /// children of a trashed node can be listed on request to browse the trash.
    pub fn children(&self, parent: Option<NodeId>, include_trashed: bool) -> Result<Vec<NodeInfo>> {
        if let Some(p) = parent {
            self.require_exists(p)?;
        }
        self.child_ids(Self::parent_id(parent))
            .into_iter()
            .filter(|&child| include_trashed || self.trashed_at(child).is_none())
            .map(|child| self.info(child))
            .collect()
    }

    /// Breadcrumb: the ancestors of `id` from the top-level node down to its
    /// parent. Empty for a top-level node. Works for trashed nodes.
    pub fn ancestors(&self, id: NodeId) -> Result<Vec<NodeInfo>> {
        let tid = self.require_exists(id)?;
        let mut chain = self.ancestor_chain(tid)?;
        chain.reverse();
        chain.into_iter().map(|a| self.info(a)).collect()
    }

    /// Every visible node, parents before their children and siblings in
    /// order, ready to render a sidebar. Trashed subtrees are left out.
    pub fn snapshot(&self) -> Result<Vec<NodeInfo>> {
        let mut out = Vec::new();
        let mut failure = None;
        self.walk(TreeParentId::Root, |tid| {
            if self.trashed_at(tid).is_some() {
                return Walk::Skip;
            }
            match self.info(tid) {
                Ok(info) => out.push(info),
                Err(err) => failure = Some(err),
            }
            Walk::Descend
        });
        failure.map_or(Ok(out), Err)
    }

    /// Visible favourites, in sidebar order.
    pub fn favorites(&self) -> Result<Vec<NodeInfo>> {
        Ok(self
            .snapshot()?
            .into_iter()
            .filter(|n| n.favorite)
            .collect())
    }

    /// Top-level trashed items: trashed nodes that are not inside another
    /// trashed node. Their subtrees go with them.
    pub fn trash(&self) -> Result<Vec<NodeInfo>> {
        let mut out = Vec::new();
        let mut failure = None;
        self.walk(TreeParentId::Root, |tid| {
            if self.trashed_at(tid).is_none() {
                return Walk::Descend;
            }
            match self.info(tid) {
                Ok(info) => out.push(info),
                Err(err) => failure = Some(err),
            }
            Walk::Skip
        });
        failure.map_or(Ok(out), Err)
    }

    /// Every node below `id` (children, grandchildren, ...) in pre-order,
    /// trashed ones included, excluding `id` itself. Callers use it to
    /// cascade page-document deletion.
    pub fn descendants(&self, id: NodeId) -> Result<Vec<NodeId>> {
        let tid = self.require_exists(id)?;
        let mut out = Vec::new();
        self.walk(TreeParentId::Node(tid), |child| {
            out.push(NodeId::from_tree_id(child));
            Walk::Descend
        });
        Ok(out)
    }

    /// Kind of a node; a node without a readable kind counts as a folder,
    /// like in [`NodeInfo`].
    pub(super) fn kind_of(&self, tid: TreeID) -> NodeKind {
        let kind = self.meta(tid).ok().and_then(|meta| {
            let value = meta.get_value();
            let text = value.as_map()?.get(KEY_KIND)?.as_string()?.to_string();
            NodeKind::parse(&text)
        });
        kind.unwrap_or(NodeKind::Folder)
    }

    pub(super) fn child_ids(&self, parent: TreeParentId) -> Vec<TreeID> {
        self.tree.children(parent).unwrap_or_default()
    }

    /// Depth-first pre-order walk below `start` without recursion, so very
    /// deep trees cannot overflow the stack.
    fn walk(&self, start: TreeParentId, mut visit: impl FnMut(TreeID) -> Walk) {
        let mut stack: Vec<TreeID> = self.child_ids(start);
        stack.reverse();
        while let Some(tid) = stack.pop() {
            if let Walk::Descend = visit(tid) {
                let mut kids = self.child_ids(TreeParentId::Node(tid));
                kids.reverse();
                stack.extend(kids);
            }
        }
    }

    /// Reads a node's metadata. Tolerates missing or mistyped fields, which a
    /// misbehaving peer could produce: they fall back to empty values, and a
    /// node without a readable kind is treated as a folder.
    fn info(&self, tid: TreeID) -> Result<NodeInfo> {
        let parent = match self.tree.parent(tid) {
            Some(TreeParentId::Root) => None,
            Some(TreeParentId::Node(p)) => Some(NodeId::from_tree_id(p)),
            _ => return Err(Error::NotFound(NodeId::from_tree_id(tid))),
        };
        let value = self.meta(tid)?.get_value();
        let map = value.as_map();
        let field = |key: &str| map.and_then(|m| m.get(key));
        Ok(NodeInfo {
            id: NodeId::from_tree_id(tid),
            parent,
            kind: field(KEY_KIND)
                .and_then(|v| v.as_string())
                .and_then(|s| NodeKind::parse(s))
                .unwrap_or(NodeKind::Folder),
            title: field(KEY_TITLE)
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            icon: field(KEY_ICON)
                .and_then(|v| v.as_string())
                .map(|s| s.to_string()),
            favorite: field(KEY_FAVORITE)
                .and_then(|v| v.as_bool())
                .copied()
                .unwrap_or(false),
            created_at: field(KEY_CREATED_AT)
                .and_then(|v| v.as_i64())
                .copied()
                .unwrap_or(0),
            updated_at: field(KEY_UPDATED_AT)
                .and_then(|v| v.as_i64())
                .copied()
                .unwrap_or(0),
            trashed_at: field(KEY_TRASHED_AT).and_then(|v| v.as_i64()).copied(),
        })
    }
}
