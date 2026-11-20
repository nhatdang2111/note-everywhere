//! Reading and writing cell values.

use std::collections::{BTreeMap, HashMap, HashSet};

use loro::LoroValue;

use super::cell::{coerce, validate};
use super::types::{CellValue, Property, TITLE_PROPERTY};
use super::{CELLS, DatabaseDoc};
use crate::error::{Error, Result};

fn key(row: &str, property: &str) -> String {
    format!("{row}/{property}")
}

fn decode(value: &LoroValue) -> Option<CellValue> {
    match value {
        LoroValue::String(json) => serde_json::from_str(json).ok(),
        _ => None,
    }
}

/// The effective values of one row: the stored cells converted to the current
/// property types, empty cells left out, keyed by property id.
pub type RowValues = BTreeMap<String, CellValue>;

impl DatabaseDoc {
    fn stored(&self, row: &str, property: &str) -> Option<CellValue> {
        let value = self
            .map(CELLS)
            .get(&key(row, property))?
            .into_value()
            .ok()?;
        decode(&value)
    }

    /// Writes a cell, or clears it with `None`. The value must be of the
    /// property's type (and select options must exist); the title and a
    /// backlink property cannot be written here. Writing what is already
    /// stored changes nothing.
    pub fn set_cell(&self, row: &str, property: &str, value: Option<CellValue>) -> Result<()> {
        if row.is_empty() || row.contains('/') {
            return Err(Error::Invalid(format!("{row:?} is not a row id")));
        }
        if property == TITLE_PROPERTY {
            return Err(Error::Invalid(
                "the title of a row is the title of its page".into(),
            ));
        }
        let prop = self.property(property)?;
        if prop.is_backlink() {
            return Err(Error::Invalid(
                "a backlink property is computed from the relation it mirrors".into(),
            ));
        }
        let cells = self.map(CELLS);
        let cell_key = key(row, property);
        match value {
            Some(value) => {
                validate(&prop, &value)?;
                if self.stored(row, property).as_ref() == Some(&value) {
                    return Ok(());
                }
                let json =
                    serde_json::to_string(&value).map_err(|e| Error::Invalid(e.to_string()))?;
                cells.insert(&cell_key, json.as_str())?;
            }
            None => {
                if cells.get(&cell_key).is_some() {
                    cells.delete(&cell_key)?;
                }
            }
        }
        self.commit();
        Ok(())
    }

    /// The effective value of one cell, or `None` when it is empty.
    pub fn cell(&self, row: &str, property: &str) -> Result<Option<CellValue>> {
        let prop = self.property(property)?;
        Ok(self
            .stored(row, property)
            .and_then(|stored| coerce(&stored, &prop)))
    }

    /// The effective values of one row.
    pub fn row_values(&self, row: &str, properties: &[Property]) -> RowValues {
        properties
            .iter()
            .filter_map(|prop| {
                let stored = self.stored(row, &prop.id)?;
                Some((prop.id.clone(), coerce(&stored, prop)?))
            })
            .collect()
    }

    /// Every stored cell as `(row, property, value)`, for rebuilding an index.
    fn all_stored(&self) -> Vec<(String, String, CellValue)> {
        let LoroValue::Map(map) = self.map(CELLS).get_value() else {
            return Vec::new();
        };
        map.iter()
            .filter_map(|(k, v)| {
                let (row, property) = k.split_once('/')?;
                Some((row.to_owned(), property.to_owned(), decode(v)?))
            })
            .collect()
    }

    /// The effective values of every row that has any, keyed by row id.
    pub fn all_row_values(&self, properties: &[Property]) -> HashMap<String, RowValues> {
        let by_id: HashMap<&str, &Property> =
            properties.iter().map(|p| (p.id.as_str(), p)).collect();
        let mut rows: HashMap<String, RowValues> = HashMap::new();
        for (row, property, stored) in self.all_stored() {
            let Some(prop) = by_id.get(property.as_str()) else {
                continue;
            };
            if let Some(value) = coerce(&stored, prop) {
                rows.entry(row).or_default().insert(property, value);
            }
        }
        rows
    }

    /// Every stored value of one property, as written (not converted).
    pub(crate) fn cells_of_property(&self, property: &str) -> Vec<(String, CellValue)> {
        self.all_stored()
            .into_iter()
            .filter(|(_, p, _)| p == property)
            .map(|(row, _, value)| (row, value))
            .collect()
    }

    /// Removes every cell of the given rows (they were deleted for good).
    pub fn clear_rows(&self, rows: &[String]) -> Result<usize> {
        let wanted: HashSet<&str> = rows.iter().map(String::as_str).collect();
        let cells = self.map(CELLS);
        let doomed: Vec<String> = cells
            .keys()
            .map(|k| k.to_string())
            .filter(|k| {
                k.split_once('/')
                    .is_some_and(|(row, _)| wanted.contains(row))
            })
            .collect();
        for k in &doomed {
            cells.delete(k)?;
        }
        self.commit();
        Ok(doomed.len())
    }

    /// Removes cells of properties that no longer exist, and cells of rows
    /// that are not in `live_rows`. Returns how many were removed.
    pub fn purge_orphans(&self, live_rows: &HashSet<String>) -> Result<usize> {
        let known: HashSet<String> = self.properties()?.into_iter().map(|p| p.id).collect();
        let cells = self.map(CELLS);
        let doomed: Vec<String> = cells
            .keys()
            .map(|k| k.to_string())
            .filter(|k| match k.split_once('/') {
                Some((row, property)) => !known.contains(property) || !live_rows.contains(row),
                None => true,
            })
            .collect();
        for k in &doomed {
            cells.delete(k)?;
        }
        self.commit();
        Ok(doomed.len())
    }
}
