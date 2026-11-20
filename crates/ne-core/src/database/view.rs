//! Views: how a database is shown. A view stores its own filter, sorts,
//! grouping and columns; editing one never changes another.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::cell::fold_name;
use super::date::is_valid_date;
use super::types::{MAX_NAME_CHARS, Property, PropertyType, TITLE_PROPERTY};
use crate::error::{Error, Result};

/// Deepest nesting of filter groups (a group inside a group inside a group).
pub const MAX_FILTER_DEPTH: usize = 3;
/// Most conditions and groups in one filter.
pub const MAX_FILTER_NODES: usize = 100;
/// Most sort keys of a view.
pub const MAX_SORT_KEYS: usize = 8;

/// How rows are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    /// Rows and columns.
    Table,
    /// Cards in columns, one per option of the grouping property.
    Board,
    /// A plain list of titles.
    List,
    /// Rows placed on the days of a date property.
    Calendar,
}

/// How a group of conditions combines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Logic {
    /// Every child must match.
    And,
    /// At least one child must match.
    Or,
}

/// What a condition tests. Which operators suit a property type:
///
/// | type | operators |
/// |---|---|
/// | title, text, url | `contains`, `not_contains`, `is`, `is_not`, `starts_with`, `ends_with`, `is_empty`, `is_not_empty` (text matches ignore case and accents) |
/// | number | `is`, `is_not`, `gt`, `gte`, `lt`, `lte`, `is_empty`, `is_not_empty` |
/// | select | `is`, `is_not` (an option id), `is_empty`, `is_not_empty` |
/// | multi-select | `contains`, `not_contains` (an option id), `is_empty`, `is_not_empty` |
/// | date | `is`, `before`, `after`, `on_or_before`, `on_or_after` (a `YYYY-MM-DD` day; the time of day is ignored), `is_empty`, `is_not_empty` |
/// | checkbox | `is_checked`, `is_unchecked` |
/// | relation | `contains`, `not_contains` (a row id), `is_empty`, `is_not_empty` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)] // The names are the documentation, see the table above.
pub enum Operator {
    Contains,
    NotContains,
    Is,
    IsNot,
    StartsWith,
    EndsWith,
    Gt,
    Gte,
    Lt,
    Lte,
    Before,
    After,
    OnOrBefore,
    OnOrAfter,
    IsEmpty,
    IsNotEmpty,
    IsChecked,
    IsUnchecked,
}

impl Operator {
    /// True for operators that take no comparison value.
    pub fn is_unary(self) -> bool {
        matches!(
            self,
            Self::IsEmpty | Self::IsNotEmpty | Self::IsChecked | Self::IsUnchecked
        )
    }
}

/// A filter: a condition on one property, or a group of filters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FilterNode {
    /// Children combined with `and` or `or`. An empty group matches every row.
    Group {
        /// How the children combine.
        logic: Logic,
        /// The children.
        children: Vec<FilterNode>,
    },
    /// One test on one property.
    Condition {
        /// Property id, or `title`.
        property: String,
        /// The test.
        operator: Operator,
        /// The comparison value: a string for text, ids, relations and dates,
        /// a number for numbers; absent for unary operators.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
    },
}

/// One key of a multi-key sort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortKey {
    /// Property id, or `title`. Text sorts by the Vietnamese alphabet, select
    /// by the order of its options, the rest by value. Rows with no value come
    /// last whichever the direction.
    pub property: String,
    /// Largest first.
    #[serde(default)]
    pub descending: bool,
}

/// Grouping of a view (the columns of a board).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupConfig {
    /// A select, multi-select or checkbox property.
    pub property: String,
    /// Leave out groups with no rows.
    #[serde(default)]
    pub hide_empty: bool,
}

/// Visibility and width of one column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnConfig {
    /// Property id, or `title`.
    pub property: String,
    /// Shown or hidden.
    #[serde(default = "yes")]
    pub visible: bool,
    /// Width in pixels, when the person resized it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

fn yes() -> bool {
    true
}

/// A saved view. `columns` lists the columns in display order; properties it
/// leaves out are shown after the listed ones, in schema order, so a new
/// property appears in every view without editing them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDef {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Layout.
    pub kind: ViewKind,
    /// Which rows show.
    #[serde(default)]
    pub filter: Option<FilterNode>,
    /// Order of rows; ties fall back to the manual order of the rows.
    #[serde(default)]
    pub sorts: Vec<SortKey>,
    /// Grouping, for boards and grouped lists.
    #[serde(default)]
    pub group: Option<GroupConfig>,
    /// Column order, visibility and widths.
    #[serde(default)]
    pub columns: Vec<ColumnConfig>,
    /// The date property a calendar places rows by.
    #[serde(default)]
    pub date_property: Option<String>,
}

fn find<'a>(properties: &'a [Property], id: &str) -> Option<&'a Property> {
    properties.iter().find(|p| p.id == id)
}

/// The operators a property type accepts, see [`Operator`].
pub fn operators_for(ty: Option<PropertyType>) -> &'static [Operator] {
    use Operator::*;
    match ty {
        // The title is text.
        None | Some(PropertyType::Text | PropertyType::Url) => &[
            Contains,
            NotContains,
            Is,
            IsNot,
            StartsWith,
            EndsWith,
            IsEmpty,
            IsNotEmpty,
        ],
        Some(PropertyType::Number) => &[Is, IsNot, Gt, Gte, Lt, Lte, IsEmpty, IsNotEmpty],
        Some(PropertyType::Select) => &[Is, IsNot, IsEmpty, IsNotEmpty],
        Some(PropertyType::MultiSelect | PropertyType::Relation) => {
            &[Contains, NotContains, IsEmpty, IsNotEmpty]
        }
        Some(PropertyType::Date) => &[
            Is, Before, After, OnOrBefore, OnOrAfter, IsEmpty, IsNotEmpty,
        ],
        Some(PropertyType::Checkbox) => &[IsChecked, IsUnchecked],
    }
}

fn check_condition(
    properties: &[Property],
    property: &str,
    operator: Operator,
    value: Option<&Value>,
) -> Result<()> {
    let ty = if property == TITLE_PROPERTY {
        None
    } else {
        Some(
            find(properties, property)
                .ok_or_else(|| Error::Missing(format!("property {property}")))?
                .ty,
        )
    };
    if !operators_for(ty).contains(&operator) {
        return Err(Error::Invalid(format!(
            "{operator:?} does not apply to {}",
            ty.map_or("title".to_owned(), |t| format!("{t:?}"))
        )));
    }
    if operator.is_unary() {
        return Ok(());
    }
    let value = value.ok_or_else(|| Error::Invalid(format!("{operator:?} needs a value")))?;
    let ok = match ty {
        None | Some(PropertyType::Text | PropertyType::Url) => value.is_string(),
        Some(PropertyType::Number) => value.as_f64().is_some_and(f64::is_finite),
        Some(PropertyType::Select | PropertyType::MultiSelect | PropertyType::Relation) => {
            value.is_string()
        }
        Some(PropertyType::Date) => value.as_str().is_some_and(is_valid_date),
        Some(PropertyType::Checkbox) => true,
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "the value of the {property} condition has the wrong type"
        )))
    }
}

fn check_filter(
    properties: &[Property],
    node: &FilterNode,
    depth: usize,
    count: &mut usize,
) -> Result<()> {
    *count += 1;
    if *count > MAX_FILTER_NODES {
        return Err(Error::Invalid("the filter is too large".into()));
    }
    match node {
        FilterNode::Group { children, .. } => {
            if depth > MAX_FILTER_DEPTH {
                return Err(Error::Invalid(format!(
                    "filters nest at most {MAX_FILTER_DEPTH} groups deep"
                )));
            }
            children
                .iter()
                .try_for_each(|c| check_filter(properties, c, depth + 1, count))
        }
        FilterNode::Condition {
            property,
            operator,
            value,
        } => check_condition(properties, property, *operator, value.as_ref()),
    }
}

/// Checks a filter against the properties: every property exists, every
/// operator suits its property's type, values have the right shape, and groups
/// nest at most [`MAX_FILTER_DEPTH`] deep.
pub fn validate_filter(properties: &[Property], filter: &FilterNode) -> Result<()> {
    check_filter(properties, filter, 1, &mut 0)
}

/// Checks sorts and grouping against the properties.
pub fn validate_sorts(properties: &[Property], sorts: &[SortKey]) -> Result<()> {
    if sorts.len() > MAX_SORT_KEYS {
        return Err(Error::Invalid(format!("at most {MAX_SORT_KEYS} sort keys")));
    }
    for key in sorts {
        if key.property != TITLE_PROPERTY && find(properties, &key.property).is_none() {
            return Err(Error::Missing(format!("property {}", key.property)));
        }
    }
    Ok(())
}

/// Checks a grouping: the property must be a select, multi-select or checkbox.
pub fn validate_group(properties: &[Property], group: &GroupConfig) -> Result<()> {
    match find(properties, &group.property).map(|p| p.ty) {
        Some(PropertyType::Select | PropertyType::MultiSelect | PropertyType::Checkbox) => Ok(()),
        Some(_) => Err(Error::Invalid(
            "group by a select, multi-select or checkbox property".into(),
        )),
        None => Err(Error::Missing(format!("property {}", group.property))),
    }
}

/// Checks a whole view against the properties.
pub fn validate_view(properties: &[Property], view: &ViewDef) -> Result<()> {
    if view.name.trim().is_empty() || view.name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::Invalid("a view name is 1 to 100 characters".into()));
    }
    if let Some(filter) = &view.filter {
        validate_filter(properties, filter)?;
    }
    validate_sorts(properties, &view.sorts)?;
    if let Some(group) = &view.group {
        validate_group(properties, group)?;
    }
    if let Some(id) = &view.date_property
        && find(properties, id).map(|p| p.ty) != Some(PropertyType::Date)
    {
        return Err(Error::Invalid("a calendar needs a date property".into()));
    }
    for column in &view.columns {
        if column.property != TITLE_PROPERTY && find(properties, &column.property).is_none() {
            return Err(Error::Missing(format!("property {}", column.property)));
        }
    }
    Ok(())
}

/// Folds a text filter value the way the index folds cell text.
pub fn fold_filter_text(text: &str) -> String {
    fold_name(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props() -> Vec<Property> {
        let make = |id: &str, ty| Property {
            id: id.into(),
            name: id.into(),
            ty,
            options: Vec::new(),
            relation: None,
        };
        vec![
            make("n", PropertyType::Number),
            make("t", PropertyType::Text),
            make("d", PropertyType::Date),
        ]
    }

    fn cond(property: &str, operator: Operator, value: Option<Value>) -> FilterNode {
        FilterNode::Condition {
            property: property.into(),
            operator,
            value,
        }
    }

    #[test]
    fn operators_must_suit_the_type_and_values_the_operator() {
        let props = props();
        assert!(validate_filter(&props, &cond("n", Operator::Gt, Some(json!(3)))).is_ok());
        assert!(validate_filter(&props, &cond("n", Operator::Contains, Some(json!("3")))).is_err());
        assert!(validate_filter(&props, &cond("n", Operator::Gt, Some(json!("3")))).is_err());
        assert!(validate_filter(&props, &cond("n", Operator::Gt, None)).is_err());
        assert!(validate_filter(&props, &cond("n", Operator::IsEmpty, None)).is_ok());
        assert!(
            validate_filter(
                &props,
                &cond("title", Operator::StartsWith, Some(json!("a")))
            )
            .is_ok()
        );
        assert!(
            validate_filter(
                &props,
                &cond("d", Operator::After, Some(json!("2026-13-01")))
            )
            .is_err()
        );
        assert!(validate_filter(&props, &cond("zz", Operator::IsEmpty, None)).is_err());
    }

    #[test]
    fn groups_nest_three_deep_and_no_further() {
        let props = props();
        let leaf = cond("t", Operator::IsEmpty, None);
        let wrap = |inner| FilterNode::Group {
            logic: Logic::Or,
            children: vec![inner],
        };
        let three = wrap(wrap(wrap(leaf.clone())));
        assert!(validate_filter(&props, &three).is_ok());
        assert!(validate_filter(&props, &wrap(three)).is_err());
    }

    #[test]
    fn filters_round_trip_through_json() {
        let filter = FilterNode::Group {
            logic: Logic::And,
            children: vec![cond("n", Operator::Gte, Some(json!(2.5)))],
        };
        let text = serde_json::to_string(&filter).expect("encode");
        assert!(text.contains("\"type\":\"group\""));
        let back: FilterNode = serde_json::from_str(&text).expect("decode");
        assert_eq!(back, filter);
    }

    #[test]
    fn grouping_needs_a_choice_property() {
        let mut props = props();
        props.push(Property {
            id: "s".into(),
            name: "s".into(),
            ty: PropertyType::Select,
            options: Vec::new(),
            relation: None,
        });
        let group = |p: &str| GroupConfig {
            property: p.into(),
            hide_empty: false,
        };
        assert!(validate_group(&props, &group("s")).is_ok());
        assert!(validate_group(&props, &group("t")).is_err());
    }
}
