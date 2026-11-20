//! The request and the answer of a database query. The store runs it over its
//! SQLite index; these types only describe it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::types::CellValue;
use super::view::{FilterNode, GroupConfig, SortKey, ViewDef};

/// Rows returned by default.
pub const DEFAULT_LIMIT: usize = 100;
/// Most rows one request returns (per group when grouped).
pub const MAX_LIMIT: usize = 1000;

/// Days (inclusive, `YYYY-MM-DD`) a calendar asks about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    /// First day.
    pub from: String,
    /// Last day.
    pub to: String,
}

/// What to ask a database. A view's own settings are the usual source, see
/// [`QuerySpec::from_view`]; a request may also be built from unsaved settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySpec {
    /// Which rows match. Conditions on a property that no longer exists, or
    /// whose operator no longer fits the property's type, are ignored.
    #[serde(default)]
    pub filter: Option<FilterNode>,
    /// Order, most significant key first; ties fall back to the manual order of
    /// the rows (their order in the database).
    #[serde(default)]
    pub sorts: Vec<SortKey>,
    /// Group into buckets (board columns); then the rows come back in
    /// `groups`.
    #[serde(default)]
    pub group: Option<GroupConfig>,
    /// With `date_range`: the date property rows are placed by.
    #[serde(default)]
    pub date_property: Option<String>,
    /// Keep only rows whose date (or date range) touches these days.
    #[serde(default)]
    pub date_range: Option<DateRange>,
    /// Rows to skip (ignored when grouped).
    #[serde(default)]
    pub offset: usize,
    /// Rows to return, at most [`MAX_LIMIT`]; per group when grouped.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

impl Default for QuerySpec {
    fn default() -> Self {
        Self {
            filter: None,
            sorts: Vec::new(),
            group: None,
            date_property: None,
            date_range: None,
            offset: 0,
            limit: DEFAULT_LIMIT,
        }
    }
}

impl QuerySpec {
    /// The query a saved view asks for.
    pub fn from_view(view: &ViewDef) -> Self {
        Self {
            filter: view.filter.clone(),
            sorts: view.sorts.clone(),
            group: view.group.clone(),
            date_property: view.date_property.clone(),
            ..Self::default()
        }
    }
}

/// One row of an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRow {
    /// The row page's node id.
    pub id: String,
    /// The row page's title (the title property).
    pub title: String,
    /// The row page's icon.
    pub icon: Option<String>,
    /// Effective cell values by property id; empty cells are absent. A
    /// backlink property shows the rows that link to this one.
    pub values: BTreeMap<String, CellValue>,
}

/// One group of a grouped answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    /// The option id (select, multi-select), `"true"` or `"false"` (checkbox),
    /// or `None` for the rows with no value.
    pub key: Option<String>,
    /// How many rows are in the group (a multi-select row is in each of its
    /// groups).
    pub count: usize,
    /// The first rows of the group, up to the request's limit.
    pub rows: Vec<QueryRow>,
}

/// The answer to a query.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    /// How many rows match the filter (before paging).
    pub total: usize,
    /// The requested page, when the query is not grouped.
    pub rows: Vec<QueryRow>,
    /// The groups, in the order of the grouping property's options, when it is.
    pub groups: Vec<Bucket>,
    /// Titles of the rows that relation values point at, by node id; rows that
    /// are missing or in the trash are absent.
    pub titles: BTreeMap<String, String>,
}
