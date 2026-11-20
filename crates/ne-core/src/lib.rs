//! Block, page and workspace-tree model built on CRDT documents.
//!
//! - [`tree`]: the workspace hierarchy of folders and pages ([`tree::WorkspaceTree`]).

#![warn(missing_docs)]

pub mod error;
pub mod ids;
pub mod tree;

pub use error::{Error, Result};
pub use ids::NodeId;
