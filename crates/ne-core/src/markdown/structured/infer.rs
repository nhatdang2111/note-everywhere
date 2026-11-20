//! Guessing the type of a CSV column from its values.
//!
//! The rules, first match wins, over the non-empty cells of the column:
//!
//! 1. every cell is a yes/no word (`yes`, `no`, `true`, `false`, `checked`,
//!    `unchecked`): checkbox;
//! 2. every cell is a number: number;
//! 3. every cell is a date in one of the forms [`super::dates`] reads: date;
//! 4. every cell is a web address (`http://` or `https://`, no spaces): URL;
//! 5. every cell is a list of links to pages of the export: relation (the
//!    importer keeps it only when all targets are rows of one imported
//!    database);
//! 6. cells that contain a comma list their values in the way a multi-select
//!    does: multi-select, provided every item is at most 40 characters and
//!    there are at most 50 distinct items;
//! 7. cells with at most 20 distinct values that repeat (there are at least
//!    twice as many cells as distinct values): select;
//! 8. otherwise text.

use std::collections::BTreeSet;

use super::dates::parse_cell_date;
use crate::database::{CellValue, PropertyType};
use crate::markdown::folder::percent_decode;

const MAX_OPTION_CHARS: usize = 40;
const MAX_MULTI_OPTIONS: usize = 50;
const MAX_SELECT_OPTIONS: usize = 20;

fn yes_no(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "yes" | "true" | "checked" => Some(true),
        "no" | "false" | "unchecked" => Some(false),
        _ => None,
    }
}

fn number(text: &str) -> Option<f64> {
    let text = text.trim();
    // A leading zero or a plus sign marks an identifier (a phone number),
    // not a quantity.
    if text.starts_with('+') || (text.len() > 1 && text.starts_with('0') && !text.starts_with("0."))
    {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn web_address(text: &str) -> bool {
    let text = text.trim();
    (text.starts_with("http://") || text.starts_with("https://"))
        && !text.contains(char::is_whitespace)
}

/// The 32-character hex ids of the pages a cell links to, in order.
pub fn linked_ids(cell: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = cell;
    while let Some(end) = rest.find(".md)") {
        let before = &rest[..end];
        if let Some(open) = before.rfind('(') {
            let path = percent_decode(&before[open + 1..]);
            let stem = path.rsplit('/').next().unwrap_or(&path);
            if let Some(id) = trailing_hex_id(stem) {
                ids.push(id);
            }
        }
        rest = &rest[end + 4..];
    }
    ids
}

/// The id at the end of an exported name (`Title <32 hex>`), if there is one.
pub fn trailing_hex_id(name: &str) -> Option<String> {
    let name = name.trim_end();
    let tail = name.get(name.len().checked_sub(32)?..)?;
    let is_id = tail.bytes().all(|b| b.is_ascii_hexdigit());
    let spaced = name.len() == 32 || name[..name.len() - 32].ends_with(' ');
    (is_id && spaced).then(|| tail.to_lowercase())
}

/// Splits a multi-select cell into its items.
pub fn items(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The guessed type and, for select types, the option names in first-seen
/// order.
pub fn infer(values: &[&str]) -> (PropertyType, Vec<String>) {
    let cells: Vec<&str> = values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect();
    if cells.is_empty() {
        return (PropertyType::Text, Vec::new());
    }
    let all = |f: &dyn Fn(&str) -> bool| cells.iter().all(|c| f(c));
    if all(&|c| yes_no(c).is_some()) {
        return (PropertyType::Checkbox, Vec::new());
    }
    if all(&|c| number(c).is_some()) {
        return (PropertyType::Number, Vec::new());
    }
    if all(&|c| parse_cell_date(c).is_some()) {
        return (PropertyType::Date, Vec::new());
    }
    if all(&web_address) {
        return (PropertyType::Url, Vec::new());
    }
    if all(&|c| !linked_ids(c).is_empty()) {
        return (PropertyType::Relation, Vec::new());
    }
    let mut seen: Vec<String> = Vec::new();
    let mut note = |name: &str| {
        if !seen.iter().any(|s| s == name) {
            seen.push(name.to_owned());
        }
    };
    if cells.iter().any(|c| c.contains(',')) {
        let everything: Vec<String> = cells.iter().flat_map(|c| items(c)).collect();
        everything.iter().for_each(|i| note(i));
        let short = everything
            .iter()
            .all(|i| i.chars().count() <= MAX_OPTION_CHARS);
        if short && seen.len() <= MAX_MULTI_OPTIONS {
            return (PropertyType::MultiSelect, seen);
        }
        return (PropertyType::Text, Vec::new());
    }
    cells.iter().for_each(|c| note(c));
    let distinct: BTreeSet<&String> = seen.iter().collect();
    let short = seen.iter().all(|s| s.chars().count() <= MAX_OPTION_CHARS);
    if short && distinct.len() <= MAX_SELECT_OPTIONS && cells.len() >= 2 * distinct.len() {
        return (PropertyType::Select, seen);
    }
    (PropertyType::Text, Vec::new())
}

/// The cell value for a non-empty CSV cell of an inferred (non-relation)
/// type. `option_id` finds the id of an option by name.
pub fn to_cell(
    ty: PropertyType,
    text: &str,
    option_id: &dyn Fn(&str) -> Option<String>,
) -> Option<CellValue> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match ty {
        PropertyType::Checkbox => yes_no(text).map(CellValue::Checkbox),
        PropertyType::Number => number(text).map(CellValue::Number),
        PropertyType::Date => parse_cell_date(text).map(CellValue::Date),
        PropertyType::Url => Some(CellValue::Url(text.to_owned())),
        PropertyType::Select => option_id(text).map(CellValue::Select),
        PropertyType::MultiSelect => {
            let ids: Vec<String> = items(text).iter().filter_map(|i| option_id(i)).collect();
            (!ids.is_empty()).then_some(CellValue::MultiSelect(ids))
        }
        PropertyType::Text => Some(CellValue::Text(text.to_owned())),
        PropertyType::Relation => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(values: &[&str]) -> PropertyType {
        infer(values).0
    }

    #[test]
    fn scalar_types_are_recognised() {
        assert_eq!(ty(&["Yes", "No", ""]), PropertyType::Checkbox);
        assert_eq!(ty(&["1", "2.5", "-3"]), PropertyType::Number);
        assert_eq!(
            ty(&["0123", "0456"]),
            PropertyType::Text,
            "leading zeros are identifiers"
        );
        assert_eq!(ty(&["November 21, 2026", "2026-01-02"]), PropertyType::Date);
        assert_eq!(
            ty(&["https://a.test/x", "http://b.test"]),
            PropertyType::Url
        );
        assert_eq!(ty(&["", ""]), PropertyType::Text);
    }

    #[test]
    fn choice_types_follow_the_distinct_values() {
        let (kind, options) = infer(&["Todo", "Done", "Todo", "Done", "Todo", "Done"]);
        assert_eq!(kind, PropertyType::Select);
        assert_eq!(options, ["Todo", "Done"]);
        let (kind, options) = infer(&["a, b", "b, c", ""]);
        assert_eq!(kind, PropertyType::MultiSelect);
        assert_eq!(options, ["a", "b", "c"]);
        // All different values are text, not a select with one option each.
        assert_eq!(ty(&["one", "two", "three"]), PropertyType::Text);
        let long = "x".repeat(41);
        assert_eq!(ty(&[&format!("{long}, y")]), PropertyType::Text);
    }

    #[test]
    fn links_to_pages_are_a_relation() {
        let id = "0123456789abcdef0123456789abcdef";
        let cell = format!("Row (Db%20{id}/Row%20{id}.md), Other ({id}.md)");
        assert_eq!(linked_ids(&cell), [id, id]);
        assert_eq!(ty(&[&cell]), PropertyType::Relation);
        assert_eq!(trailing_hex_id(&format!("Title {id}")), Some(id.to_owned()));
        assert_eq!(trailing_hex_id("Title 123"), None);
        assert_eq!(
            trailing_hex_id(&format!("Title{id}")),
            None,
            "needs a space before the id"
        );
    }
}
