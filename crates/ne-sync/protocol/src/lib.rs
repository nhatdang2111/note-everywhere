//! Wire formats shared by the Note Everywhere sync client and server: the
//! binary WebSocket frames, the JSON bodies of the HTTP API, identifier rules
//! and size limits.

pub mod frame;
pub mod ids;
pub mod limits;
pub mod wire;

pub use frame::{
    CLIENT_ID_LEN, ClientFrame, ClientId, EntryKind, ErrorCode, LogEntry, ProtocolError, PushEntry,
    ServerFrame, SnapshotOutcome, VERSION,
};
