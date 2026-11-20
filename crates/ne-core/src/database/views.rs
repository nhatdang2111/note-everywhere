//! Reading and changing the saved views of a database.

use loro::{LoroMap, LoroValue, ValueOrContainer};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::view::{
    ColumnConfig, FilterNode, GroupConfig, SortKey, ViewDef, ViewKind, validate_view,
};
use super::{
    DatabaseDoc, VIEW_ORDER, VIEWS, check_name, fresh_id, list_ids, move_in_list, remove_from_list,
};
use crate::error::{Error, Result};

const KEY_NAME: &str = "name";
const KEY_KIND: &str = "kind";
const KEY_FILTER: &str = "filter";
const KEY_SORTS: &str = "sorts";
const KEY_GROUP: &str = "group";
const KEY_COLUMNS: &str = "columns";
const KEY_DATE_PROPERTY: &str = "date_property";

fn text(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn json<T: DeserializeOwned>(map: &LoroMap, key: &str) -> Option<T> {
    serde_json::from_str(&text(map, key)?).ok()
}

fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Invalid(e.to_string()))
}

fn read_view(id: &str, map: &LoroMap) -> Option<ViewDef> {
    let kind: ViewKind =
        serde_json::from_value(serde_json::Value::String(text(map, KEY_KIND)?)).ok()?;
    Some(ViewDef {
        id: id.to_owned(),
        name: text(map, KEY_NAME).unwrap_or_default(),
        kind,
        filter: json::<FilterNode>(map, KEY_FILTER),
        sorts: json::<Vec<SortKey>>(map, KEY_SORTS).unwrap_or_default(),
        group: json::<GroupConfig>(map, KEY_GROUP),
        columns: json::<Vec<ColumnConfig>>(map, KEY_COLUMNS).unwrap_or_default(),
        date_property: text(map, KEY_DATE_PROPERTY),
    })
}

/// Writes `key` only when the stored text differs, so saving a view whose
/// other fields did not change leaves them as they are for a concurrent
/// editor; `None` removes the key.
fn put(map: &LoroMap, key: &str, new: Option<String>) -> Result<()> {
    let old = text(map, key);
    if old == new {
        return Ok(());
    }
    match new {
        Some(value) => map.insert(key, value.as_str())?,
        None => map.delete(key)?,
    }
    Ok(())
}

fn kind_text(kind: ViewKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl DatabaseDoc {
    /// The saved views in display order.
    pub fn views(&self) -> Result<Vec<ViewDef>> {
        let views = self.map(VIEWS);
        let mut ids: Vec<String> = Vec::new();
        for id in list_ids(&self.movable(VIEW_ORDER)) {
            if views.get(&id).is_some() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut rest: Vec<String> = views
            .keys()
            .map(|k| k.to_string())
            .filter(|k| !ids.contains(k))
            .collect();
        rest.sort();
        ids.extend(rest);
        Ok(ids
            .iter()
            .filter_map(|id| {
                let map = views.get(id)?.into_container().ok()?.into_map().ok()?;
                read_view(id, &map)
            })
            .collect())
    }

    /// One saved view.
    pub fn view(&self, id: &str) -> Result<ViewDef> {
        self.views()?
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| Error::Missing(format!("view {id}")))
    }

    /// Adds an empty view of `kind` at the end and returns its id.
    pub fn add_view(&self, name: &str, kind: ViewKind) -> Result<String> {
        let name = check_name(name)?;
        let id = fresh_id();
        let map = self.map(VIEWS).insert_container(&id, LoroMap::new())?;
        map.insert(KEY_NAME, name.as_str())?;
        map.insert(KEY_KIND, kind_text(kind))?;
        self.movable(VIEW_ORDER).push(id.as_str())?;
        self.commit();
        Ok(id)
    }

    /// Saves every field of `view` (the view must exist). Only fields that
    /// differ from what is stored are written, so concurrent edits of
    /// different fields of one view (say a filter and a column width) both
    /// survive; the same field converges to one value.
    pub fn save_view(&self, view: &ViewDef) -> Result<()> {
        validate_view(&self.properties()?, view)?;
        let map = self
            .map(VIEWS)
            .get(&view.id)
            .and_then(|v| v.into_container().ok())
            .and_then(|c| c.into_map().ok())
            .ok_or_else(|| Error::Missing(format!("view {}", view.id)))?;
        put(&map, KEY_NAME, Some(check_name(&view.name)?))?;
        put(&map, KEY_KIND, Some(kind_text(view.kind)))?;
        put(
            &map,
            KEY_FILTER,
            view.filter.as_ref().map(encode).transpose()?,
        )?;
        let sorts = (!view.sorts.is_empty())
            .then(|| encode(&view.sorts))
            .transpose()?;
        put(&map, KEY_SORTS, sorts)?;
        put(
            &map,
            KEY_GROUP,
            view.group.as_ref().map(encode).transpose()?,
        )?;
        let columns = (!view.columns.is_empty())
            .then(|| encode(&view.columns))
            .transpose()?;
        put(&map, KEY_COLUMNS, columns)?;
        put(&map, KEY_DATE_PROPERTY, view.date_property.clone())?;
        self.commit();
        Ok(())
    }

    /// Deletes a view. The last view cannot be deleted.
    pub fn delete_view(&self, id: &str) -> Result<()> {
        let views = self.views()?;
        if !views.iter().any(|v| v.id == id) {
            return Err(Error::Missing(format!("view {id}")));
        }
        if views.len() == 1 {
            return Err(Error::Invalid("a database keeps at least one view".into()));
        }
        self.map(VIEWS).delete(id)?;
        remove_from_list(&self.movable(VIEW_ORDER), id)?;
        self.commit();
        Ok(())
    }

    /// Moves a view to `index` among the views.
    pub fn move_view(&self, id: &str, index: usize) -> Result<()> {
        self.view(id)?;
        move_in_list(&self.movable(VIEW_ORDER), id, index)?;
        self.commit();
        Ok(())
    }
}
