//! Client for the end-to-end encrypted sync protocol.
//!
//! * [`SyncClient`] signs up, logs in, manages sessions and key records, and
//!   transfers encrypted attachments. Callers pass passwords and receive
//!   unlocked keys; the server never sees either.
//! * [`SyncEngine`] keeps a workspace in step with the server: it seals local
//!   updates, pushes them in batches, applies what other devices send, and
//!   reconnects on its own. Persistence is behind [`SyncStore`], which the
//!   storage crate implements; [`MemoryStore`] is provided for tests.
//! * [`SyncStatus`] is the simple state the UI shows.
//!
//! The wire formats live in `ne-sync-protocol` and are described in
//! `docs/sync-protocol.md`.

mod attachments;
mod backoff;
mod client;
mod engine;
mod error;
mod parts;
mod random;
mod runner;
mod status;
mod store;
mod tls;
mod transport;

pub use attachments::BlobRef;
pub use backoff::Backoff;
pub use client::{ClientConfig, SignUp, SyncClient, Unlocked};
pub use engine::{EngineConfig, SyncEngine};
pub use error::{Result, SyncError};
pub use ne_sync_protocol::SnapshotOutcome;
pub use status::{SyncFailure, SyncStatus};
pub use store::{
    IncomingBatch, IncomingKind, IncomingUpdate, MemoryStore, OutboxItem, StoreError, StoreResult,
    SyncStore,
};
pub use transport::{SessionState, SessionToken};
