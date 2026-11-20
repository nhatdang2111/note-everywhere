//! What a stored document is, for a device that receives it for the first time.
//!
//! Besides the workspace tree and plain pages, the store holds documents that
//! travel through sync under the id of a node: a database, the template
//! library, the reminders and the shared content of a synced block. Each of
//! them carries a root map `ne` with a `role` string, written by its creator,
//! so the receiver can open it with the right type. A document without the
//! marker is a plain page.

use loro::{LoroDoc, ValueOrContainer};

/// Name of the root map that holds the marker.
pub const ROLE_ROOT: &str = "ne";
/// Key of the role inside it.
pub const KEY_ROLE: &str = "role";

/// What a document is, other than a plain page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A database: schema, cells and views.
    Database,
    /// The template library.
    Library,
    /// The reminders.
    Reminders,
    /// The shared content of a synced block (laid out like a page).
    Synced,
}

impl Role {
    /// The marker text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Library => "library",
            Self::Reminders => "reminders",
            Self::Synced => "synced",
        }
    }

    /// The role a marker names.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "database" => Some(Self::Database),
            "library" => Some(Self::Library),
            "reminders" => Some(Self::Reminders),
            "synced" => Some(Self::Synced),
            _ => None,
        }
    }
}

/// The role of the document that `bytes` (a snapshot or its first update)
/// describe; `None` for a plain page or for bytes that do not decode.
pub fn role_of(bytes: &[u8]) -> Option<Role> {
    let doc = LoroDoc::new();
    doc.import(bytes).ok()?;
    let value = doc.get_map(ROLE_ROOT).get(KEY_ROLE)?;
    match value {
        ValueOrContainer::Value(v) => Role::parse(&v.into_string().ok()?),
        ValueOrContainer::Container(_) => None,
    }
}

/// Writes the marker into a document.
pub(crate) fn write_role(doc: &LoroDoc, role: Role) -> crate::Result<()> {
    doc.get_map(ROLE_ROOT).insert(KEY_ROLE, role.as_str())?;
    Ok(())
}
