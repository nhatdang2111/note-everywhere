//! Templates: saved page content that can be turned into new pages, and, for a
//! database, saved rows with default property values.
//!
//! All templates live in one small document, the *library*, so listing them
//! never opens a page and they sync like any other document. A template stores
//! its content as plain blocks, not as a page document: creating a page from
//! it writes fresh blocks (new containers with new identities, so nothing is
//! shared with the template or with a page made from it earlier).
//!
//! # What a template keeps
//!
//! The content is copied from a page with the page's blocks as they are,
//! except `child_page` blocks, which are dropped: they stand for pages that are
//! children of the source page, and a copy has no such children. Links to
//! other pages (`page_link`, link marks) and synced-block placeholders are
//! kept as they are, so a copy links to the same pages and shows the same
//! shared content.
//!
//! A *row* template belongs to one database and also carries default values
//! for properties. The title of a row made from it is the template's name
//! unless the caller gives one. Defaults for a property that no longer exists,
//! or whose type has changed to one the value does not fit, are skipped when
//! the row is made.
//!
//! Concurrency: a template is a map under its id; renaming and changing its
//! defaults are separate keys. Deleting a template wins over a concurrent
//! rename.

use std::collections::BTreeMap;

use loro::{LoroMap, LoroValue, ValueOrContainer};
use serde::{Deserialize, Serialize};

use crate::database::{CellValue, check_name, fresh_id};
use crate::error::{Error, Result};
use crate::kv_doc::{ImportOutcome, KvDoc};
use crate::page::{Block, BlockKind};
use crate::roles::Role;

const TEMPLATES: &str = "templates";
const KEY_NAME: &str = "name";
const KEY_KIND: &str = "kind";
const KEY_DATABASE: &str = "database";
const KEY_BLOCKS: &str = "blocks";
const KEY_DEFAULTS: &str = "defaults";
const KEY_CREATED_AT: &str = "created_at";

/// Most blocks (counting nested ones) a template may hold.
pub const MAX_TEMPLATE_BLOCKS: usize = 20_000;

/// What a template makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateKind {
    /// A page.
    Page,
    /// A row of one database.
    Row,
}

/// A saved template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Page or row template.
    pub kind: TemplateKind,
    /// For a row template, the node id of its database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// Default property values (property id to value) of a row template.
    #[serde(default)]
    pub defaults: BTreeMap<String, CellValue>,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at: i64,
    /// The content; left out of listings, see `ne_store`.
    #[serde(skip)]
    pub blocks: Vec<Block>,
}

/// The document that holds every template.
pub struct LibraryDoc {
    base: KvDoc,
}

fn text(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn count_blocks(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| 1 + count_blocks(&b.children)).sum()
}

/// The blocks of a page as a template keeps them: `child_page` blocks dropped,
/// at every level.
pub fn template_blocks(blocks: &[Block]) -> Vec<Block> {
    blocks
        .iter()
        .filter(|b| !matches!(b.kind, BlockKind::ChildPage { .. }))
        .map(|b| {
            let mut copy = b.clone();
            copy.children = template_blocks(&b.children);
            copy
        })
        .collect()
}

impl LibraryDoc {
    /// An empty library. Devices that each create one merge when they sync.
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: KvDoc::new(Role::Library)?,
        })
    }

    /// Loads a library from a snapshot or its first update.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            base: KvDoc::from_snapshot(bytes)?,
        })
    }

    /// Sets this replica's id.
    pub fn set_peer_id(&self, peer_id: u64) -> Result<()> {
        self.base.set_peer_id(peer_id)
    }

    /// Full state for storage.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.base.export_snapshot()
    }

    /// The whole history as updates, for the first push.
    pub fn export_all_updates(&self) -> Result<Vec<u8>> {
        self.base.export_all_updates()
    }

    /// Changes a replica at `version` lacks.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        self.base.export_updates_since(version)
    }

    /// Applies a snapshot or updates.
    pub fn import(&self, bytes: &[u8]) -> Result<ImportOutcome> {
        self.base.import(bytes)
    }

    /// The version vector.
    pub fn version(&self) -> Vec<u8> {
        self.base.version()
    }

    /// Runs `edit` and returns the update holding exactly its changes.
    pub fn edit<R>(&self, edit: impl FnOnce(&Self) -> Result<R>) -> Result<(R, Vec<u8>)> {
        self.base.edit(|_| edit(self))
    }

    /// Saves a template and returns its id. `blocks` are cleaned with
    /// [`template_blocks`]. A row template needs `database`; a page template
    /// must not have it.
    pub fn create(
        &self,
        name: &str,
        kind: TemplateKind,
        database: Option<&str>,
        blocks: &[Block],
        defaults: &BTreeMap<String, CellValue>,
        now: i64,
    ) -> Result<String> {
        let name = check_name(name)?;
        match (kind, database) {
            (TemplateKind::Row, None) => {
                return Err(Error::Invalid(
                    "a row template belongs to a database".into(),
                ));
            }
            (TemplateKind::Page, Some(_)) => {
                return Err(Error::Invalid("a page template has no database".into()));
            }
            _ => {}
        }
        let blocks = template_blocks(blocks);
        if count_blocks(&blocks) > MAX_TEMPLATE_BLOCKS {
            return Err(Error::Invalid("the template is too large".into()));
        }
        let id = fresh_id();
        let map = self
            .base
            .map(TEMPLATES)
            .insert_container(&id, LoroMap::new())?;
        map.insert(KEY_NAME, name.as_str())?;
        map.insert(KEY_KIND, serde_text(&kind)?.as_str())?;
        if let Some(database) = database {
            map.insert(KEY_DATABASE, database)?;
        }
        map.insert(KEY_BLOCKS, serde_text(&blocks)?.as_str())?;
        map.insert(KEY_DEFAULTS, serde_text(defaults)?.as_str())?;
        map.insert(KEY_CREATED_AT, now)?;
        self.base.doc.commit();
        Ok(id)
    }

    fn map_of(&self, id: &str) -> Result<LoroMap> {
        self.base
            .map(TEMPLATES)
            .get(id)
            .and_then(|v| v.into_container().ok())
            .and_then(|c| c.into_map().ok())
            .ok_or_else(|| Error::Missing(format!("template {id}")))
    }

    fn read(&self, id: &str, map: &LoroMap) -> Option<Template> {
        let kind: TemplateKind = serde_json::from_str(&text(map, KEY_KIND)?).ok()?;
        Some(Template {
            id: id.to_owned(),
            name: text(map, KEY_NAME).unwrap_or_default(),
            kind,
            database: text(map, KEY_DATABASE),
            defaults: text(map, KEY_DEFAULTS)
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            created_at: match map.get(KEY_CREATED_AT) {
                Some(ValueOrContainer::Value(LoroValue::I64(n))) => n,
                _ => 0,
            },
            blocks: text(map, KEY_BLOCKS)
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
        })
    }

    /// One template with its content.
    pub fn get(&self, id: &str) -> Result<Template> {
        let map = self.map_of(id)?;
        self.read(id, &map)
            .ok_or_else(|| Error::Missing(format!("template {id}")))
    }

    /// Every template, oldest first. `database` of `Some(id)` lists that
    /// database's row templates only; `None` lists page templates.
    pub fn list(&self, database: Option<&str>) -> Vec<Template> {
        let all = self.base.map(TEMPLATES);
        let mut out: Vec<Template> = all
            .keys()
            .filter_map(|id| {
                let map = all.get(&id)?.into_container().ok()?.into_map().ok()?;
                self.read(&id, &map)
            })
            .filter(|t| t.database.as_deref() == database)
            .collect();
        out.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        out
    }

    /// Renames a template.
    pub fn rename(&self, id: &str, name: &str) -> Result<()> {
        let name = check_name(name)?;
        self.map_of(id)?.insert(KEY_NAME, name.as_str())?;
        self.base.doc.commit();
        Ok(())
    }

    /// Replaces the default values of a row template.
    pub fn set_defaults(&self, id: &str, defaults: &BTreeMap<String, CellValue>) -> Result<()> {
        let map = self.map_of(id)?;
        if text(&map, KEY_KIND).as_deref() != Some("\"row\"") {
            return Err(Error::Invalid(
                "only a row template has default values".into(),
            ));
        }
        map.insert(KEY_DEFAULTS, serde_text(defaults)?.as_str())?;
        self.base.doc.commit();
        Ok(())
    }

    /// Deletes a template.
    pub fn delete(&self, id: &str) -> Result<()> {
        self.map_of(id)?;
        self.base.map(TEMPLATES).delete(id)?;
        self.base.doc.commit();
        Ok(())
    }
}

fn serde_text<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Invalid(e.to_string()))
}
