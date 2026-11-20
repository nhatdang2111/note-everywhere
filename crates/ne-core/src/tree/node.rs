//! Plain data types of the workspace tree: node metadata, positions, clock.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::ids::NodeId;

/// Longest allowed icon, counted in Unicode scalar values. Enough for an emoji
/// with modifiers or a joined sequence, small enough to stay "an icon".
pub const MAX_ICON_CHARS: usize = 16;

/// What a node is. The kind is fixed when the node is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A container for other nodes with no content of its own.
    Folder,
    /// A page. Its content lives in a separate document keyed by the node id.
    Page,
}

impl NodeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Page => "page",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "folder" => Some(Self::Folder),
            "page" => Some(Self::Page),
            _ => None,
        }
    }
}

/// Where to put a node among the visible children of its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// After the last visible child.
    End,
    /// Before the visible child currently at this index (`0` is first). An
    /// index equal to the number of visible children means the end.
    Index(usize),
}

/// A node's metadata as seen by the UI and by indexers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    /// Stable id.
    pub id: NodeId,
    /// Parent node, or `None` for a top-level node.
    pub parent: Option<NodeId>,
    /// Folder or page.
    pub kind: NodeKind,
    /// Display title.
    pub title: String,
    /// Optional short icon, such as an emoji.
    pub icon: Option<String>,
    /// Whether the node is pinned as a favourite.
    pub favorite: bool,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at: i64,
    /// Time of the last change to this node's own metadata or placement.
    pub updated_at: i64,
    /// Set when this node itself was trashed (its descendants are hidden with
    /// it but carry no value of their own).
    pub trashed_at: Option<i64>,
}

/// Source of timestamps in milliseconds since the Unix epoch.
///
/// Injected into [`crate::tree::WorkspaceTree`] so tests are deterministic.
/// Any `Fn() -> i64 + Send + Sync` closure is a clock.
pub trait Clock: Send + Sync {
    /// The current time in milliseconds.
    fn now_ms(&self) -> i64;
}

impl<F: Fn() -> i64 + Send + Sync> Clock for F {
    fn now_ms(&self) -> i64 {
        self()
    }
}

/// The wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0)
    }
}
