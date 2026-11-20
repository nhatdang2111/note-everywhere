//! The error type returned by every fallible `ne-core` operation.

use crate::ids::NodeId;

/// Everything that can go wrong in `ne-core`. Bad input (unknown ids, illegal
/// moves, malformed sync bytes) is reported here and never panics.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A string is not a valid [`NodeId`].
    #[error("invalid node id: {0:?}")]
    InvalidNodeId(String),
    /// The node does not exist, or has been permanently deleted.
    #[error("node {0} not found")]
    NotFound(NodeId),
    /// The operation needs a visible node, but the node is in the trash
    /// (itself or through one of its ancestors).
    #[error("node {0} is in the trash")]
    Trashed(NodeId),
    /// The operation needs a node that is in the trash.
    #[error("node {0} is not in the trash")]
    NotTrashed(NodeId),
    /// A node cannot become a child of itself or of one of its descendants.
    #[error("cannot move node {node} under {new_parent}: it is the node itself or a descendant")]
    MoveIntoDescendant {
        /// The node being moved.
        node: NodeId,
        /// The rejected new parent.
        new_parent: NodeId,
    },
    /// The requested position is past the end of the parent's visible children.
    #[error("position {index} is out of range, the parent has {len} visible children")]
    PositionOutOfRange {
        /// The requested index.
        index: usize,
        /// The number of visible children at the destination.
        len: usize,
    },
    /// The icon is empty or longer than [`crate::tree::MAX_ICON_CHARS`].
    #[error("icon must be 1 to {max} characters, got {len}")]
    InvalidIcon {
        /// Number of characters supplied.
        len: usize,
        /// Maximum allowed.
        max: usize,
    },
    /// The tree structure is inconsistent (a parent cycle was detected).
    #[error("workspace tree is corrupt near node {0}")]
    Corrupt(NodeId),
    /// The CRDT engine rejected the operation or the bytes handed to it.
    #[error("crdt error: {0}")]
    Crdt(String),
}

impl From<loro::LoroError> for Error {
    fn from(err: loro::LoroError) -> Self {
        Self::Crdt(err.to_string())
    }
}

impl From<loro::LoroEncodeError> for Error {
    fn from(err: loro::LoroEncodeError) -> Self {
        Self::Crdt(err.to_string())
    }
}

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;
