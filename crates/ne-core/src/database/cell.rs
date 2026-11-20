//! Cell values: validation against a property, and the conversion table that
//! lets a cell written under one property type be read under another.
//!
//! # Conversion table
//!
//! A cell stores the type it was written under. When a property's type
//! changes, **no cell is rewritten**: the stored value is converted on read
//! to the property's current type. Because the original stays in the document,
//! converting the property back restores it exactly, and a value written on
//! another device under the old type needs no special handling. Where the
//! table has no entry the cell reads as empty (and stays stored).
//!
//! | from \ to | text | number | select | multi-select | date | checkbox | url | relation |
//! |---|---|---|---|---|---|---|---|---|
//! | text | same | parsed if numeric | option with that name | options named in the comma list | parsed if ISO date | true for `true`, `yes`, `1`, `x` | same | empty |
//! | number | decimal text | same | empty | empty | empty | not zero | empty | empty |
//! | select | option name | empty | same | that option | empty | empty | option name | empty |
//! | multi-select | names joined by `, ` | empty | first option | same | empty | empty | names joined | empty |
//! | date | start (and ` → end`) | empty | empty | empty | same | empty | empty | empty |
//! | checkbox | `true`/`false` | 1 or 0 | empty | empty | empty | same | empty | empty |
//! | url | same | parsed if numeric | option with that name | like text | parsed if ISO date | like text | same | empty |
//! | relation | empty | empty | empty | empty | empty | empty | empty | same |
//!
//! Options are matched by name ignoring case and accents. Options that no
//! longer exist, and ids a property does not know, are dropped. Changing a
//! text property to select or multi-select first creates the options its
//! values name, see `DatabaseDoc::change_property_type`.

use super::date::{DateValue, day_of};
use super::types::{CellValue, Property, PropertyType};
use crate::error::{Error, Result};

/// Most characters of a text cell.
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Most entries of a multi-select or relation cell.
pub const MAX_LIST_ENTRIES: usize = 10_000;

/// Folds text for matching names: lowercase, no accents (`đ` as `d`).
pub fn fold_name(text: &str) -> String {
    use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'đ' { 'd' } else { c })
        .collect()
}

/// Checks a value written to a cell of `property`: its type must be the
/// property's, selects must name existing options, and numbers must be finite.
pub fn validate(property: &Property, value: &CellValue) -> Result<()> {
    if value.property_type() != property.ty {
        return Err(Error::Invalid(format!(
            "property {:?} holds {:?}, not {:?}",
            property.name,
            property.ty,
            value.property_type()
        )));
    }
    match value {
        CellValue::Number(n) if !n.is_finite() => {
            return Err(Error::Invalid("a number must be finite".into()));
        }
        CellValue::Text(s) | CellValue::Url(s) if s.chars().count() > MAX_TEXT_CHARS => {
            return Err(Error::Invalid("the text is too long".into()));
        }
        CellValue::Select(id) if property.option(id).is_none() => {
            return Err(Error::Missing(format!("option {id}")));
        }
        CellValue::MultiSelect(ids) => {
            if ids.len() > MAX_LIST_ENTRIES {
                return Err(Error::Invalid("too many options".into()));
            }
            if let Some(id) = ids.iter().find(|id| property.option(id).is_none()) {
                return Err(Error::Missing(format!("option {id}")));
            }
        }
        CellValue::Relation(ids) if ids.len() > MAX_LIST_ENTRIES => {
            return Err(Error::Invalid("too many linked rows".into()));
        }
        CellValue::Date(date) => date.validate()?,
        _ => {}
    }
    Ok(())
}

fn non_empty(value: CellValue) -> Option<CellValue> {
    (!value.is_empty()).then_some(value)
}

fn option_named(property: &Property, name: &str) -> Option<String> {
    let wanted = fold_name(name.trim());
    property
        .options
        .iter()
        .find(|o| fold_name(&o.name) == wanted)
        .map(|o| o.id.clone())
}

fn option_name(property: &Property, id: &str) -> Option<String> {
    property.option(id).map(|o| o.name.clone())
}

fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim();
    let normal = if text.contains('.') {
        text.to_owned()
    } else {
        text.replace(',', ".")
    };
    normal.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Number as text without a trailing `.0` (`3` rather than `3.0`).
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        n.to_string()
    }
}

fn date_prefix(text: &str) -> Option<DateValue> {
    let text = text.trim();
    for len in [16, 10] {
        if let Some(candidate) = text.get(..len)
            && day_of(candidate).is_some()
        {
            return Some(DateValue::new(candidate));
        }
    }
    None
}

/// The value as text, for the conversions that go through text.
pub(crate) fn as_text(value: &CellValue, property: &Property) -> Option<String> {
    let text = match value {
        CellValue::Text(s) | CellValue::Url(s) => s.clone(),
        CellValue::Number(n) => number_text(*n),
        CellValue::Select(id) => option_name(property, id)?,
        CellValue::MultiSelect(ids) => ids
            .iter()
            .filter_map(|id| option_name(property, id))
            .collect::<Vec<_>>()
            .join(", "),
        CellValue::Date(d) => match &d.end {
            Some(end) => format!("{} → {end}", d.start),
            None => d.start.clone(),
        },
        CellValue::Checkbox(b) => b.to_string(),
        CellValue::Relation(_) => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn truthy(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "true" | "yes" | "1" | "x" | "checked"
    )
}

/// Converts a stored `value` to what `property` (with its current type and
/// options) shows, following the table in the [module documentation](self).
/// `None` means the cell reads as empty.
pub fn coerce(value: &CellValue, property: &Property) -> Option<CellValue> {
    if value.property_type() == property.ty {
        return coerce_same(value, property);
    }
    let text = || as_text(value, property);
    match property.ty {
        PropertyType::Text => text().map(CellValue::Text),
        PropertyType::Url => match value {
            CellValue::Text(_) | CellValue::Select(_) | CellValue::MultiSelect(_) => {
                text().map(CellValue::Url)
            }
            _ => None,
        },
        PropertyType::Number => match value {
            CellValue::Text(s) | CellValue::Url(s) => parse_number(s).map(CellValue::Number),
            CellValue::Checkbox(b) => Some(CellValue::Number(f64::from(u8::from(*b)))),
            _ => None,
        },
        PropertyType::Select => match value {
            CellValue::Text(s) | CellValue::Url(s) => {
                option_named(property, s).map(CellValue::Select)
            }
            CellValue::MultiSelect(ids) => ids
                .iter()
                .find(|id| property.option(id).is_some())
                .cloned()
                .map(CellValue::Select),
            _ => None,
        },
        PropertyType::MultiSelect => match value {
            CellValue::Select(id) => property
                .option(id)
                .map(|_| CellValue::MultiSelect(vec![id.clone()])),
            CellValue::Text(s) | CellValue::Url(s) => {
                let mut ids: Vec<String> = Vec::new();
                for part in s.split(',') {
                    if let Some(id) = option_named(property, part)
                        && !ids.contains(&id)
                    {
                        ids.push(id);
                    }
                }
                non_empty(CellValue::MultiSelect(ids))
            }
            _ => None,
        },
        PropertyType::Date => match value {
            CellValue::Text(s) | CellValue::Url(s) => date_prefix(s).map(CellValue::Date),
            _ => None,
        },
        PropertyType::Checkbox => match value {
            CellValue::Number(n) => Some(CellValue::Checkbox(*n != 0.0)),
            CellValue::Text(s) | CellValue::Url(s) => Some(CellValue::Checkbox(truthy(s))),
            _ => None,
        },
        PropertyType::Relation => None,
    }
}

/// A value already of the property's type: options that do not exist any more
/// are dropped, empty values read as empty.
fn coerce_same(value: &CellValue, property: &Property) -> Option<CellValue> {
    match value {
        CellValue::Select(id) => property.option(id).map(|_| CellValue::Select(id.clone())),
        CellValue::MultiSelect(ids) => {
            let ids: Vec<String> = ids
                .iter()
                .filter(|id| property.option(id).is_some())
                .cloned()
                .collect();
            non_empty(CellValue::MultiSelect(ids))
        }
        CellValue::Number(n) if !n.is_finite() => None,
        CellValue::Date(d) if d.validate().is_err() => None,
        other => non_empty(other.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::types::SelectOption;

    fn property(ty: PropertyType) -> Property {
        Property {
            id: "p".into(),
            name: "P".into(),
            ty,
            options: vec![
                SelectOption {
                    id: "a".into(),
                    name: "Đang làm".into(),
                    color: "blue".into(),
                },
                SelectOption {
                    id: "b".into(),
                    name: "Xong".into(),
                    color: "green".into(),
                },
            ],
            relation: None,
        }
    }

    #[test]
    fn text_converts_to_typed_values_where_it_can() {
        let text = |s: &str| CellValue::Text(s.into());
        assert_eq!(
            coerce(&text("3,5"), &property(PropertyType::Number)),
            Some(CellValue::Number(3.5))
        );
        assert_eq!(coerce(&text("abc"), &property(PropertyType::Number)), None);
        assert_eq!(
            coerce(&text("dang lam"), &property(PropertyType::Select)),
            Some(CellValue::Select("a".into()))
        );
        assert_eq!(
            coerce(
                &text("Xong, Đang làm, nope"),
                &property(PropertyType::MultiSelect)
            ),
            Some(CellValue::MultiSelect(vec!["b".into(), "a".into()]))
        );
        assert_eq!(
            coerce(&text("2026-11-21 sau"), &property(PropertyType::Date)),
            Some(CellValue::Date(DateValue::new("2026-11-21")))
        );
        assert_eq!(
            coerce(&text("Yes"), &property(PropertyType::Checkbox)),
            Some(CellValue::Checkbox(true))
        );
    }

    #[test]
    fn typed_values_convert_to_text() {
        let text = property(PropertyType::Text);
        let holder = property(PropertyType::Select);
        assert_eq!(
            coerce(&CellValue::Number(3.0), &text),
            Some(CellValue::Text("3".into()))
        );
        assert_eq!(
            coerce(&CellValue::Number(2.5), &text),
            Some(CellValue::Text("2.5".into()))
        );
        // A select converts by option name; the options come from the property.
        let mut with_options = text.clone();
        with_options.options = holder.options;
        assert_eq!(
            coerce(&CellValue::Select("b".into()), &with_options),
            Some(CellValue::Text("Xong".into()))
        );
        assert_eq!(
            coerce(
                &CellValue::MultiSelect(vec!["a".into(), "b".into()]),
                &with_options
            ),
            Some(CellValue::Text("Đang làm, Xong".into()))
        );
        assert_eq!(
            coerce(&CellValue::Checkbox(false), &text),
            Some(CellValue::Text("false".into()))
        );
        assert_eq!(
            coerce(&CellValue::Relation(vec!["1@1".into()]), &text),
            None
        );
    }

    #[test]
    fn removed_options_are_dropped_and_wrong_types_read_empty() {
        let select = property(PropertyType::Select);
        assert_eq!(coerce(&CellValue::Select("gone".into()), &select), None);
        let multi = property(PropertyType::MultiSelect);
        assert_eq!(
            coerce(
                &CellValue::MultiSelect(vec!["gone".into(), "a".into()]),
                &multi
            ),
            Some(CellValue::MultiSelect(vec!["a".into()]))
        );
        assert_eq!(
            coerce(&CellValue::Number(1.0), &property(PropertyType::Date)),
            None
        );
        assert_eq!(
            coerce(
                &CellValue::Text(String::new()),
                &property(PropertyType::Text)
            ),
            None
        );
    }

    #[test]
    fn validation_rejects_wrong_type_unknown_option_and_bad_numbers() {
        let number = property(PropertyType::Number);
        assert!(validate(&number, &CellValue::Number(1.0)).is_ok());
        assert!(validate(&number, &CellValue::Number(f64::NAN)).is_err());
        assert!(validate(&number, &CellValue::Text("1".into())).is_err());
        let select = property(PropertyType::Select);
        assert!(validate(&select, &CellValue::Select("a".into())).is_ok());
        assert!(validate(&select, &CellValue::Select("zz".into())).is_err());
        let date = property(PropertyType::Date);
        assert!(validate(&date, &CellValue::Date(DateValue::new("2026-02-30"))).is_err());
    }

    #[test]
    fn fold_name_ignores_case_and_accents() {
        assert_eq!(fold_name("Đang LÀM"), "dang lam");
    }
}
