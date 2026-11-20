//! Errors of the sync client.

use ne_crypto::account::AccountError;
use ne_sync_protocol::ProtocolError;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The server could not be reached or the connection broke.
    #[error("network error: {0}")]
    Network(String),
    /// The server is up but cannot serve the request right now (including
    /// free-plan limits); retrying later is expected to work.
    #[error("server unavailable")]
    Unavailable,
    #[error("wrong username or password")]
    InvalidCredentials,
    /// Too many failed attempts; the number is the seconds to wait.
    #[error("too many attempts, try again in {0} seconds")]
    Locked(u64),
    #[error("registration is closed on this server")]
    RegistrationClosed,
    /// The server refused the request (wrong setup code, not an app client).
    #[error("request refused by the server")]
    Forbidden,
    /// There is no session, or it expired or was revoked.
    #[error("not signed in or the session ended")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error("too large for this server")]
    TooLarge,
    #[error("the server's attachment quota is full")]
    QuotaExceeded,
    #[error("the server rejected the request ({status}, {code})")]
    Server { status: u16, code: String },
    #[error("account error: {0}")]
    Account(#[from] AccountError),
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("local storage error: {0}")]
    Store(String),
    #[error("encryption failed")]
    Crypto,
    /// A downloaded attachment does not match its address or fails
    /// authentication; it has been discarded.
    #[error("attachment is not authentic")]
    Tampered,
    #[error("the server sent something unexpected")]
    BadResponse,
    #[error("input error: {0}")]
    Input(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The sync engine is not connected, so the request cannot be served now.
    #[error("sync engine is offline")]
    Offline,
    /// The sync engine has stopped.
    #[error("sync engine has stopped")]
    Stopped,
}

pub type Result<T> = std::result::Result<T, SyncError>;

impl SyncError {
    /// Whether the same request may succeed later without any change.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Unavailable | Self::Offline)
    }
}

impl From<reqwest::Error> for SyncError {
    fn from(err: reqwest::Error) -> Self {
        // The message can contain the URL; it holds no secret (tokens travel
        // in headers) but is trimmed to its kind to stay generic.
        let kind = if err.is_timeout() {
            "timeout"
        } else if err.is_connect() {
            "connection failed"
        } else if err.is_body() || err.is_decode() {
            "bad body"
        } else {
            "request failed"
        };
        Self::Network(kind.to_owned())
    }
}

impl From<serde_json::Error> for SyncError {
    fn from(_: serde_json::Error) -> Self {
        Self::BadResponse
    }
}
