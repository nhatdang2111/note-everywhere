//! Identifier types shared across the core crate.

use std::fmt;
use std::str::FromStr;

use loro::TreeID;

use crate::error::Error;

/// Stable identifier of a folder or page node in the workspace tree.
///
/// It wraps the CRDT tree id, so ids are unique across replicas without
/// coordination and never change when the node is renamed or moved. Other
/// crates use the id of a page node to locate that page's own document.
///
/// The string form is `<counter>@<peer>` and round-trips through
/// [`fmt::Display`] and [`FromStr`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(TreeID);

impl NodeId {
    pub(crate) fn from_tree_id(id: TreeID) -> Self {
        Self(id)
    }

    pub(crate) fn tree_id(self) -> TreeID {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

impl FromStr for NodeId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let id = TreeID::try_from(s).map_err(|_| Error::InvalidNodeId(s.to_owned()))?;
        // The reserved id of Loro's internal "deleted" root is never a real node.
        if id.is_deleted_root() {
            return Err(Error::InvalidNodeId(s.to_owned()));
        }
        Ok(Self(id))
    }
}
