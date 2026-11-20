//! The error type of the store.

use std::path::PathBuf;

/// Everything that can go wrong in `ne-store`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A document, node, or other model operation failed in `ne-core`.
    #[error(transparent)]
    Core(#[from] ne_core::Error),
    /// SQLite reported an error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Reading or writing a file failed.
    #[error("i/o error at {path}: {message}")]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// The operating system's message.
        message: String,
    },
    /// The database was written by a newer version of the application.
    #[error("database schema version {found} is newer than the supported version {supported}")]
    SchemaTooNew {
        /// Version recorded in the file.
        found: i64,
        /// Newest version this build understands.
        supported: i64,
    },
    /// The workspace directory already holds a workspace.
    #[error("a workspace already exists at {0}")]
    AlreadyExists(PathBuf),
    /// The directory holds no workspace.
    #[error("no workspace at {0}")]
    NoWorkspace(PathBuf),
    /// The requested document, page or attachment is not stored.
    #[error("{0} not found")]
    NotFound(String),
    /// The caller passed something the operation cannot accept.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Stored data is inconsistent (an attachment file with the wrong size,
    /// a document that does not decode).
    #[error("stored data is corrupt: {0}")]
    Corrupt(String),
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, err: &std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            message: err.to_string(),
        }
    }
}

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;
