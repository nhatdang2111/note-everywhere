//! Local SQLite storage, full-text search and the sync outbox.
//!
//! [`Workspace`] is the only type the application shell needs. It owns one
//! SQLite database per workspace (in WAL mode, schema migrations applied on
//! open), the live [`ne_core::tree::WorkspaceTree`], a cache of open
//! [`ne_core::page::PageDoc`]s and a directory of attachment blobs beside the
//! database.
//!
//! # What is stored
//!
//! Source of truth: every CRDT document (the tree, each page, vault documents)
//! as a snapshot plus a log of updates that is folded into a new snapshot once
//! it grows past a threshold; the attachment files; the sync outbox, cursor and
//! settings. Everything else is derived and can be rebuilt with
//! [`Workspace::rebuild_indexes`]: the node index behind the sidebar, the
//! full-text index, the link index behind backlinks and the attachment
//! reference counts.
//!
//! # Writes
//!
//! Each mutating call is one SQLite transaction that stores the update,
//! refreshes the derived indexes incrementally, and queues the update in the
//! outbox. A crash cannot leave a half-applied change: on restart the database
//! holds either all of it or none. If a transaction fails, the in-memory tree
//! or page is reloaded from the database.
//!
//! # Search
//!
//! See [`search`] for folding (so "ghi chu" finds "ghi chú"), substring and
//! prefix matching, ranking and snippets. Trashed pages and vault documents
//! never appear.
//!
//! # Sync
//!
//! This crate does not depend on the sync crate. The shell moves bytes with
//! the plain methods [`Workspace::outbox_peek`], [`Workspace::outbox_acknowledge`],
//! [`Workspace::apply_remote_update`], [`Workspace::sync_cursor`] and
//! [`Workspace::set_sync_cursor`].

#![warn(missing_docs)]

mod blobs;
mod cache;
mod db;
mod db_index;
mod db_query;
mod docs;
mod error;
mod nodes;
mod outbox;
mod pages;
pub mod search;
mod workspace;
mod workspace_aux;
mod workspace_db;
mod workspace_history;
mod workspace_import;
mod workspace_pages;
mod workspace_reminders;
mod workspace_synced;
mod workspace_templates;
mod workspace_transfer;
mod workspace_tree;

pub use blobs::{AttachmentInfo, GcReport, is_valid_hash};
pub use db::{Durability, SCHEMA_VERSION};
pub use docs::{DocKind, DocRef};
pub use error::{Error, Result};
pub use nodes::NodeRow;
pub use outbox::OutboxItem;
pub use pages::Backlink;
pub use search::{SearchHit, Snippet, fold};
pub use workspace::{ATTACHMENTS_DIR, Change, DB_FILE, StoreConfig, VaultDoc, Workspace};
pub use workspace_reminders::ReminderEntry;
pub use workspace_synced::{Occurrence, SyncedInfo};
