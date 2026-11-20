//! Database operations of the [`Workspace`]: creating databases and rows,
//! editing cells and the schema, relations, board cards and queries.
//!
//! Every edit goes through [`Workspace::database_edit`], which persists the
//! change, refreshes the cell index and queues the update for sync in one
//! transaction. The rules of the document (types, concurrency) are in
//! `ne_core::database`.

use std::collections::{BTreeMap, BTreeSet};

use ne_core::NodeId;
use ne_core::database::{
    CellValue, DatabaseDoc, PropertyType, QueryResult, QuerySpec, RelationConfig, Schema,
    TITLE_PROPERTY,
};
use ne_core::tree::{NodeKind, Position};
use rusqlite::{OptionalExtension, params};

use crate::db_query::{self, Request};
use crate::docs::{self, DocKind, DocRef};
use crate::error::{Error, Result};
use crate::outbox;
use crate::workspace::{Change, Workspace};

impl Workspace {
    /// Creates a database node under `parent` (`None` is the top level) with
    /// its document: one table view, no properties, no rows.
    pub fn create_database(
        &mut self,
        parent: Option<NodeId>,
        position: Position,
        title: &str,
    ) -> Result<NodeId> {
        let doc = DatabaseDoc::new()?;
        doc.set_peer_id(self.peer_id)?;
        let now = self.now();
        let id = self.local_tree_change(
            |tree| tree.create_database(parent, position, title),
            |_| Vec::new(),
            |conn, id, _| {
                let key = id.to_string();
                docs::insert(
                    conn,
                    &key,
                    DocKind::Page,
                    &doc.export_snapshot()?,
                    &doc.version(),
                    now,
                )?;
                docs::set_role(conn, &key, ne_core::roles::Role::Database)?;
                outbox::enqueue(conn, &DocRef::Page(*id), &doc.export_all_updates()?, now)?;
                Ok(())
            },
        )?;
        self.databases.insert(id, doc);
        Ok(id)
    }

    /// The properties and views of a database.
    pub fn database_schema(&mut self, id: NodeId) -> Result<Schema> {
        self.load_database(id)?;
        let doc = self.cached_database(id)?;
        Ok(doc.schema()?)
    }

    pub(crate) fn cached_database(&self, id: NodeId) -> Result<&DatabaseDoc> {
        self.databases
            .get(&id)
            .ok_or_else(|| Error::NotFound(format!("database {id}")))
    }

    /// Edits a database document and persists the change. `edit` sees the
    /// document and must make its changes through its methods. A failed edit
    /// stores nothing. The returned [`Change`] lists the rows whose cells
    /// changed (as nodes) and names the database as the changed document.
    pub fn database_edit<R>(
        &mut self,
        id: NodeId,
        edit: impl FnOnce(&DatabaseDoc) -> ne_core::Result<R>,
    ) -> Result<(R, Change)> {
        self.load_database(id)?;
        let doc = self.cached_database(id)?;
        let before = doc.version();
        let (value, update) = match doc.edit(edit) {
            Ok(done) => done,
            Err(err) => {
                // Part of the edit may have been applied in memory.
                self.databases.remove(&id);
                return Err(err.into());
            }
        };
        if self.cached_database(id)?.version() == before {
            return Ok((value, Change::default()));
        }
        let change = self.commit_database_update(id, &update, true)?;
        Ok((value, change))
    }

    fn require_row(&self, db: NodeId, row: NodeId) -> Result<()> {
        match self.node(row)? {
            Some(info) if info.parent == Some(db) && info.kind == NodeKind::Page => Ok(()),
            _ => Err(Error::InvalidInput(format!(
                "{row} is not a row of database {db}"
            ))),
        }
    }

    /// Creates a row: a page under the database, with default cell values.
    pub fn database_create_row(
        &mut self,
        db: NodeId,
        title: &str,
        position: Position,
        values: &BTreeMap<String, CellValue>,
    ) -> Result<NodeId> {
        self.load_database(db)?;
        let row = self.create_page(Some(db), position, title)?;
        if !values.is_empty() {
            let key = row.to_string();
            self.database_edit(db, |doc| {
                for (property, value) in values {
                    doc.set_cell(&key, property, Some(value.clone()))?;
                }
                Ok(())
            })?;
        }
        Ok(row)
    }

    /// Checks that every id is a visible row of `database`.
    fn require_rows_of(&self, database: &str, ids: &[String]) -> Result<()> {
        for id in ids {
            let found: Option<i64> = self
                .conn
                .prepare_cached(
                    "SELECT 1 FROM nodes WHERE id = ?1 AND parent_id = ?2 AND kind = 'page'",
                )?
                .query_row(params![id, database], |r| r.get(0))
                .optional()?;
            if found.is_none() {
                return Err(Error::InvalidInput(format!(
                    "{id} is not a row of database {database}"
                )));
            }
        }
        Ok(())
    }

    /// Sets or clears one cell of a row. The title property renames the row's
    /// page; a backlink property edits the links of the rows on the other
    /// side; relation values must be rows of the linked database.
    pub fn database_set_cell(
        &mut self,
        db: NodeId,
        row: NodeId,
        property: &str,
        value: Option<CellValue>,
    ) -> Result<Change> {
        self.load_database(db)?;
        self.require_row(db, row)?;
        if property == TITLE_PROPERTY {
            let Some(CellValue::Text(title)) = value else {
                return Err(Error::InvalidInput("the title is text".into()));
            };
            self.rename(row, &title)?;
            return Ok(Change {
                nodes: [row].into(),
                ..Change::default()
            });
        }
        let prop = self.cached_database(db)?.property(property)?;
        if prop.ty == PropertyType::Relation {
            let Some(config) = prop.relation.clone() else {
                return Err(Error::InvalidInput("the relation has no target".into()));
            };
            if let Some(CellValue::Relation(targets)) = &value {
                self.require_rows_of(&config.database, targets)?;
            }
            if config.backlink_of.is_some() {
                return self.set_backlinks(&config, row, value);
            }
        }
        let key = row.to_string();
        let ((), change) = self.database_edit(db, |doc| doc.set_cell(&key, property, value))?;
        Ok(change)
    }

    /// Edits the forward relation cells so that exactly the rows in `value`
    /// link to `row`.
    fn set_backlinks(
        &mut self,
        config: &RelationConfig,
        row: NodeId,
        value: Option<CellValue>,
    ) -> Result<Change> {
        let (Some(forward), Ok(source_db)) = (
            config.backlink_of.clone(),
            config.database.parse::<NodeId>(),
        ) else {
            return Err(Error::InvalidInput("the backlink has no source".into()));
        };
        let wanted: BTreeSet<String> = match value {
            Some(CellValue::Relation(ids)) => ids.into_iter().collect(),
            None => BTreeSet::new(),
            Some(_) => return Err(Error::InvalidInput("a backlink holds rows".into())),
        };
        let target = row.to_string();
        let current: BTreeSet<String> = self
            .conn
            .prepare_cached(
                "SELECT l.src_row FROM db_links l JOIN nodes s ON s.id = l.src_row AND s.hidden = 0
                 WHERE l.prop_id = ?1 AND l.dst_row = ?2",
            )?
            .query_map(params![forward, target], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut change = Change::default();
        for (source, add) in wanted
            .difference(&current)
            .map(|s| (s, true))
            .chain(current.difference(&wanted).map(|s| (s, false)))
        {
            let ((), one) = self.database_edit(source_db, |doc| {
                let existing = match doc.cell(source, &forward)? {
                    Some(CellValue::Relation(ids)) => ids,
                    _ => Vec::new(),
                };
                let mut ids: Vec<String> = existing.into_iter().filter(|i| *i != target).collect();
                if add {
                    ids.push(target.clone());
                }
                doc.set_cell(
                    source,
                    &forward,
                    (!ids.is_empty()).then_some(CellValue::Relation(ids)),
                )
            })?;
            change.nodes.extend(one.nodes);
            change.page = one.page.or(change.page);
        }
        Ok(change)
    }

    /// Adds a relation property to `db` that links to rows of `target`
    /// (which may be `db` itself). With `reverse_name` the relation is
    /// two-way: `target` gets a backlink property of that name. Returns the
    /// id of the property added to `db`.
    pub fn database_add_relation(
        &mut self,
        db: NodeId,
        name: &str,
        target: NodeId,
        reverse_name: Option<&str>,
    ) -> Result<String> {
        self.load_database(target)?;
        let config = RelationConfig {
            database: target.to_string(),
            reverse_property: None,
            backlink_of: None,
        };
        let (forward, _) = self.database_edit(db, |doc| {
            doc.add_property(name, PropertyType::Relation, Some(config.clone()))
        })?;
        if let Some(reverse) = reverse_name {
            let backlink = RelationConfig {
                database: db.to_string(),
                reverse_property: None,
                backlink_of: Some(forward.clone()),
            };
            let (reverse_id, _) = self.database_edit(target, |doc| {
                doc.add_property(reverse, PropertyType::Relation, Some(backlink))
            })?;
            let two_way = RelationConfig {
                reverse_property: Some(reverse_id),
                ..config
            };
            self.database_edit(db, |doc| {
                doc.change_property_type(&forward, PropertyType::Relation, Some(two_way))
            })?;
        }
        Ok(forward)
    }

    /// Deletes a property; the backlink of a two-way relation goes with it.
    pub fn database_delete_property(&mut self, db: NodeId, property: &str) -> Result<Change> {
        self.load_database(db)?;
        let prop = self.cached_database(db)?.property(property)?;
        if let Some(config) = &prop.relation
            && let (Some(reverse), Ok(other)) = (
                config.reverse_property.clone(),
                config.database.parse::<NodeId>(),
            )
            && self.load_database(other).is_ok()
        {
            // The other side may already be gone; deleting it is best effort.
            let _ = self.database_edit(other, |doc| doc.delete_property(&reverse));
        }
        let ((), change) = self.database_edit(db, |doc| doc.delete_property(property))?;
        Ok(change)
    }

    /// Moves a board card: sets the grouping property of `row` to `group` (an
    /// option id, `"true"`/`"false"` for a checkbox, `None` for no value) and
    /// puts the row before `before` in the manual order, or last. The order of
    /// rows is the order of the pages in the tree, so concurrent moves on two
    /// devices converge like any other reordering.
    pub fn database_move_card(
        &mut self,
        db: NodeId,
        row: NodeId,
        group_property: &str,
        group: Option<&str>,
        before: Option<NodeId>,
    ) -> Result<()> {
        self.load_database(db)?;
        self.require_row(db, row)?;
        let prop = self.cached_database(db)?.property(group_property)?;
        let value = match (prop.ty, group) {
            (PropertyType::Select, Some(option)) => Some(CellValue::Select(option.to_owned())),
            (PropertyType::MultiSelect, Some(option)) => {
                Some(CellValue::MultiSelect(vec![option.to_owned()]))
            }
            (PropertyType::Checkbox, Some(flag)) => Some(CellValue::Checkbox(flag == "true")),
            (PropertyType::Select | PropertyType::MultiSelect | PropertyType::Checkbox, None) => {
                None
            }
            _ => {
                return Err(Error::InvalidInput(
                    "cards are grouped by a select, multi-select or checkbox property".into(),
                ));
            }
        };
        let key = row.to_string();
        self.database_edit(db, |doc| doc.set_cell(&key, group_property, value))?;
        let position = match before {
            None => Position::End,
            Some(other) if other == row => return Ok(()),
            Some(other) => {
                self.require_row(db, other)?;
                // Index of `other` among the visible rows, not counting the
                // card being moved.
                let index: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM nodes
                     WHERE parent_id = ?1 AND hidden = 0 AND id != ?2
                       AND position < (SELECT position FROM nodes WHERE id = ?3)",
                    params![db.to_string(), key, other.to_string()],
                    |r| r.get(0),
                )?;
                Position::Index(usize::try_from(index).unwrap_or_default())
            }
        };
        self.reorder(row, position)
    }

    /// Runs a query: filter, sort, group and page. Conditions that no longer
    /// apply to the schema are ignored, so a saved view keeps working after a
    /// property is deleted.
    pub fn database_query(&mut self, db: NodeId, spec: &QuerySpec) -> Result<QueryResult> {
        self.load_database(db)?;
        let properties = self.cached_database(db)?.properties()?;
        let key = db.to_string();
        db_query::run(
            &self.conn,
            &Request {
                db_id: &key,
                properties: &properties,
                spec,
            },
        )
    }

    /// Removes the cells of rows that were deleted for good, and of properties
    /// that no longer exist. Rows in the trash keep theirs.
    pub fn database_purge_orphans(&mut self, db: NodeId) -> Result<usize> {
        self.load_database(db)?;
        let live: BTreeSet<String> = self
            .conn
            .prepare_cached("SELECT id FROM nodes WHERE parent_id = ?1 AND kind = 'page'")?
            .query_map([db.to_string()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let live = live.into_iter().collect();
        let (removed, _) = self.database_edit(db, |doc| doc.purge_orphans(&live))?;
        Ok(removed)
    }

    /// Removes the cells of rows deleted for good, right after the tree
    /// operation that deleted them.
    pub(crate) fn clear_deleted_rows(&mut self, purged: &BTreeMap<String, Vec<String>>) {
        for (db, rows) in purged {
            let Ok(db) = db.parse::<NodeId>() else {
                continue;
            };
            // Best effort: a failure leaves orphan cells that
            // `database_purge_orphans` collects later.
            let _ = self.database_edit(db, |doc| doc.clear_rows(rows));
        }
    }

    /// The effective value of a cell of a row, for callers that need one
    /// value (a reminder that follows a date property).
    pub fn database_cell(
        &mut self,
        db: NodeId,
        row: NodeId,
        property: &str,
    ) -> Result<Option<CellValue>> {
        self.load_database(db)?;
        Ok(self.cached_database(db)?.cell(&row.to_string(), property)?)
    }
}
