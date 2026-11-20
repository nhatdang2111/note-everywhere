//! The error every command rejects with: `{code, message}`.
//!
//! Codes are stable snake_case strings the UI switches on. Messages are fixed
//! English text chosen per code; they never embed paths, keys or note content,
//! because the underlying errors of storage and crypto may.

use ne_crypto::account::AccountError;
use ne_sync::SyncError;
use ne_vault::VaultError;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }

    pub fn not_found(what: &str) -> Self {
        Self::new("not_found", format!("{what} does not exist."))
    }

    pub fn internal(message: &str) -> Self {
        Self::new("internal", message)
    }
}

impl From<ne_store::Error> for AppError {
    fn from(err: ne_store::Error) -> Self {
        use ne_store::Error as E;
        match err {
            E::NotFound(_) | E::NoWorkspace(_) => Self::not_found("That item"),
            E::InvalidInput(message) => Self::invalid(message),
            E::Core(core) => core.into(),
            E::SchemaTooNew { .. } => Self::new(
                "schema_too_new",
                "The data on this device was written by a newer version of the app.",
            ),
            E::AlreadyExists(_) => Self::new("already_exists", "A workspace already exists."),
            E::Sqlite(_) | E::Io { .. } | E::Corrupt(_) => Self::new(
                "storage_error",
                "The local data could not be read or written.",
            ),
        }
    }
}

impl From<ne_core::Error> for AppError {
    fn from(err: ne_core::Error) -> Self {
        use ne_core::Error as E;
        match err {
            E::NotFound(_) | E::InvalidNodeId(_) => Self::not_found("That page or folder"),
            E::Trashed(_) => Self::new("trashed", "That page or folder is in the trash."),
            E::NotTrashed(_) => {
                Self::new("not_trashed", "That page or folder is not in the trash.")
            }
            E::MoveIntoDescendant { .. } => {
                Self::new("cycle", "A page or folder cannot be moved into itself.")
            }
            E::PositionOutOfRange { .. } | E::InvalidIcon { .. } | E::InvalidBlock(_) => {
                Self::invalid(err.to_string())
            }
            E::InvalidParent(_) => Self::invalid("A database holds only pages."),
            E::Invalid(message) => Self::invalid(message),
            E::Missing(_) => Self::not_found("That item"),
            E::Io(_) => Self::new("io_error", "A file could not be read or written."),
            E::Corrupt(_) | E::Crdt(_) | E::InvalidPage(_) | E::NoSuchBlock(_) => Self::new(
                "storage_error",
                "The document could not be read or applied.",
            ),
        }
    }
}

impl From<SyncError> for AppError {
    fn from(err: SyncError) -> Self {
        match err {
            SyncError::InvalidCredentials => {
                Self::new("wrong_password", "Wrong username or password.")
            }
            SyncError::Account(AccountError::WrongSecret) => {
                Self::new("wrong_password", "Wrong password.")
            }
            SyncError::Account(AccountError::EmptyPassword) => {
                Self::invalid("The password must not be empty.")
            }
            SyncError::Account(AccountError::Kdf(_)) => Self::new(
                "kdf_rejected",
                "The server asked for password settings that are not safe.",
            ),
            SyncError::Locked(seconds) => Self::new(
                "locked_out",
                format!("Too many attempts. Try again in {seconds} seconds."),
            ),
            SyncError::RegistrationClosed => Self::new(
                "registration_closed",
                "This server already has an account. Sign in instead.",
            ),
            SyncError::Forbidden => Self::new(
                "forbidden",
                "The server refused the request. Check the setup code.",
            ),
            SyncError::Unauthorized => {
                Self::new("unauthorized", "The session ended. Sign in again.")
            }
            SyncError::Network(_) | SyncError::Offline | SyncError::Stopped => {
                Self::new("offline", "The server cannot be reached.")
            }
            SyncError::Unavailable => {
                Self::new("server_unavailable", "The server is busy. Try again later.")
            }
            SyncError::NotFound => Self::not_found("That item on the server"),
            SyncError::TooLarge => Self::new("too_large", "That is too large for this server."),
            SyncError::QuotaExceeded => {
                Self::new("quota_exceeded", "The server's attachment storage is full.")
            }
            SyncError::Input(message) => Self::invalid(message),
            SyncError::Tampered => Self::new(
                "tampered",
                "A downloaded file failed its integrity check and was discarded.",
            ),
            SyncError::Store(_) | SyncError::Io(_) => Self::new(
                "storage_error",
                "The local data could not be read or written.",
            ),
            SyncError::Server { .. }
            | SyncError::Protocol(_)
            | SyncError::Crypto
            | SyncError::BadResponse => {
                Self::new("sync_error", "The server sent an unexpected answer.")
            }
        }
    }
}

impl From<VaultError> for AppError {
    fn from(err: VaultError) -> Self {
        match err {
            VaultError::Locked => Self::new("vault_locked", "The vault is locked."),
            VaultError::ReauthRequired(_) => Self::new(
                "reauth_required",
                "Enter the vault password again to continue.",
            ),
            VaultError::NotFound(_) => Self::not_found("That vault item"),
            VaultError::NoSuchField => Self::new("no_such_field", "That item has no such field."),
            VaultError::InvalidItem(reason) => Self::new("invalid_item", reason),
            VaultError::Account(AccountError::WrongSecret) => {
                Self::new("wrong_password", "Wrong vault password.")
            }
            VaultError::Account(AccountError::EmptyPassword) => {
                Self::invalid("The password must not be empty.")
            }
            VaultError::Account(AccountError::Kdf(_)) => Self::new(
                "kdf_rejected",
                "The stored password settings are not safe to use.",
            ),
            VaultError::Corrupt | VaultError::UnsupportedVersion(_) => Self::new(
                "vault_corrupt",
                "A stored vault record is damaged or from a newer version.",
            ),
            VaultError::Import(inner) => Self::new("import_failed", inner.to_string()),
            VaultError::Crypto | VaultError::Sync(_) => {
                Self::new("vault_error", "The vault could not complete that action.")
            }
        }
    }
}

impl From<ne_platform::secure_store::CredentialError> for AppError {
    fn from(_: ne_platform::secure_store::CredentialError) -> Self {
        Self::new(
            "secure_store_unavailable",
            "The system keychain could not be used.",
        )
    }
}

impl From<AccountError> for AppError {
    fn from(err: AccountError) -> Self {
        VaultError::Account(err).into()
    }
}

impl From<ne_vault::import::ImportError> for AppError {
    fn from(err: ne_vault::import::ImportError) -> Self {
        Self::new("import_failed", err.to_string())
    }
}
