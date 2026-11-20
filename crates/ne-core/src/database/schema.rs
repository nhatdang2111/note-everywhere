//! Reading and changing the properties of a database.

use std::collections::BTreeSet;

use loro::{LoroMap, LoroMovableList, LoroValue, ValueOrContainer};

use super::cell::{as_text, fold_name};
use super::types::{
    MAX_PROPERTIES, OPTION_COLORS, Property, PropertyType, RelationConfig, SelectOption,
};
use super::view::ViewDef;
use super::{
    DatabaseDoc, PROP_ORDER, PROPS, check_name, fresh_id, list_ids, move_in_list, remove_from_list,
};
use crate::error::{Error, Result};

const KEY_NAME: &str = "name";
const KEY_TYPE: &str = "type";
const KEY_RELATION: &str = "relation";
const KEY_OPTIONS: &str = "options";
const KEY_OPTION_ORDER: &str = "option_order";
const KEY_COLOR: &str = "color";

/// Most options of one property.
pub const MAX_OPTIONS: usize = 500;

/// The shape of a database: its properties and its views, in display order.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Schema {
    /// Properties in display order (the title is implicit and not listed).
    pub properties: Vec<Property>,
    /// Views in display order.
    pub views: Vec<ViewDef>,
}

impl Schema {
    /// The property with this id.
    pub fn property(&self, id: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.id == id)
    }
}

fn string_of(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn child_map(map: &LoroMap, key: &str) -> Option<LoroMap> {
    map.get(key)?.into_container().ok()?.into_map().ok()
}

fn child_list(map: &LoroMap, key: &str) -> Option<LoroMovableList> {
    map.get(key)?
        .into_container()
        .ok()?
        .into_movable_list()
        .ok()
}

fn parse_type(text: &str) -> Option<PropertyType> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

fn type_text(ty: PropertyType) -> String {
    serde_json::to_value(ty)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Options of a property in display order. Options missing from the order
/// list (it is only a hint after concurrent edits) follow, sorted by id.
fn read_options(prop: &LoroMap) -> Vec<SelectOption> {
    let Some(options) = child_map(prop, KEY_OPTIONS) else {
        return Vec::new();
    };
    let order = child_list(prop, KEY_OPTION_ORDER)
        .map(|l| list_ids(&l))
        .unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    for id in order {
        if options.get(&id).is_some() && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut rest: Vec<String> = options
        .keys()
        .map(|k| k.to_string())
        .filter(|k| !ids.contains(k))
        .collect();
    rest.sort();
    ids.extend(rest);
    ids.into_iter()
        .filter_map(|id| {
            let option = child_map(&options, &id)?;
            Some(SelectOption {
                name: string_of(&option, KEY_NAME).unwrap_or_default(),
                color: string_of(&option, KEY_COLOR).unwrap_or_else(|| "gray".into()),
                id,
            })
        })
        .collect()
}

fn read_property(id: &str, prop: &LoroMap) -> Option<Property> {
    let ty = parse_type(&string_of(prop, KEY_TYPE)?)?;
    let relation: Option<RelationConfig> =
        string_of(prop, KEY_RELATION).and_then(|json| serde_json::from_str(&json).ok());
    Some(Property {
        id: id.to_owned(),
        name: string_of(prop, KEY_NAME).unwrap_or_default(),
        ty,
        options: read_options(prop),
        relation: if ty == PropertyType::Relation {
            relation
        } else {
            None
        },
    })
}

fn color_for(id: &str) -> String {
    let sum: usize = id.bytes().map(usize::from).sum();
    OPTION_COLORS[sum % OPTION_COLORS.len()].to_owned()
}

impl DatabaseDoc {
    /// Properties in display order. A property that another peer left half
    /// written (no readable type) is skipped.
    pub fn properties(&self) -> Result<Vec<Property>> {
        let props = self.map(PROPS);
        let mut ids: Vec<String> = Vec::new();
        for id in list_ids(&self.movable(PROP_ORDER)) {
            if props.get(&id).is_some() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut rest: Vec<String> = props
            .keys()
            .map(|k| k.to_string())
            .filter(|k| !ids.contains(k))
            .collect();
        rest.sort();
        ids.extend(rest);
        Ok(ids
            .iter()
            .filter_map(|id| read_property(id, &child_map(&props, id)?))
            .collect())
    }

    /// One property.
    pub fn property(&self, id: &str) -> Result<Property> {
        let props = self.map(PROPS);
        child_map(&props, id)
            .and_then(|m| read_property(id, &m))
            .ok_or_else(|| Error::Missing(format!("property {id}")))
    }

    /// The properties and views.
    pub fn schema(&self) -> Result<Schema> {
        Ok(Schema {
            properties: self.properties()?,
            views: self.views()?,
        })
    }

    fn property_map(&self, id: &str) -> Result<LoroMap> {
        child_map(&self.map(PROPS), id).ok_or_else(|| Error::Missing(format!("property {id}")))
    }

    /// Adds a property at the end and returns its id. A relation property
    /// needs `relation`; other types must not have it.
    pub fn add_property(
        &self,
        name: &str,
        ty: PropertyType,
        relation: Option<RelationConfig>,
    ) -> Result<String> {
        let name = check_name(name)?;
        check_relation(ty, relation.as_ref())?;
        let props = self.map(PROPS);
        if props.len() >= MAX_PROPERTIES {
            return Err(Error::Invalid(format!(
                "a database has at most {MAX_PROPERTIES} properties"
            )));
        }
        let id = fresh_id();
        let prop = props.insert_container(&id, LoroMap::new())?;
        prop.insert(KEY_NAME, name.as_str())?;
        prop.insert(KEY_TYPE, type_text(ty))?;
        if let Some(relation) = relation {
            prop.insert(KEY_RELATION, encode(&relation)?)?;
        }
        // The containers that hold options exist from the start so that two
        // devices adding the first options never both create them.
        prop.insert_container(KEY_OPTIONS, LoroMap::new())?;
        prop.insert_container(KEY_OPTION_ORDER, LoroMovableList::new())?;
        self.movable(PROP_ORDER).push(id.as_str())?;
        self.commit();
        Ok(id)
    }

    /// Renames a property.
    pub fn rename_property(&self, id: &str, name: &str) -> Result<()> {
        let name = check_name(name)?;
        self.property_map(id)?.insert(KEY_NAME, name.as_str())?;
        self.commit();
        Ok(())
    }

    /// Deletes a property. Its cells stay in the document, unread, until
    /// [`Self::purge_orphans`]; views that mention it ignore it.
    pub fn delete_property(&self, id: &str) -> Result<()> {
        self.property_map(id)?;
        self.map(PROPS).delete(id)?;
        remove_from_list(&self.movable(PROP_ORDER), id)?;
        self.commit();
        Ok(())
    }

    /// Moves a property to `index` among the properties.
    pub fn move_property(&self, id: &str, index: usize) -> Result<()> {
        self.property_map(id)?;
        move_in_list(&self.movable(PROP_ORDER), id, index)?;
        self.commit();
        Ok(())
    }

    /// Changes a property's type. No cell is rewritten: values convert on
    /// read, see [`super::coerce`] for the table, so changing back loses
    /// nothing. Changing text to select or multi-select first creates the
    /// options the existing values name.
    pub fn change_property_type(
        &self,
        id: &str,
        ty: PropertyType,
        relation: Option<RelationConfig>,
    ) -> Result<()> {
        let old = self.property(id)?;
        check_relation(ty, relation.as_ref())?;
        if old.is_backlink() && ty != PropertyType::Relation {
            return Err(Error::Invalid(
                "a backlink property cannot change type".into(),
            ));
        }
        if matches!(ty, PropertyType::Select | PropertyType::MultiSelect)
            && matches!(old.ty, PropertyType::Text | PropertyType::Url)
        {
            self.create_options_for_values(&old, ty == PropertyType::MultiSelect)?;
        }
        let prop = self.property_map(id)?;
        prop.insert(KEY_TYPE, type_text(ty))?;
        match relation {
            Some(relation) => prop.insert(KEY_RELATION, encode(&relation)?)?,
            None if ty != PropertyType::Relation => prop.delete(KEY_RELATION)?,
            None => {}
        }
        self.commit();
        Ok(())
    }

    fn create_options_for_values(&self, old: &Property, multi: bool) -> Result<()> {
        let mut known: BTreeSet<String> = old.options.iter().map(|o| fold_name(&o.name)).collect();
        let mut room = MAX_OPTIONS.saturating_sub(old.options.len());
        for (_, value) in self.cells_of_property(&old.id) {
            let Some(text) = as_text(&value, old) else {
                continue;
            };
            let names: Vec<&str> = if multi {
                text.split(',').collect()
            } else {
                vec![text.as_str()]
            };
            for name in names {
                let name = name.trim();
                if room == 0 {
                    return Ok(());
                }
                if name.is_empty() || name.chars().count() > super::MAX_NAME_CHARS {
                    continue;
                }
                if known.insert(fold_name(name)) {
                    self.add_option(&old.id, name, None)?;
                    room -= 1;
                }
            }
        }
        Ok(())
    }

    // ---- options ----

    fn options_of(&self, property: &str) -> Result<(LoroMap, LoroMovableList)> {
        let prop = self.property_map(property)?;
        let options = child_map(&prop, KEY_OPTIONS)
            .ok_or_else(|| Error::Invalid("the property has no option list".into()))?;
        let order = child_list(&prop, KEY_OPTION_ORDER)
            .ok_or_else(|| Error::Invalid("the property has no option order".into()))?;
        Ok((options, order))
    }

    /// Adds an option at the end and returns its id. The colour is one of
    /// [`super::OPTION_COLORS`]; without one it is assigned from the id.
    pub fn add_option(&self, property: &str, name: &str, color: Option<&str>) -> Result<String> {
        let name = check_name(name)?;
        let current = self.property(property)?;
        if current.options.len() >= MAX_OPTIONS {
            return Err(Error::Invalid(format!(
                "a property has at most {MAX_OPTIONS} options"
            )));
        }
        let wanted = fold_name(&name);
        if current.options.iter().any(|o| fold_name(&o.name) == wanted) {
            return Err(Error::Invalid(format!("option {name:?} already exists")));
        }
        let (options, order) = self.options_of(property)?;
        let id = fresh_id();
        let color = match color {
            Some(c) if OPTION_COLORS.contains(&c) => c.to_owned(),
            Some(c) => return Err(Error::Invalid(format!("unknown colour {c:?}"))),
            None => color_for(&id),
        };
        let option = options.insert_container(&id, LoroMap::new())?;
        option.insert(KEY_NAME, name.as_str())?;
        option.insert(KEY_COLOR, color.as_str())?;
        order.push(id.as_str())?;
        self.commit();
        Ok(id)
    }

    fn option_map(&self, property: &str, option: &str) -> Result<LoroMap> {
        let (options, _) = self.options_of(property)?;
        child_map(&options, option).ok_or_else(|| Error::Missing(format!("option {option}")))
    }

    /// Renames an option. Values refer to the id, so none changes.
    pub fn rename_option(&self, property: &str, option: &str, name: &str) -> Result<()> {
        let name = check_name(name)?;
        let wanted = fold_name(&name);
        let current = self.property(property)?;
        if current
            .options
            .iter()
            .any(|o| o.id != option && fold_name(&o.name) == wanted)
        {
            return Err(Error::Invalid(format!("option {name:?} already exists")));
        }
        self.option_map(property, option)?
            .insert(KEY_NAME, name.as_str())?;
        self.commit();
        Ok(())
    }

    /// Changes an option's colour.
    pub fn set_option_color(&self, property: &str, option: &str, color: &str) -> Result<()> {
        if !OPTION_COLORS.contains(&color) {
            return Err(Error::Invalid(format!("unknown colour {color:?}")));
        }
        self.option_map(property, option)?
            .insert(KEY_COLOR, color)?;
        self.commit();
        Ok(())
    }

    /// Deletes an option. Cells that held it read without it from now on.
    pub fn delete_option(&self, property: &str, option: &str) -> Result<()> {
        self.option_map(property, option)?;
        let (options, order) = self.options_of(property)?;
        options.delete(option)?;
        remove_from_list(&order, option)?;
        self.commit();
        Ok(())
    }

    /// Moves an option to `index` among the property's options.
    pub fn move_option(&self, property: &str, option: &str, index: usize) -> Result<()> {
        self.option_map(property, option)?;
        let (_, order) = self.options_of(property)?;
        move_in_list(&order, option, index)?;
        self.commit();
        Ok(())
    }
}

fn check_relation(ty: PropertyType, relation: Option<&RelationConfig>) -> Result<()> {
    match (ty, relation) {
        (PropertyType::Relation, Some(r)) if !r.database.is_empty() => Ok(()),
        (PropertyType::Relation, _) => Err(Error::Invalid(
            "a relation property needs the database it links to".into(),
        )),
        (_, Some(_)) => Err(Error::Invalid(
            "only a relation property has a relation".into(),
        )),
        (_, None) => Ok(()),
    }
}

fn encode(relation: &RelationConfig) -> Result<String> {
    serde_json::to_string(relation).map_err(|e| Error::Invalid(e.to_string()))
}
