//! Secrets vault protected by a second password.
//!
//! The vault keeps logins, secure notes and API keys. Its key is independent of
//! the account: it is wrapped only by the second password and the vault
//! recovery key (see [`ne_crypto::account`]), so neither the account password
//! nor an unlocked app reveals the contents.
//!
//! # Pieces
//!
//! * [`VaultDoc`]: the synchronised state, one Loro document in which every
//!   item is a single sealed record. Without the key only ids, sealed bytes,
//!   an update counter and deletions are visible. Syncs through the same
//!   channel as notes and converges without the key.
//! * [`Vault`]: the lock state machine and item operations. The key and the
//!   decrypted items exist only while unlocked; [`Vault::lock`] wipes them.
//!   [`LockPolicy`] decides auto-lock from caller-supplied [`LockEvent`]s and
//!   when sensitive actions ask for the password again.
//! * [`generator`]: random passwords and passphrases with entropy reporting.
//! * [`SecretRef`]: `secret://<item id>/<field>` references notes can embed.
//! * [`import`]: CSV import from Bitwarden, 1Password, LastPass or any
//!   header-mapped file, and the matching export.
//! * [`reuses_account_password`]: the second-password hygiene check.
//!
//! # Conflict rules
//!
//! Two replicas editing different items keep both edits. Two edits of the same
//! item converge to the whole version written last (Loro's last-writer-wins on
//! one map value); ciphertexts are never mixed. A deletion beats every edit of
//! that item. Items are never re-created under a deleted id.
//!
//! # Limits
//!
//! Zeroizing is best effort (see the security model). Plaintext length is not
//! hidden: sealed sizes reveal roughly how long a record is.

#![deny(missing_docs)]

mod clock;
mod csv;
mod doc;
mod error;
pub mod generator;
mod hygiene;
pub mod import;
mod item;
mod lock;
mod record;
mod search;
mod secret_ref;
mod vault;
mod vault_items;

pub use clock::{Clock, SystemClock};
pub use doc::{ImportOutcome, VaultDoc};
pub use error::{Result, VaultError};
pub use hygiene::reuses_account_password;
pub use import::{ColumnMapping, CsvFormat, ImportReport, SkipReason, SkippedRow};
pub use item::{
    Field, ItemBody, ItemDraft, ItemId, ItemKind, ItemView, MAX_TAG_CHARS, MAX_TAGS,
    MAX_TEXT_BYTES, MAX_TITLE_CHARS, MAX_URL_BYTES, MAX_URLS, ParseFieldError, ParseItemIdError,
};
pub use lock::{LockEvent, LockPolicy, LockState, SensitiveAction};
pub use record::FORMAT_VERSION;
pub use secret_ref::{ParseSecretRefError, REDACTED_PLACEHOLDER, Resolution, SecretRef};
pub use vault::Vault;
pub use vault_items::ClipboardPayload;
