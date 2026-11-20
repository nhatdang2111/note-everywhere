//! Stable textual references to vault fields, for embedding in notes.
//!
//! A reference looks like `secret://<item id>/<field>`, for example
//! `secret://0123456789abcdef0123456789abcdef/password`. The note stores only
//! the reference; the value is looked up through `Vault::resolve`, which hands
//! it out only while the vault is unlocked and recently authenticated.
//!
//! Parsing is strict: the scheme is lowercase, the id is exactly 32 lowercase
//! hex characters, the field is one of the known names, and nothing may
//! surround or follow the reference (no whitespace, query, fragment or extra
//! path segment).

use std::fmt;
use std::str::FromStr;

use zeroize::Zeroizing;

use crate::item::{Field, ItemId};

const PREFIX: &str = "secret://";

/// Shown instead of a value when the vault is locked or needs re-authentication.
pub const REDACTED_PLACEHOLDER: &str =
    "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";

/// A reference to one field of one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecretRef {
    /// The referenced item.
    pub item: ItemId,
    /// The referenced field.
    pub field: Field,
}

impl SecretRef {
    /// A reference to `field` of `item`.
    pub fn new(item: ItemId, field: Field) -> Self {
        Self { item, field }
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}/{}", self.item, self.field)
    }
}

/// Why a string is not a valid secret reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseSecretRefError {
    /// It does not start with `secret://`.
    #[error("a secret reference starts with secret://")]
    MissingScheme,
    /// It is not `<id>/<field>` after the scheme.
    #[error("a secret reference is secret://<item id>/<field>")]
    Malformed,
    /// The id is not 32 lowercase hex characters.
    #[error("the item id in the reference is invalid")]
    InvalidItemId,
    /// The field is not a known field name.
    #[error("the field in the reference is unknown")]
    UnknownField,
}

impl FromStr for SecretRef {
    type Err = ParseSecretRefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rest = s
            .strip_prefix(PREFIX)
            .ok_or(ParseSecretRefError::MissingScheme)?;
        let (id, field) = rest.split_once('/').ok_or(ParseSecretRefError::Malformed)?;
        if field.contains('/') {
            return Err(ParseSecretRefError::Malformed);
        }
        Ok(Self {
            item: id.parse().map_err(|_| ParseSecretRefError::InvalidItemId)?,
            field: field
                .parse()
                .map_err(|_| ParseSecretRefError::UnknownField)?,
        })
    }
}

/// What a reference resolves to right now.
pub enum Resolution {
    /// The secret value. Wiped on drop.
    Value(Zeroizing<String>),
    /// The vault is locked or needs re-authentication: show
    /// [`REDACTED_PLACEHOLDER`]. Says nothing about whether the item exists.
    Redacted,
    /// The vault is open but the item or field does not exist.
    Missing,
}

impl fmt::Debug for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Value(_) => "Resolution::Value(<redacted>)",
            Self::Redacted => "Resolution::Redacted",
            Self::Missing => "Resolution::Missing",
        })
    }
}

impl Resolution {
    /// The text to display: the value, or the placeholder when redacted or
    /// missing.
    pub fn display_text(&self) -> &str {
        match self {
            Self::Value(value) => value,
            Self::Redacted | Self::Missing => REDACTED_PLACEHOLDER,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn round_trips() {
        let reference: SecretRef = format!("secret://{ID}/password").parse().expect("valid");
        assert_eq!(reference.field, Field::Password);
        assert_eq!(reference.to_string(), format!("secret://{ID}/password"));
    }

    #[test]
    fn parsing_is_strict() {
        let bad = [
            format!("Secret://{ID}/password"),
            format!("secret:/{ID}/password"),
            format!("secret://{ID}"),
            format!("secret://{ID}/"),
            format!("secret://{ID}/password/"),
            format!("secret://{ID}/password/extra"),
            format!("secret://{ID}/password?x=1"),
            format!("secret://{ID}/password#frag"),
            format!("secret://{ID}/Password"),
            format!(" secret://{ID}/password"),
            format!("secret://{ID}/password "),
            format!("secret://{}/password", ID.to_uppercase()),
            format!("secret://{}/password", &ID[1..]),
            format!("secret://{ID}0/password"),
            "secret:///password".to_owned(),
            String::new(),
        ];
        for text in bad {
            assert!(text.parse::<SecretRef>().is_err(), "{text:?}");
        }
        assert_eq!(
            format!("secret://{ID}/nope").parse::<SecretRef>(),
            Err(ParseSecretRefError::UnknownField)
        );
        assert_eq!(
            "https://x".parse::<SecretRef>(),
            Err(ParseSecretRefError::MissingScheme)
        );
    }

    #[test]
    fn display_text_never_leaks_for_redacted() {
        assert_eq!(Resolution::Redacted.display_text(), REDACTED_PLACEHOLDER);
        assert_eq!(Resolution::Missing.display_text(), REDACTED_PLACEHOLDER);
        let value = Resolution::Value(Zeroizing::new("v".into()));
        assert_eq!(value.display_text(), "v");
        assert_eq!(format!("{value:?}"), "Resolution::Value(<redacted>)");
    }
}
