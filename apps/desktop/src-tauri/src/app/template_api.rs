//! Templates for the UI: saved page content and row templates.

use std::collections::BTreeMap;

use ne_core::database::CellValue;
use ne_core::template::Template;
use ne_core::tree::Position;

use super::{AppCore, AppResult, CoreEvent, node_id};

impl AppCore {
    pub async fn template_list(&self, database_id: Option<&str>) -> AppResult<Vec<Template>> {
        let database = database_id.map(node_id).transpose()?;
        self.store(|ws| ws.templates(database))
    }

    pub async fn template_create_from_page(&self, page_id: &str, name: &str) -> AppResult<String> {
        let page = node_id(page_id)?;
        let id = self.store(|ws| ws.template_from_page(page, name))?;
        self.after_local_change();
        Ok(id)
    }

    pub async fn template_create_for_database(
        &self,
        database_id: &str,
        name: &str,
        from_row_id: Option<&str>,
    ) -> AppResult<String> {
        let database = node_id(database_id)?;
        let row = from_row_id.map(node_id).transpose()?;
        let id = self.store(|ws| ws.template_for_database(database, name, row, None))?;
        self.after_local_change();
        Ok(id)
    }

    pub async fn template_rename(&self, id: &str, name: &str) -> AppResult<()> {
        self.store(|ws| ws.template_rename(id, name))?;
        self.after_local_change();
        Ok(())
    }

    pub async fn template_set_defaults(
        &self,
        id: &str,
        defaults: &BTreeMap<String, CellValue>,
    ) -> AppResult<()> {
        self.store(|ws| ws.template_set_defaults(id, defaults))?;
        self.after_local_change();
        Ok(())
    }

    pub async fn template_delete(&self, id: &str) -> AppResult<()> {
        self.store(|ws| ws.template_delete(id))?;
        self.after_local_change();
        Ok(())
    }

    pub async fn template_instantiate_page(
        &self,
        id: &str,
        parent_id: Option<&str>,
        index: Option<usize>,
        title: Option<&str>,
    ) -> AppResult<String> {
        let parent = parent_id.map(node_id).transpose()?;
        let page = self.store(|ws| {
            let place = match index {
                Some(i) => Position::Index(i.min(ws.children(parent)?.len())),
                None => Position::End,
            };
            ws.template_new_page(id, parent, place, title)
        })?;
        self.emit(CoreEvent::TreeChanged {
            node_ids: vec![page.to_string()],
        });
        self.after_local_change();
        self.schedule_maintenance();
        Ok(page.to_string())
    }

    pub async fn template_instantiate_row(
        &self,
        id: &str,
        title: Option<&str>,
    ) -> AppResult<String> {
        let row = self.store(|ws| ws.template_new_row(id, title))?;
        let parent = self.store(|ws| Ok(ws.node(row)?.and_then(|n| n.parent)))?;
        if let Some(database) = parent {
            self.emit(CoreEvent::DatabaseChanged {
                database_id: database.to_string(),
                row_ids: vec![row.to_string()],
            });
        }
        self.after_local_change();
        self.schedule_maintenance();
        Ok(row.to_string())
    }
}
