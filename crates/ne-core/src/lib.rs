//! Block, page and workspace-tree model built on CRDT documents.
//!
//! - [`markdown`]: Markdown import and export of pages and subtrees.
//! - [`page`]: the content of one page ([`page::PageDoc`]).
//! - [`tree`]: the workspace hierarchy of folders and pages ([`tree::WorkspaceTree`]).

#![warn(missing_docs)]

pub mod collation;
pub mod database;
pub mod error;
pub mod ids;
mod kv_doc;
pub mod markdown;
pub mod page;
pub mod reminders;
pub mod roles;
pub mod synced;
pub mod template;
pub mod tree;

pub use error::{Error, Result};
pub use ids::NodeId;
