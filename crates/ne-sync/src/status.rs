//! The simple status the UI shows.

/// Why sync stopped and will not resume on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncFailure {
    /// The session ended (signed out elsewhere, expired); sign in again.
    Unauthorized,
    /// The server's log is behind this device's position, as if the server
    /// was wiped or restored from an old backup.
    ServerReset,
    /// A row from the server failed to decrypt or authenticate.
    CorruptData,
    /// The server and client disagree about the protocol.
    Protocol,
    /// Local storage refused a write.
    Storage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    /// Connected, caught up, nothing waiting to be sent.
    Synced,
    /// Connected but catching up or sending; `outbox` updates are waiting.
    Pending { outbox: usize },
    /// Not connected; retrying with backoff. Local edits are queued.
    Offline,
    /// Stopped; see [`SyncFailure`].
    Error(SyncFailure),
}
