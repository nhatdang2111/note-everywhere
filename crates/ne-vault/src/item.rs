//! Vault items: what is stored, what is shown, and how drafts are validated.
//!
//! To add an item kind, add a variant to [`ItemBody`] (and to [`ItemKind`]),
//! list its fields in [`ItemBody::field`] and [`Field`] if it has new ones, and
//! extend `search_text` in the search module. Existing sealed records keep
//! decoding because the kind is a tag inside the sealed plaintext.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Result, VaultError};

/// Longest accepted title, in characters.
pub const MAX_TITLE_CHARS: usize = 256;
/// Most tags per item.
pub const MAX_TAGS: usize = 32;
/// Longest accepted tag, in characters.
pub const MAX_TAG_CHARS: usize = 64;
/// Most URLs per login.
pub const MAX_URLS: usize = 16;
/// Longest accepted URL, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// Longest accepted free-text field (password, notes, note text, key value),
/// in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_USERNAME_BYTES: usize = 512;

/// Stable, random identifier of an item. 128 bits, shown as 32 lowercase hex
/// characters. It is visible to the sync server (it is the key of the record).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId([u8; 16]);

impl ItemId {
    /// A fresh random id from the OS CSPRNG.
    ///
    /// # Panics
    /// If the operating system cannot supply randomness, like `ne-crypto`.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("operating system random number generator unavailable");
        Self(bytes)
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ItemId({self})")
    }
}

/// A string is not a valid [`ItemId`]: it must be exactly 32 lowercase hex
/// characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an item id is exactly 32 lowercase hex characters")]
pub struct ParseItemIdError;

impl FromStr for ItemId {
    type Err = ParseItemIdError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let digits = s.as_bytes();
        if digits.len() != 32 {
            return Err(ParseItemIdError);
        }
        let mut out = [0u8; 16];
        for (byte, pair) in out.iter_mut().zip(digits.chunks(2)) {
            *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Ok(Self(out))
    }
}

fn nibble(c: u8) -> std::result::Result<u8, ParseItemIdError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(ParseItemIdError),
    }
}

/// Which kind of item this is, without its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// Username and password for a site or app.
    Login,
    /// Free text kept private.
    SecureNote,
    /// An API key or access token.
    ApiKey,
}

/// A named field of an item that can be revealed, copied or referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// Login username.
    Username,
    /// Login password.
    Password,
    /// Notes of a login or API key.
    Notes,
    /// The key or token of an API key item.
    Value,
    /// The text of a secure note.
    Text,
}

impl Field {
    /// The stable lowercase name used in secret references.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Username => "username",
            Self::Password => "password",
            Self::Notes => "notes",
            Self::Value => "value",
            Self::Text => "text",
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A string is not a known field name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unknown item field")]
pub struct ParseFieldError;

impl FromStr for Field {
    type Err = ParseFieldError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        [
            Self::Username,
            Self::Password,
            Self::Notes,
            Self::Value,
            Self::Text,
        ]
        .into_iter()
        .find(|field| field.as_str() == s)
        .ok_or(ParseFieldError)
    }
}

/// The kind-specific content of an item.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemBody {
    /// A login.
    Login {
        /// Account name.
        #[serde(default)]
        username: String,
        /// Account password.
        #[serde(default)]
        password: String,
        /// Sites this login is for.
        #[serde(default)]
        urls: Vec<String>,
        /// Free notes.
        #[serde(default)]
        notes: String,
    },
    /// A secure note.
    SecureNote {
        /// The note text.
        #[serde(default)]
        text: String,
    },
    /// An API key or token.
    ApiKey {
        /// The key or token.
        #[serde(default)]
        value: String,
        /// Free notes.
        #[serde(default)]
        notes: String,
    },
}

impl ItemBody {
    /// The kind of this body.
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Login { .. } => ItemKind::Login,
            Self::SecureNote { .. } => ItemKind::SecureNote,
            Self::ApiKey { .. } => ItemKind::ApiKey,
        }
    }

    /// The text of `field`, or `None` when this kind has no such field.
    pub fn field(&self, field: Field) -> Option<&str> {
        match (self, field) {
            (Self::Login { username, .. }, Field::Username) => Some(username),
            (Self::Login { password, .. }, Field::Password) => Some(password),
            (Self::Login { notes, .. } | Self::ApiKey { notes, .. }, Field::Notes) => Some(notes),
            (Self::ApiKey { value, .. }, Field::Value) => Some(value),
            (Self::SecureNote { text }, Field::Text) => Some(text),
            _ => None,
        }
    }

    fn text_fields(&self) -> Vec<&str> {
        match self {
            Self::Login {
                username,
                password,
                notes,
                ..
            } => vec![password, notes, username],
            Self::SecureNote { text } => vec![text],
            Self::ApiKey { value, notes } => vec![value, notes],
        }
    }
}

impl fmt::Debug for ItemBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ItemBody::{:?}(<redacted>)", self.kind())
    }
}

/// What a caller supplies to create or replace an item. Wiped on drop.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ItemDraft {
    /// Display title; required.
    pub title: String,
    /// Free-form labels.
    pub tags: Vec<String>,
    /// The kind-specific content.
    pub body: ItemBody,
}

impl fmt::Debug for ItemDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ItemDraft(<redacted>)")
    }
}

impl ItemDraft {
    /// Trims the title and tags, drops empty and duplicate tags and empty
    /// URLs, and enforces the size limits.
    pub fn normalized(mut self) -> Result<Self> {
        self.title = self.title.trim().to_owned();
        if self.title.is_empty() {
            return Err(VaultError::InvalidItem("the title is empty"));
        }
        if self.title.chars().count() > MAX_TITLE_CHARS {
            return Err(VaultError::InvalidItem("the title is too long"));
        }
        let mut tags: Vec<String> = Vec::with_capacity(self.tags.len());
        for tag in self.tags.drain(..) {
            let tag = tag.trim().to_owned();
            if tag.is_empty() || tags.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
                continue;
            }
            if tag.chars().count() > MAX_TAG_CHARS {
                return Err(VaultError::InvalidItem("a tag is too long"));
            }
            tags.push(tag);
        }
        if tags.len() > MAX_TAGS {
            return Err(VaultError::InvalidItem("too many tags"));
        }
        self.tags = tags;
        if let ItemBody::Login { urls, username, .. } = &mut self.body {
            urls.iter_mut().for_each(|u| *u = u.trim().to_owned());
            urls.retain(|u| !u.is_empty());
            if urls.len() > MAX_URLS || urls.iter().any(|u| u.len() > MAX_URL_BYTES) {
                return Err(VaultError::InvalidItem("too many or too long URLs"));
            }
            if username.len() > MAX_USERNAME_BYTES {
                return Err(VaultError::InvalidItem("the username is too long"));
            }
        }
        if self
            .body
            .text_fields()
            .iter()
            .any(|t| t.len() > MAX_TEXT_BYTES)
        {
            return Err(VaultError::InvalidItem("a text field is too long"));
        }
        Ok(self)
    }
}

/// The plaintext that is sealed: everything about an item except its id.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct ItemContent {
    pub title: String,
    pub tags: Vec<String>,
    pub body: ItemBody,
    #[zeroize(skip)]
    pub created_ms: i64,
    #[zeroize(skip)]
    pub updated_ms: i64,
}

impl ItemContent {
    pub fn new(draft: ItemDraft, now_ms: i64) -> Self {
        Self::from_draft(draft, now_ms, now_ms)
    }

    pub fn from_draft(draft: ItemDraft, created_ms: i64, updated_ms: i64) -> Self {
        let mut draft = draft;
        Self {
            title: std::mem::take(&mut draft.title),
            tags: std::mem::take(&mut draft.tags),
            body: draft.body.clone(),
            created_ms,
            updated_ms,
        }
    }

    /// Upper bound of the serialised size, used to pre-size the buffer so it
    /// does not reallocate (and leave an unwiped copy behind).
    pub fn size_hint(&self) -> usize {
        let text: usize = self.body.text_fields().iter().map(|t| t.len()).sum();
        let urls = match &self.body {
            ItemBody::Login { urls, .. } => urls.iter().map(String::len).sum(),
            _ => 0,
        };
        let tags: usize = self.tags.iter().map(String::len).sum();
        (text + urls + tags + self.title.len()) * 2 + 512
    }
}

/// An item as shown in lists and detail views: no secret fields. Use
/// `Vault::reveal` or `Vault::copy` to get password, key, note text or notes.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ItemView {
    /// The item id.
    #[zeroize(skip)]
    pub id: ItemId,
    /// The item kind.
    #[zeroize(skip)]
    pub kind: ItemKind,
    /// Display title.
    pub title: String,
    /// Labels.
    pub tags: Vec<String>,
    /// Login username; `None` for other kinds.
    pub username: Option<String>,
    /// Login URLs; empty for other kinds.
    pub urls: Vec<String>,
    /// Creation time, ms since the Unix epoch.
    #[zeroize(skip)]
    pub created_ms: i64,
    /// Last update time, ms since the Unix epoch.
    #[zeroize(skip)]
    pub updated_ms: i64,
}

impl fmt::Debug for ItemView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ItemView({}, {:?}, <redacted>)", self.id, self.kind)
    }
}

impl ItemView {
    pub(crate) fn of(id: ItemId, content: &ItemContent) -> Self {
        let (username, urls) = match &content.body {
            ItemBody::Login { username, urls, .. } => (Some(username.clone()), urls.clone()),
            _ => (None, Vec::new()),
        };
        Self {
            id,
            kind: content.body.kind(),
            title: content.title.clone(),
            tags: content.tags.clone(),
            username,
            urls,
            created_ms: content.created_ms,
            updated_ms: content.updated_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(title: &str) -> ItemDraft {
        ItemDraft {
            title: title.into(),
            tags: vec![" Work ".into(), "work".into(), "".into(), "ops".into()],
            body: ItemBody::Login {
                username: "u".into(),
                password: "p".into(),
                urls: vec![" https://example.test ".into(), "  ".into()],
                notes: String::new(),
            },
        }
    }

    #[test]
    fn item_id_round_trips_and_parsing_is_strict() {
        let id = ItemId::generate();
        assert_eq!(id.to_string().parse::<ItemId>(), Ok(id));
        let text = id.to_string();
        assert!(format!("{text}0").parse::<ItemId>().is_err());
        assert!(text[..31].parse::<ItemId>().is_err());
        assert!(format!("{}g", &text[..31]).parse::<ItemId>().is_err());
        assert!(format!(" {}", &text[1..]).parse::<ItemId>().is_err());
    }

    #[test]
    fn uppercase_hex_is_rejected() {
        assert!("A".repeat(32).parse::<ItemId>().is_err());
    }

    #[test]
    fn normalisation_trims_and_dedupes() {
        let draft = login("  Mail  ").normalized().expect("valid");
        assert_eq!(draft.title, "Mail");
        assert_eq!(draft.tags, ["Work", "ops"]);
        let ItemBody::Login { urls, .. } = &draft.body else {
            panic!("login")
        };
        assert_eq!(urls, &["https://example.test"]);
    }

    #[test]
    fn invalid_drafts_are_rejected() {
        assert!(login("   ").normalized().is_err());
        assert!(
            login(&"x".repeat(MAX_TITLE_CHARS + 1))
                .normalized()
                .is_err()
        );
        let mut many = login("t");
        many.tags = (0..=MAX_TAGS).map(|i| format!("tag{i}")).collect();
        assert!(many.normalized().is_err());
        let huge = ItemDraft {
            title: "t".into(),
            tags: vec![],
            body: ItemBody::SecureNote {
                text: "x".repeat(MAX_TEXT_BYTES + 1),
            },
        };
        assert!(huge.normalized().is_err());
    }

    #[test]
    fn field_lookup_matches_kind() {
        let body = ItemBody::ApiKey {
            value: "k".into(),
            notes: "n".into(),
        };
        assert_eq!(body.field(Field::Value), Some("k"));
        assert_eq!(body.field(Field::Notes), Some("n"));
        assert_eq!(body.field(Field::Password), None);
        for field in [
            Field::Username,
            Field::Password,
            Field::Notes,
            Field::Value,
            Field::Text,
        ] {
            assert_eq!(field.as_str().parse::<Field>(), Ok(field));
        }
        assert!("Password".parse::<Field>().is_err());
    }

    #[test]
    fn debug_output_hides_contents() {
        let draft = login("Secret title");
        let text = format!("{draft:?} {:?}", draft.body);
        assert!(!text.contains("Secret title"));
        assert!(text.contains("redacted"));
    }
}
