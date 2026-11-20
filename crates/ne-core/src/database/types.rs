//! Plain data types of a database: properties, options and cell values.

use serde::{Deserialize, Serialize};

use super::date::DateValue;

/// Id of the title property. Every database has it; its value is the title of
/// the row's page node, so it is never stored in the database document.
pub const TITLE_PROPERTY: &str = "title";

/// Most properties a database can have (the title is not counted).
pub const MAX_PROPERTIES: usize = 500;

/// Longest property, option or view name, in characters.
pub const MAX_NAME_CHARS: usize = 100;

/// Colours an option can have.
pub const OPTION_COLORS: [&str; 9] = [
    "gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red",
];

/// What a property holds. The title is a property too, but a fixed one that
/// is not listed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyType {
    /// Free text.
    Text,
    /// A number.
    Number,
    /// One of the property's options.
    Select,
    /// Any number of the property's options.
    MultiSelect,
    /// A date, optionally with a time and an end.
    Date,
    /// A tick box.
    Checkbox,
    /// A web address kept as text.
    Url,
    /// Links to rows of the same or another database.
    Relation,
}

/// One choice of a select or multi-select property. Values refer to the id,
/// so renaming an option keeps every value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One of [`OPTION_COLORS`].
    pub color: String,
}

/// How a relation property is wired.
///
/// A two-way relation is two properties: the forward property, which stores
/// the links in its own database, and a *backlink* property in the target
/// database that stores nothing and is computed from the forward one. Links
/// therefore live in one place only, and two devices editing either side
/// cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationConfig {
    /// Node id of the database whose rows the links point at (for a backlink,
    /// the database that holds the forward property).
    pub database: String,
    /// On a forward property: the id of its backlink property in the target
    /// database, when the relation is two-way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_property: Option<String>,
    /// On a backlink property: the id of the forward property it mirrors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlink_of: Option<String>,
}

/// A column of the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// What the property holds.
    #[serde(rename = "type")]
    pub ty: PropertyType,
    /// Options in display order. Kept (and kept resolvable) even while the
    /// property has another type, so a conversion back loses nothing.
    pub options: Vec<SelectOption>,
    /// Present on relation properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<RelationConfig>,
}

impl Property {
    /// The option with this id.
    pub fn option(&self, id: &str) -> Option<&SelectOption> {
        self.options.iter().find(|o| o.id == id)
    }

    /// True for a relation property that is computed from a forward one.
    pub fn is_backlink(&self) -> bool {
        self.relation
            .as_ref()
            .is_some_and(|r| r.backlink_of.is_some())
    }
}

/// The value of one cell. Serialised as `{"type": ..., "value": ...}`; the
/// type tag is stored with the value so a later change of the property's type
/// can convert it, see [`super::coerce`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum CellValue {
    /// Text.
    Text(String),
    /// A finite number.
    Number(f64),
    /// An option id.
    Select(String),
    /// Option ids, in the order the person picked them.
    MultiSelect(Vec<String>),
    /// A date.
    Date(DateValue),
    /// Ticked or not.
    Checkbox(bool),
    /// A web address.
    Url(String),
    /// Row ids (page node ids) in the linked database.
    Relation(Vec<String>),
}

impl CellValue {
    /// The property type this value belongs to.
    pub fn property_type(&self) -> PropertyType {
        match self {
            Self::Text(_) => PropertyType::Text,
            Self::Number(_) => PropertyType::Number,
            Self::Select(_) => PropertyType::Select,
            Self::MultiSelect(_) => PropertyType::MultiSelect,
            Self::Date(_) => PropertyType::Date,
            Self::Checkbox(_) => PropertyType::Checkbox,
            Self::Url(_) => PropertyType::Url,
            Self::Relation(_) => PropertyType::Relation,
        }
    }

    /// True for a value that counts as an empty cell: empty text, an empty
    /// list. A number, a date and a checkbox (even unticked) are never empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(s) | Self::Url(s) | Self::Select(s) => s.is_empty(),
            Self::MultiSelect(v) | Self::Relation(v) => v.is_empty(),
            Self::Number(_) | Self::Date(_) | Self::Checkbox(_) => false,
        }
    }
}
