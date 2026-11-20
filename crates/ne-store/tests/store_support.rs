//! Shared helpers for the `store_*` integration tests. Included with
//! `mod store_support;` from each test file.
#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use ne_core::NodeId;
use ne_core::page::{Block, PageDoc};
use ne_core::tree::Position;
use ne_store::{DocRef, Durability, StoreConfig, Workspace};

/// A clock the test can read and move.
#[derive(Clone)]
pub struct TestClock(pub Arc<AtomicI64>);

impl TestClock {
    pub fn new(start: i64) -> Self {
        Self(Arc::new(AtomicI64::new(start)))
    }

    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

/// Every reading moves time one millisecond forward.
impl ne_core::tree::Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

pub fn config_with(clock: &TestClock) -> StoreConfig {
    StoreConfig {
        durability: Durability::Normal,
        clock: Arc::new(clock.clone()),
        ..StoreConfig::default()
    }
}

pub fn config() -> StoreConfig {
    config_with(&TestClock::new(1_000_000))
}

pub fn new_workspace() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = Workspace::create(dir.path(), config()).expect("create workspace");
    (dir, ws)
}

pub fn reopen(dir: &Path) -> Workspace {
    Workspace::open(dir, config()).expect("open workspace")
}

/// Creates a top-level page and fills it with `blocks`.
pub fn page_with(ws: &mut Workspace, title: &str, blocks: &[Block]) -> NodeId {
    page_in(ws, None, title, blocks)
}

pub fn page_in(
    ws: &mut Workspace,
    parent: Option<NodeId>,
    title: &str,
    blocks: &[Block],
) -> NodeId {
    let id = ws
        .create_page(parent, Position::End, title)
        .expect("create page");
    fill_page(ws, id, blocks);
    id
}

/// Replaces the page body (the skeleton paragraph) with `blocks`.
pub fn fill_page(ws: &mut Workspace, id: NodeId, blocks: &[Block]) {
    if blocks.is_empty() {
        return;
    }
    ws.edit_page(id, |doc| {
        let existing = doc.blocks()?.len();
        doc.append_blocks(&[], blocks)?;
        for index in (0..existing).rev() {
            doc.delete_block(&[index])?;
        }
        Ok(())
    })
    .expect("edit page");
}

pub fn paragraphs(texts: &[&str]) -> Vec<Block> {
    texts.iter().map(|t| Block::paragraph(t)).collect()
}

pub fn page_text(ws: &mut Workspace, id: NodeId) -> String {
    ws.open_page(id)
        .expect("open page")
        .plain_text()
        .expect("plain text")
}

/// Hands every queued update of `from` to `to` as a remote update, in order,
/// and acknowledges them.
pub fn deliver(from: &mut Workspace, to: &mut Workspace) {
    loop {
        let batch = from.outbox_peek(50).expect("peek");
        if batch.is_empty() {
            break;
        }
        for item in &batch {
            to.apply_remote_update(&item.doc, &item.bytes)
                .expect("apply remote update");
        }
        let ids: Vec<String> = batch.iter().map(|i| i.client_id.clone()).collect();
        from.outbox_acknowledge(&ids).expect("acknowledge");
    }
}

/// A standalone replica of a page, as the editor would hold it.
pub fn editor_replica(ws: &mut Workspace, id: NodeId) -> PageDoc {
    let doc = PageDoc::from_snapshot(&ws.page_snapshot(id).expect("snapshot")).expect("load");
    doc.set_peer_id(0x00ed_1707).expect("peer id");
    doc
}

pub fn doc_ref(id: NodeId) -> DocRef {
    DocRef::Page(id)
}
