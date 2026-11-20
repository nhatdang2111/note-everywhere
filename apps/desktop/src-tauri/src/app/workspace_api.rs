//! Tree, pages, search and Markdown import and export.

use std::collections::HashMap;

use ne_core::NodeId;
use ne_core::tree::{NodeKind as CoreKind, Position};
use ne_store::NodeRow;

use super::types::{
    Backlink, CreateNodeArgs, MoveNodeArgs, NodeKind, OpenPage, SearchHit, TrashEntry, TreeNode,
};
use super::{AppCore, AppError, AppResult, CoreEvent, lock, node_id};

fn kind_of(kind: CoreKind) -> NodeKind {
    match kind {
        CoreKind::Folder => NodeKind::Folder,
        CoreKind::Page => NodeKind::Page,
    }
}

fn tree_node(row: &NodeRow) -> TreeNode {
    TreeNode {
        id: row.id.to_string(),
        parent_id: row.parent.map(|p| p.to_string()),
        kind: kind_of(row.kind),
        title: row.title.clone(),
        icon: row.icon.clone(),
        favorite: row.favorite,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Visible rows in depth-first order, siblings in display order.
fn depth_first(rows: &[NodeRow]) -> Vec<TreeNode> {
    let mut by_parent: HashMap<Option<NodeId>, Vec<&NodeRow>> = HashMap::new();
    for row in rows {
        by_parent.entry(row.parent).or_default().push(row);
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut stack: Vec<&NodeRow> = by_parent
        .get(&None)
        .map(|top| top.iter().rev().copied().collect())
        .unwrap_or_default();
    while let Some(row) = stack.pop() {
        out.push(tree_node(row));
        if let Some(children) = by_parent.get(&Some(row.id)) {
            stack.extend(children.iter().rev().copied());
        }
    }
    out
}

fn position(index: Option<usize>) -> Position {
    index.map_or(Position::End, Position::Index)
}

fn out_of_range(err: &ne_store::Error) -> bool {
    matches!(
        err,
        ne_store::Error::Core(ne_core::Error::PositionOutOfRange { .. })
    )
}

fn snippet_of(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut out: String = line.trim().chars().take(140).collect();
    if line.trim().chars().count() > 140 {
        out.push('…');
    }
    out
}

/// Converts a byte range of `text` to UTF-16 code unit offsets, which is how
/// the webview indexes strings.
fn utf16_range(text: &str, range: &std::ops::Range<usize>) -> (usize, usize) {
    let units = |end: usize| text.get(..end).map_or(0, |s| s.encode_utf16().count());
    (units(range.start), units(range.end))
}

impl AppCore {
    fn tree_changed(&self, ids: Vec<String>) {
        self.emit(CoreEvent::TreeChanged { node_ids: ids });
        self.after_local_change();
    }

    pub async fn tree_snapshot(&self) -> AppResult<Vec<TreeNode>> {
        self.store(|ws| ws.sidebar()).map(|rows| depth_first(&rows))
    }

    pub async fn tree_create(&self, args: CreateNodeArgs) -> AppResult<String> {
        let parent = args.parent_id.as_deref().map(node_id).transpose()?;
        let title = args.title.unwrap_or_default();
        let id = self.store(|ws| {
            let place = match args.index {
                Some(i) => Position::Index(i.min(ws.children(parent)?.len())),
                None => Position::End,
            };
            match args.kind {
                NodeKind::Folder => ws.create_folder(parent, place, &title),
                NodeKind::Page => ws.create_page(parent, place, &title),
            }
        })?;
        self.tree_changed(vec![id.to_string()]);
        Ok(id.to_string())
    }

    pub async fn tree_rename(&self, id: &str, title: &str) -> AppResult<()> {
        let node = node_id(id)?;
        self.store(|ws| ws.rename(node, title))?;
        self.tree_changed(vec![id.to_owned()]);
        Ok(())
    }

    pub async fn tree_set_icon(&self, id: &str, icon: Option<&str>) -> AppResult<()> {
        let node = node_id(id)?;
        self.store(|ws| ws.set_icon(node, icon))?;
        self.tree_changed(vec![id.to_owned()]);
        Ok(())
    }

    pub async fn tree_set_favorite(&self, id: &str, favorite: bool) -> AppResult<()> {
        let node = node_id(id)?;
        self.store(|ws| ws.set_favorite(node, favorite))?;
        self.tree_changed(vec![id.to_owned()]);
        Ok(())
    }

    pub async fn tree_move(&self, args: MoveNodeArgs) -> AppResult<()> {
        let node = node_id(&args.id)?;
        let parent = args.parent_id.as_deref().map(node_id).transpose()?;
        self.store(|ws| {
            match ws.move_node(node, parent, position(args.index)) {
                // A stale index from the UI means "at the end".
                Err(err) if out_of_range(&err) => ws.move_node(node, parent, Position::End),
                other => other,
            }
        })?;
        self.tree_changed(vec![args.id]);
        Ok(())
    }

    pub async fn tree_trash(&self, id: &str) -> AppResult<()> {
        let node = node_id(id)?;
        self.store(|ws| ws.trash_node(node))?;
        lock(&self.inner.open_pages).remove(&node);
        self.tree_changed(vec![id.to_owned()]);
        Ok(())
    }

    pub async fn tree_restore(&self, id: &str) -> AppResult<()> {
        let node = node_id(id)?;
        self.store(|ws| ws.restore(node))?;
        self.tree_changed(vec![id.to_owned()]);
        Ok(())
    }

    pub async fn tree_delete_permanently(&self, id: &str) -> AppResult<()> {
        let node = node_id(id)?;
        let removed = self.store(|ws| ws.delete_permanently(node))?;
        self.forget_pages(&removed);
        self.tree_changed(removed.iter().map(ToString::to_string).collect());
        Ok(())
    }

    pub async fn tree_list_trash(&self) -> AppResult<Vec<TrashEntry>> {
        self.store(|ws| {
            let mut entries = Vec::new();
            for info in ws.trash()? {
                if let Some(row) = ws.node(info.id)? {
                    entries.push(TrashEntry {
                        node: tree_node(&row),
                        trashed_at: row.trashed_at.unwrap_or(info.updated_at),
                    });
                }
            }
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.trashed_at));
            Ok(entries)
        })
    }

    pub async fn tree_empty_trash(&self) -> AppResult<()> {
        let removed = self.store(|ws| ws.empty_trash())?;
        self.forget_pages(&removed);
        self.tree_changed(removed.iter().map(ToString::to_string).collect());
        Ok(())
    }

    fn forget_pages(&self, removed: &[NodeId]) {
        let mut open = lock(&self.inner.open_pages);
        for id in removed {
            open.remove(id);
        }
    }

    // ---- pages ----

    pub async fn page_open(&self, id: &str) -> AppResult<OpenPage> {
        let page = node_id(id)?;
        let snapshot = self.store(|ws| ws.page_snapshot(page))?;
        lock(&self.inner.open_pages).insert(page);
        Ok(OpenPage { snapshot })
    }

    pub async fn page_apply_update(&self, id: &str, update: &[u8]) -> AppResult<()> {
        let page = node_id(id)?;
        if update.is_empty() {
            return Err(AppError::invalid("The update is empty."));
        }
        self.store(|ws| ws.apply_local_update(page, update))?;
        self.after_local_change();
        self.schedule_maintenance();
        Ok(())
    }

    pub async fn page_close(&self, id: &str) -> AppResult<()> {
        let page = node_id(id)?;
        lock(&self.inner.open_pages).remove(&page);
        Ok(())
    }

    pub async fn page_backlinks(&self, id: &str) -> AppResult<Vec<Backlink>> {
        let page = node_id(id)?;
        self.store(|ws| {
            let mut out = Vec::new();
            for link in ws.backlinks(page)? {
                let text = ws.open_page(link.source)?.plain_text().unwrap_or_default();
                out.push(Backlink {
                    page_id: link.source.to_string(),
                    title: link.title,
                    snippet: snippet_of(&text),
                });
            }
            Ok(out)
        })
    }

    // ---- search ----

    pub async fn search_query(
        &self,
        text: &str,
        limit: Option<usize>,
    ) -> AppResult<Vec<SearchHit>> {
        let limit = limit.unwrap_or(20).clamp(1, 100);
        self.store(|ws| {
            let mut hits = Vec::new();
            for hit in ws.search(text, limit)? {
                if hit.kind != CoreKind::Page {
                    continue;
                }
                let breadcrumb = ws
                    .tree()
                    .ancestors(hit.id)?
                    .into_iter()
                    .map(|a| a.title)
                    .collect();
                let snippet = hit.snippet.text;
                let highlights = hit
                    .snippet
                    .highlights
                    .iter()
                    .map(|r| utf16_range(&snippet, r))
                    .collect();
                hits.push(SearchHit {
                    page_id: hit.id.to_string(),
                    title: hit.title,
                    breadcrumb,
                    snippet,
                    highlights,
                });
            }
            Ok(hits)
        })
    }

    // ---- Markdown ----

    /// Exports into a new folder inside the folder the person picks, so an
    /// existing file is never overwritten.
    pub async fn export_markdown(&self, id: Option<&str>) -> AppResult<Option<usize>> {
        let root = id.map(node_id).transpose()?;
        let dialogs = self.inner.deps.dialogs.clone();
        let picked = self
            .blocking(move |_| Ok(dialogs.pick_folder("Export Markdown to…")))
            .await?;
        let Some(parent) = picked else {
            return Ok(None);
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let dest = parent.join(format!("note-everywhere-export-{stamp}"));
        let report = self
            .blocking(move |core| core.store(|ws| ws.export_markdown(root, &dest)))
            .await?;
        Ok(Some(report.pages))
    }

    pub async fn import_markdown(&self, parent_id: Option<&str>) -> AppResult<Option<usize>> {
        let parent = parent_id.map(node_id).transpose()?;
        let dialogs = self.inner.deps.dialogs.clone();
        let picked = self
            .blocking(move |_| Ok(dialogs.pick_folder("Import a folder of Markdown files…")))
            .await?;
        let Some(dir) = picked else {
            return Ok(None);
        };
        let report = self
            .blocking(move |core| core.store(|ws| ws.import_markdown_dir(parent, &dir)))
            .await?;
        self.tree_changed(parent_id.map(str::to_owned).into_iter().collect());
        self.schedule_maintenance();
        Ok(Some(report.pages))
    }
}
