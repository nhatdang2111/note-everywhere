//! Templates on the [`Workspace`]: the library document, and making pages and
//! rows from templates. See `ne_core::template` for what a template keeps.

use std::collections::BTreeMap;

use ne_core::NodeId;
use ne_core::database::{CellValue, PropertyType, validate_cell};
use ne_core::template::{LibraryDoc, Template, TemplateKind};
use ne_core::tree::Position;

use crate::error::{Error, Result};
use crate::workspace::Workspace;

impl Workspace {
    fn edit_library<R>(
        &mut self,
        edit: impl FnOnce(&LibraryDoc) -> ne_core::Result<R>,
    ) -> Result<R> {
        let doc = self.load_library()?;
        let before = doc.version();
        let (value, update) = match doc.edit(edit) {
            Ok(done) => done,
            Err(err) => {
                // Part of the edit may have been applied in memory.
                self.library = None;
                return Err(err.into());
            }
        };
        let doc = self.load_library()?;
        let version = doc.version();
        if version == before {
            return Ok(value);
        }
        let state = (doc.export_snapshot()?, version.clone());
        self.commit_singleton_update(NodeId::library(), &update, &version, true, || Ok(state))?;
        Ok(value)
    }

    /// The templates, oldest first, content included. `None` lists the page
    /// templates, `Some(database)` the row templates of that database.
    pub fn templates(&mut self, database: Option<NodeId>) -> Result<Vec<Template>> {
        if !self.has_document(NodeId::library())? {
            return Ok(Vec::new());
        }
        let key = database.map(|d| d.to_string());
        Ok(self.load_library()?.list(key.as_deref()))
    }

    /// Saves a page's content as a page template and returns its id.
    pub fn template_from_page(&mut self, page: NodeId, name: &str) -> Result<String> {
        let blocks = self.open_page(page)?.blocks()?;
        let now = self.now();
        self.edit_library(|lib| {
            lib.create(
                name,
                TemplateKind::Page,
                None,
                &blocks,
                &BTreeMap::new(),
                now,
            )
        })
    }

    /// Saves a row template for `database`. When `from_row` is given, the
    /// template takes that row's page content and its current values as the
    /// defaults (links to other rows are not kept, so rows made from the
    /// template do not all link to the same rows); otherwise it starts empty.
    pub fn template_for_database(
        &mut self,
        database: NodeId,
        name: &str,
        from_row: Option<NodeId>,
        defaults: Option<BTreeMap<String, CellValue>>,
    ) -> Result<String> {
        self.load_database(database)?;
        let (blocks, from_values) = match from_row {
            Some(row) => {
                let blocks = self.open_page(row)?.blocks()?;
                let doc = self.cached_database(database)?;
                let properties = doc.properties()?;
                let mut values = doc.row_values(&row.to_string(), &properties);
                values.retain(|_, v| !matches!(v, CellValue::Relation(_)));
                (blocks, values)
            }
            None => (Vec::new(), BTreeMap::new()),
        };
        let defaults = defaults.unwrap_or(from_values);
        let key = database.to_string();
        let now = self.now();
        self.edit_library(|lib| {
            lib.create(name, TemplateKind::Row, Some(&key), &blocks, &defaults, now)
        })
    }

    /// Renames a template.
    pub fn template_rename(&mut self, id: &str, name: &str) -> Result<()> {
        self.edit_library(|lib| lib.rename(id, name))
    }

    /// Replaces the default values of a row template.
    pub fn template_set_defaults(
        &mut self,
        id: &str,
        defaults: &BTreeMap<String, CellValue>,
    ) -> Result<()> {
        self.edit_library(|lib| lib.set_defaults(id, defaults))
    }

    /// Deletes a template.
    pub fn template_delete(&mut self, id: &str) -> Result<()> {
        self.edit_library(|lib| lib.delete(id))
    }

    fn template(&mut self, id: &str) -> Result<Template> {
        if !self.has_document(NodeId::library())? {
            return Err(Error::NotFound(format!("template {id}")));
        }
        Ok(self.load_library()?.get(id)?)
    }

    /// Makes a page from a page template: a new page with a deep copy of the
    /// template's content, fresh blocks sharing nothing with the template.
    pub fn template_new_page(
        &mut self,
        template: &str,
        parent: Option<NodeId>,
        position: Position,
        title: Option<&str>,
    ) -> Result<NodeId> {
        let template = self.template(template)?;
        if template.kind != TemplateKind::Page {
            return Err(Error::InvalidInput("that is a row template".into()));
        }
        let page = self.create_page(parent, position, title.unwrap_or(&template.name))?;
        self.edit_page(page, |doc| doc.replace_blocks(&template.blocks))?;
        Ok(page)
    }

    /// Makes a row from a row template: a page in the template's database
    /// with the template's content and default values. A default whose
    /// property is gone, or no longer fits its value, is skipped.
    pub fn template_new_row(&mut self, template: &str, title: Option<&str>) -> Result<NodeId> {
        let template = self.template(template)?;
        let Some(database) = template
            .database
            .as_deref()
            .filter(|_| template.kind == TemplateKind::Row)
            .and_then(|d| d.parse::<NodeId>().ok())
        else {
            return Err(Error::InvalidInput("that is not a row template".into()));
        };
        self.load_database(database)?;
        let properties = self.cached_database(database)?.properties()?;
        let values: BTreeMap<String, CellValue> = template
            .defaults
            .iter()
            .filter(|(id, value)| {
                properties.iter().any(|p| {
                    p.id == **id
                        && !p.is_backlink()
                        && p.ty != PropertyType::Relation
                        && validate_cell(p, value).is_ok()
                })
            })
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        let row = self.database_create_row(
            database,
            title.unwrap_or(&template.name),
            Position::End,
            &values,
        )?;
        self.edit_page(row, |doc| doc.replace_blocks(&template.blocks))?;
        Ok(row)
    }
}
