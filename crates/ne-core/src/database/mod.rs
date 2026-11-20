//! Databases: a table of rows whose schema, cell values and views live in one
//! CRDT document.
//!
//! # Model
//!
//! A database is a node of the workspace tree (see
//! [`crate::tree::NodeKind::Database`]). Its rows are real page nodes that are
//! children of that node: each has its own content document and opens like any
//! page. The *title* property is the row page's title in the tree. Everything
//! else about a row sits in the database's own [`DatabaseDoc`], so queries
//! never open a row page.
//!
//! # Document layout
//!
//! | Root container | Contents |
//! |---|---|
//! | `ne` (map) | `role = "database"`, the marker that tells a sync receiver what the document is, see [`crate::roles`] |
//! | `props` (map) | property id → map (`name`, `type`, `relation`, `options`, `option_order`) |
//! | `prop_order` (movable list) | property ids in display order |
//! | `cells` (map) | `"<row id>/<property id>"` → the cell value as JSON text |
//! | `views` (map) | view id → map (`name`, `kind`, `filter`, `sorts`, `group`, `columns`, `date_property`) |
//! | `view_order` (movable list) | view ids in display order |
//!
//! # Concurrency
//!
//! Every cell is a separate key of one flat map, so two devices editing
//! different cells (of the same row too) both keep their edit, and two devices
//! editing the *same* cell converge to one value (the later write in Loro's
//! order wins). Nothing nests below a row, so there is no container that two
//! devices could create at once and lose one of.
//!
//! The property definition always wins over cell contents:
//!
//! * A schema change races a cell edit: both are kept. The cell is converted on
//!   read to the property's current type ([`coerce`]), so the outcome is the
//!   same on every device and never corrupt; at worst a value reads as empty
//!   until the property's type is changed back.
//! * A property is deleted while another device writes to it: the property
//!   stays deleted, and the stray cell is ignored by every reader and removed
//!   by [`DatabaseDoc::purge_orphans`].
//! * An option is deleted while another device picks it: the option stays
//!   deleted and the cell reads without it.
//! * Order of properties, options and views is a movable list, so concurrent
//!   moves converge. The order of *rows* is the sibling order in the tree.
//!
//! Two-way relations store their links on the forward side only; see
//! [`RelationConfig`].

mod cell;
mod cells;
mod date;
mod query;
mod schema;
mod types;
mod view;
mod views;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use loro::event::Diff;
use loro::{
    ContainerID, ExportMode, LoroDoc, LoroMap, LoroMovableList, Subscription, VersionVector,
};

use crate::error::{Error, Result};
pub use crate::tree::ImportOutcome;
pub use cell::{
    MAX_LIST_ENTRIES, MAX_TEXT_CHARS, coerce, fold_name, number_text, validate as validate_cell,
};
pub use cells::RowValues;
pub use date::{DateValue, day_of, is_valid_date, to_epoch_ms};
pub use query::{Bucket, DEFAULT_LIMIT, DateRange, MAX_LIMIT, QueryResult, QueryRow, QuerySpec};
pub use schema::{MAX_OPTIONS, Schema};
pub use types::{
    CellValue, MAX_NAME_CHARS, MAX_PROPERTIES, OPTION_COLORS, Property, PropertyType,
    RelationConfig, SelectOption, TITLE_PROPERTY,
};
pub use view::{
    ColumnConfig, FilterNode, GroupConfig, Logic, MAX_FILTER_DEPTH, MAX_SORT_KEYS, Operator,
    SortKey, ViewDef, ViewKind, fold_filter_text, operators_for, validate_filter, validate_group,
    validate_sorts, validate_view,
};

pub(crate) const PROPS: &str = "props";
pub(crate) const PROP_ORDER: &str = "prop_order";
pub(crate) const CELLS: &str = "cells";
pub(crate) const VIEWS: &str = "views";
pub(crate) const VIEW_ORDER: &str = "view_order";

/// A fresh random id for a property, option or view.
pub fn fresh_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_owned()
}

/// What changed in the document since the last [`DatabaseDoc::take_changes`],
/// from local edits and imports alike. The store re-indexes exactly this.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DbChanges {
    /// Ids of rows whose cells changed.
    pub rows: BTreeSet<String>,
    /// A property or option was added, changed or removed: every row's
    /// effective values may differ.
    pub schema: bool,
    /// A view changed (no effect on the index).
    pub views: bool,
}

impl DbChanges {
    /// True when nothing changed.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && !self.schema && !self.views
    }
}

#[derive(Clone, Default)]
struct ChangeLog(Arc<Mutex<DbChanges>>);

impl ChangeLog {
    fn subscribe(&self, doc: &LoroDoc) -> Subscription {
        let log = self.0.clone();
        doc.subscribe_root(Arc::new(move |event| {
            let mut changes = log.lock().unwrap_or_else(PoisonError::into_inner);
            for container in &event.events {
                let root = match container.target {
                    ContainerID::Root { name, .. } => Some(name.to_string()),
                    ContainerID::Normal { .. } => {
                        container.path.first().and_then(|(id, _)| match id {
                            ContainerID::Root { name, .. } => Some(name.to_string()),
                            ContainerID::Normal { .. } => None,
                        })
                    }
                };
                match root.as_deref() {
                    Some(CELLS) => {
                        if let Diff::Map(map) = &container.diff {
                            for key in map.updated.keys() {
                                if let Some((row, _)) = key.split_once('/') {
                                    changes.rows.insert(row.to_owned());
                                }
                            }
                        }
                    }
                    Some(PROPS | PROP_ORDER) => changes.schema = true,
                    Some(VIEWS | VIEW_ORDER) => changes.views = true,
                    _ => {}
                }
            }
        }))
    }
}

/// Typed API over the CRDT document of one database. See the
/// [module documentation](self) for the model and the concurrency rules.
pub struct DatabaseDoc {
    doc: LoroDoc,
    changes: ChangeLog,
    // Dropping the guard would stop change tracking.
    _subscription: Subscription,
}

impl DatabaseDoc {
    fn from_doc(doc: LoroDoc) -> Self {
        let changes = ChangeLog::default();
        let subscription = changes.subscribe(&doc);
        Self {
            doc,
            changes,
            _subscription: subscription,
        }
    }

    /// Creates an empty database with one table view. Only the device that
    /// creates the database does this; others receive the document by sync.
    pub fn new() -> Result<Self> {
        let db = Self::from_doc(LoroDoc::new());
        crate::roles::write_role(&db.doc, crate::roles::Role::Database)?;
        db.add_view("Table", ViewKind::Table)?;
        db.take_changes();
        Ok(db)
    }

    /// Loads a database from bytes produced by [`Self::export_snapshot`] or
    /// from the updates that created it. The content is not checked until read.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        let db = Self::from_doc(doc);
        db.take_changes();
        Ok(db)
    }

    /// True when `bytes` (a snapshot or the first update of a document) are a
    /// database document.
    pub fn is_database(bytes: &[u8]) -> bool {
        crate::roles::role_of(bytes) == Some(crate::roles::Role::Database)
    }

    /// Sets this replica's id. Do it before the first local edit.
    pub fn set_peer_id(&self, peer_id: u64) -> Result<()> {
        Ok(self.doc.set_peer_id(peer_id)?)
    }

    /// The Loro peer id of this replica.
    pub fn peer_id(&self) -> u64 {
        self.doc.peer_id()
    }

    // ---- sync ----

    /// Exports the full state, including history.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// Exports the whole history as updates, the form in which a new
    /// database's first state is queued for sync.
    pub fn export_all_updates(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::all_updates())?)
    }

    /// Exports every change a replica at `version` has not seen yet.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let vv = VersionVector::decode(version)?;
        self.doc.commit();
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    /// Applies a snapshot or updates from another replica. What it changed is
    /// reported by [`Self::take_changes`].
    pub fn import(&self, bytes: &[u8]) -> Result<ImportOutcome> {
        let status = self.doc.import(bytes)?;
        Ok(ImportOutcome {
            has_missing_dependencies: status.pending.is_some(),
        })
    }

    /// The current version vector as opaque bytes.
    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    /// Drains what changed since the previous call.
    pub fn take_changes(&self) -> DbChanges {
        std::mem::take(
            &mut *self
                .changes
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Runs `edit` and returns its result with the update bytes holding
    /// exactly the changes it made, ready to queue for sync.
    pub fn edit<R>(&self, edit: impl FnOnce(&Self) -> Result<R>) -> Result<(R, Vec<u8>)> {
        self.doc.commit();
        let before = self.doc.oplog_vv();
        let result = edit(self);
        self.doc.commit();
        let update = self.doc.export(ExportMode::updates(&before))?;
        result.map(|value| (value, update))
    }

    // ---- shared helpers ----

    pub(crate) fn map(&self, name: &str) -> LoroMap {
        self.doc.get_map(name)
    }

    pub(crate) fn movable(&self, name: &str) -> LoroMovableList {
        self.doc.get_movable_list(name)
    }

    pub(crate) fn commit(&self) {
        self.doc.commit();
    }
}

/// Position of `id` in a list of string ids.
pub(crate) fn index_of(list: &LoroMovableList, id: &str) -> Option<usize> {
    (0..list.len()).find(|&i| {
        list.get(i)
            .and_then(|v| v.into_value().ok())
            .and_then(|v| v.into_string().ok())
            .is_some_and(|s| s.as_str() == id)
    })
}

/// Moves `id` to `index` (clamped to the end) in an id list; fails if absent.
pub(crate) fn move_in_list(list: &LoroMovableList, id: &str, index: usize) -> Result<()> {
    let from = index_of(list, id).ok_or_else(|| Error::Missing(id.to_owned()))?;
    let to = index.min(list.len().saturating_sub(1));
    if from != to {
        list.mov(from, to)?;
    }
    Ok(())
}

/// Removes `id` from an id list if present.
pub(crate) fn remove_from_list(list: &LoroMovableList, id: &str) -> Result<()> {
    if let Some(i) = index_of(list, id) {
        list.delete(i, 1)?;
    }
    Ok(())
}

/// The ids of a list, in order.
pub(crate) fn list_ids(list: &LoroMovableList) -> Vec<String> {
    list.to_vec()
        .into_iter()
        .filter_map(|v| v.into_string().ok().map(|s| s.to_string()))
        .collect()
}

pub(crate) fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::Invalid(format!(
            "a name is 1 to {MAX_NAME_CHARS} characters"
        )));
    }
    Ok(name.to_owned())
}
