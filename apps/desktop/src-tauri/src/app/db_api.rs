//! Databases for the UI: schema, rows, cells, views and queries.
//!
//! Every mutating call emits `database:changed` and wakes the sync engine.
//! Changes made on other devices reach the UI as `tree:changed` events whose
//! node ids include the database and the rows whose cells changed.

use std::collections::BTreeMap;

use ne_core::database::{
    CellValue, DatabaseDoc, PropertyType, QueryResult, QuerySpec, RelationConfig, ViewDef,
    ViewKind, validate_filter, validate_group, validate_sorts,
};
use ne_core::tree::Position;
use ne_store::Change;
use serde::{Deserialize, Serialize};

use super::{AppCore, AppError, AppResult, CoreEvent, node_id};

/// The properties and views of a database, as the UI reads them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatabaseSchema {
    pub properties: Vec<ne_core::database::Property>,
    pub views: Vec<ViewDef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbQueryArgs {
    pub id: String,
    pub view_id: Option<String>,
    pub spec: Option<QuerySpec>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbPropertyAddArgs {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub ty: PropertyType,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbPropertyChangeTypeArgs {
    pub id: String,
    pub property_id: String,
    #[serde(rename = "type")]
    pub ty: PropertyType,
    pub relation_database_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbRowCreateArgs {
    pub id: String,
    pub title: Option<String>,
    pub index: Option<usize>,
    pub values: Option<BTreeMap<String, CellValue>>,
}

impl AppCore {
    /// Tells the UI what a database edit changed and wakes the sync engine.
    pub(crate) fn database_changed(&self, change: &Change) {
        let Some(database) = change.page else { return };
        self.emit(CoreEvent::DatabaseChanged {
            database_id: database.to_string(),
            row_ids: change
                .nodes
                .iter()
                .filter(|id| **id != database)
                .map(ToString::to_string)
                .collect(),
        });
        self.after_local_change();
        self.schedule_maintenance();
    }

    fn db_edit<R>(
        &self,
        id: &str,
        edit: impl FnOnce(&DatabaseDoc) -> ne_core::Result<R>,
    ) -> AppResult<R> {
        let db = node_id(id)?;
        let (value, change) = self.store(|ws| ws.database_edit(db, edit))?;
        self.database_changed(&change);
        Ok(value)
    }

    pub async fn db_create(
        &self,
        parent_id: Option<&str>,
        index: Option<usize>,
        title: Option<&str>,
    ) -> AppResult<String> {
        let parent = parent_id.map(node_id).transpose()?;
        let id = self.store(|ws| {
            let place = match index {
                Some(i) => Position::Index(i.min(ws.children(parent)?.len())),
                None => Position::End,
            };
            ws.create_database(parent, place, title.unwrap_or_default())
        })?;
        self.emit(CoreEvent::TreeChanged {
            node_ids: vec![id.to_string()],
        });
        self.after_local_change();
        Ok(id.to_string())
    }

    pub async fn db_schema(&self, id: &str) -> AppResult<DatabaseSchema> {
        let db = node_id(id)?;
        let schema = self.store(|ws| ws.database_schema(db))?;
        Ok(DatabaseSchema {
            properties: schema.properties,
            views: schema.views,
        })
    }

    pub async fn db_query(&self, args: DbQueryArgs) -> AppResult<QueryResult> {
        let db = node_id(&args.id)?;
        self.store(|ws| {
            let schema = ws.database_schema(db)?;
            let mut spec = match (args.spec, args.view_id.as_deref()) {
                (Some(spec), _) => {
                    // A request built by the UI is checked; a saved view is
                    // not, so a stale view keeps working.
                    if let Some(filter) = &spec.filter {
                        validate_filter(&schema.properties, filter)?;
                    }
                    validate_sorts(&schema.properties, &spec.sorts)?;
                    if let Some(group) = &spec.group {
                        validate_group(&schema.properties, group)?;
                    }
                    spec
                }
                (None, Some(view)) => {
                    let view = schema
                        .views
                        .iter()
                        .find(|v| v.id == view)
                        .ok_or_else(|| ne_core::Error::Missing(format!("view {view}")))?;
                    QuerySpec::from_view(view)
                }
                (None, None) => QuerySpec::default(),
            };
            if let Some(offset) = args.offset {
                spec.offset = offset;
            }
            if let Some(limit) = args.limit {
                spec.limit = limit;
            }
            ws.database_query(db, &spec)
        })
    }

    pub async fn db_property_add(&self, args: DbPropertyAddArgs) -> AppResult<String> {
        if args.ty == PropertyType::Relation {
            return Err(AppError::invalid(
                "Add a relation with the relation command, which needs the database it links to.",
            ));
        }
        self.db_edit(&args.id, |doc| doc.add_property(&args.name, args.ty, None))
    }

    pub async fn db_property_rename(&self, id: &str, property: &str, name: &str) -> AppResult<()> {
        self.db_edit(id, |doc| doc.rename_property(property, name))
    }

    pub async fn db_property_delete(&self, id: &str, property: &str) -> AppResult<()> {
        let db = node_id(id)?;
        let change = self.store(|ws| ws.database_delete_property(db, property))?;
        self.database_changed(&change);
        Ok(())
    }

    pub async fn db_property_move(&self, id: &str, property: &str, index: usize) -> AppResult<()> {
        self.db_edit(id, |doc| doc.move_property(property, index))
    }

    pub async fn db_property_change_type(&self, args: DbPropertyChangeTypeArgs) -> AppResult<()> {
        let relation = match (args.ty, args.relation_database_id.as_deref()) {
            (PropertyType::Relation, Some(target)) => {
                let target = node_id(target)?;
                self.store(|ws| ws.database_schema(target).map(drop))?;
                Some(RelationConfig {
                    database: target.to_string(),
                    reverse_property: None,
                    backlink_of: None,
                })
            }
            (PropertyType::Relation, None) => {
                return Err(AppError::invalid(
                    "A relation needs the database it links to.",
                ));
            }
            _ => None,
        };
        self.db_edit(&args.id, |doc| {
            doc.change_property_type(&args.property_id, args.ty, relation)
        })
    }

    pub async fn db_relation_add(
        &self,
        id: &str,
        name: &str,
        target: &str,
        reverse_name: Option<&str>,
    ) -> AppResult<String> {
        let (db, target) = (node_id(id)?, node_id(target)?);
        let property = self.store(|ws| ws.database_add_relation(db, name, target, reverse_name))?;
        for database in [db, target] {
            self.emit(CoreEvent::DatabaseChanged {
                database_id: database.to_string(),
                row_ids: Vec::new(),
            });
        }
        self.after_local_change();
        Ok(property)
    }

    pub async fn db_option_add(
        &self,
        id: &str,
        property: &str,
        name: &str,
        color: Option<&str>,
    ) -> AppResult<String> {
        self.db_edit(id, |doc| doc.add_option(property, name, color))
    }

    pub async fn db_option_rename(
        &self,
        id: &str,
        property: &str,
        option: &str,
        name: &str,
    ) -> AppResult<()> {
        self.db_edit(id, |doc| doc.rename_option(property, option, name))
    }

    pub async fn db_option_color(
        &self,
        id: &str,
        property: &str,
        option: &str,
        color: &str,
    ) -> AppResult<()> {
        self.db_edit(id, |doc| doc.set_option_color(property, option, color))
    }

    pub async fn db_option_delete(&self, id: &str, property: &str, option: &str) -> AppResult<()> {
        self.db_edit(id, |doc| doc.delete_option(property, option))
    }

    pub async fn db_option_move(
        &self,
        id: &str,
        property: &str,
        option: &str,
        index: usize,
    ) -> AppResult<()> {
        self.db_edit(id, |doc| doc.move_option(property, option, index))
    }

    pub async fn db_row_create(&self, args: DbRowCreateArgs) -> AppResult<String> {
        let db = node_id(&args.id)?;
        let title = args.title.unwrap_or_default();
        let values = args.values.unwrap_or_default();
        let row = self.store(|ws| {
            let place = match args.index {
                Some(i) => Position::Index(i.min(ws.children(Some(db))?.len())),
                None => Position::End,
            };
            ws.database_create_row(db, &title, place, &values)
        })?;
        self.emit(CoreEvent::DatabaseChanged {
            database_id: db.to_string(),
            row_ids: vec![row.to_string()],
        });
        self.after_local_change();
        self.schedule_maintenance();
        Ok(row.to_string())
    }

    pub async fn db_cell_set(
        &self,
        id: &str,
        row: &str,
        property: &str,
        value: Option<CellValue>,
    ) -> AppResult<()> {
        let (db, row_id) = (node_id(id)?, node_id(row)?);
        let mut change = self.store(|ws| ws.database_set_cell(db, row_id, property, value))?;
        if change.page.is_none() {
            // A title edit renames the row's page and touches no document.
            change.page = Some(db);
            change.nodes.insert(row_id);
        }
        self.database_changed(&change);
        Ok(())
    }

    pub async fn db_card_move(
        &self,
        id: &str,
        row: &str,
        property: &str,
        group: Option<&str>,
        before: Option<&str>,
    ) -> AppResult<()> {
        let (db, row_id) = (node_id(id)?, node_id(row)?);
        let before = before.map(node_id).transpose()?;
        self.store(|ws| ws.database_move_card(db, row_id, property, group, before))?;
        self.emit(CoreEvent::DatabaseChanged {
            database_id: db.to_string(),
            row_ids: vec![row_id.to_string()],
        });
        self.emit(CoreEvent::TreeChanged {
            node_ids: vec![row_id.to_string()],
        });
        self.after_local_change();
        Ok(())
    }

    pub async fn db_view_add(&self, id: &str, name: &str, kind: ViewKind) -> AppResult<String> {
        self.db_edit(id, |doc| doc.add_view(name, kind))
    }

    pub async fn db_view_save(&self, id: &str, view: ViewDef) -> AppResult<()> {
        self.db_edit(id, |doc| doc.save_view(&view))
    }

    pub async fn db_view_delete(&self, id: &str, view: &str) -> AppResult<()> {
        self.db_edit(id, |doc| doc.delete_view(view))
    }

    pub async fn db_view_move(&self, id: &str, view: &str, index: usize) -> AppResult<()> {
        self.db_edit(id, |doc| doc.move_view(view, index))
    }
}
