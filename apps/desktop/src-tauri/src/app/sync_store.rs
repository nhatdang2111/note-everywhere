//! Adapts the workspace store to the sync engine's [`SyncStore`] trait.
//!
//! Wire document ids must satisfy the server's id rules (letters, digits and
//! `. _ : -`), which a node id (`<counter>@<peer>`) does not, so the `@`
//! travels as `:`.
//!
//! Crash safety: the engine asks for updates to be applied and the cursor to
//! move as one step. The store has no single transaction for both, so updates
//! are applied first and the cursor moved after. A crash in between replays
//! the page on restart, which is harmless: importing an update the document
//! already contains changes nothing and stores nothing.

use std::collections::BTreeSet;

use ne_core::NodeId;
use ne_store::DocRef;
use ne_sync::{IncomingBatch, OutboxItem, StoreError, StoreResult, SyncStore};
use ne_sync_protocol::ClientId;
use uuid::Uuid;

use super::{CoreEvent, EventSink, OpenPages, SharedVault, SharedWorkspace, lock};

pub const TREE_DOC: &str = "tree";
pub const VAULT_DOC: &str = "vault";
pub const BLOB_INDEX_DOC: &str = "attachments";

pub fn wire_id(doc: &DocRef) -> String {
    match doc {
        DocRef::Tree => TREE_DOC.to_owned(),
        DocRef::Page(id) => id.to_string().replace('@', ":"),
        DocRef::Vault(name) => name.clone(),
    }
}

/// The document a wire id names, or `None` for an id this version does not know.
pub fn doc_ref(wire: &str) -> Option<DocRef> {
    match wire {
        TREE_DOC => Some(DocRef::Tree),
        VAULT_DOC | BLOB_INDEX_DOC => Some(DocRef::Vault(wire.to_owned())),
        _ => wire
            .replace(':', "@")
            .parse::<NodeId>()
            .ok()
            .map(DocRef::Page),
    }
}

pub fn encode_cursor(cursor: u64) -> [u8; 8] {
    cursor.to_be_bytes()
}

fn decode_cursor(bytes: Option<Vec<u8>>) -> StoreResult<u64> {
    match bytes {
        None => Ok(0),
        Some(bytes) => <[u8; 8]>::try_from(bytes.as_slice())
            .map(u64::from_be_bytes)
            .map_err(|_| StoreError("the stored sync position is damaged".into())),
    }
}

fn storage<E: std::fmt::Display>(err: E) -> StoreError {
    StoreError(err.to_string())
}

pub struct WorkspaceStore {
    pub ws: SharedWorkspace,
    pub vault: SharedVault,
    pub open_pages: OpenPages,
    pub events: std::sync::Arc<dyn EventSink>,
}

impl WorkspaceStore {
    fn with<R>(
        &self,
        f: impl FnOnce(&mut ne_store::Workspace) -> ne_store::Result<R>,
    ) -> StoreResult<R> {
        let mut guard = lock(&self.ws);
        let ws = guard
            .as_mut()
            .ok_or_else(|| StoreError("the workspace is closed".into()))?;
        f(ws).map_err(storage)
    }
}

impl SyncStore for WorkspaceStore {
    fn enqueue(&self, item: OutboxItem) -> StoreResult<()> {
        let doc = doc_ref(&item.doc_id).ok_or_else(|| StoreError("unknown document".into()))?;
        self.with(|ws| ws.outbox_enqueue(&doc, &item.update).map(drop))
    }

    fn outbox_next(&self, max_items: usize, max_bytes: usize) -> StoreResult<Vec<OutboxItem>> {
        let queued = self.with(|ws| ws.outbox_peek(max_items))?;
        let mut taken = Vec::new();
        let mut bytes = 0usize;
        for item in queued {
            if !taken.is_empty() && bytes + item.bytes.len() > max_bytes {
                break;
            }
            let id = Uuid::parse_str(&item.client_id).map_err(storage)?;
            bytes += item.bytes.len();
            taken.push(OutboxItem {
                client_id: ClientId(*id.as_bytes()),
                doc_id: wire_id(&item.doc),
                update: item.bytes,
            });
        }
        Ok(taken)
    }

    fn outbox_len(&self) -> StoreResult<usize> {
        self.with(|ws| ws.outbox_len())
    }

    fn outbox_remove(&self, ids: &[ClientId]) -> StoreResult<()> {
        let ids: Vec<String> = ids
            .iter()
            .map(|id| Uuid::from_bytes(id.0).to_string())
            .collect();
        self.with(|ws| ws.outbox_acknowledge(&ids).map(drop))
    }

    fn cursor(&self) -> StoreResult<u64> {
        decode_cursor(self.with(|ws| ws.sync_cursor())?)
    }

    fn apply_incoming(&self, batch: IncomingBatch) -> StoreResult<()> {
        let mut tree_nodes: BTreeSet<String> = BTreeSet::new();
        let mut pages: Vec<(NodeId, Vec<u8>)> = Vec::new();
        let mut vault_updates: Vec<Vec<u8>> = Vec::new();
        self.with(|ws| {
            for update in &batch.updates {
                let Some(doc) = doc_ref(&update.doc_id) else {
                    continue;
                };
                let change = ws.apply_remote_update(&doc, &update.plaintext)?;
                tree_nodes.extend(
                    change
                        .nodes
                        .iter()
                        .chain(&change.removed)
                        .map(ToString::to_string),
                );
                match (&doc, change.page) {
                    (DocRef::Page(id), Some(_)) => pages.push((*id, update.plaintext.clone())),
                    (DocRef::Vault(name), _) if name == VAULT_DOC => {
                        vault_updates.push(update.plaintext.clone());
                    }
                    _ => {}
                }
            }
            let stored = ws.sync_cursor()?;
            let current = decode_cursor(stored).map_err(|e| ne_store::Error::Corrupt(e.0))?;
            if batch.cursor > current {
                ws.set_sync_cursor(&encode_cursor(batch.cursor))?;
            }
            Ok(())
        })?;
        // The workspace lock is released: the vault and the UI are told next.
        if !vault_updates.is_empty()
            && let Some(vault) = lock(&self.vault).as_mut()
        {
            for bytes in &vault_updates {
                vault.import(bytes).map_err(storage)?;
            }
        }
        if !tree_nodes.is_empty() {
            self.events.emit(CoreEvent::TreeChanged {
                node_ids: tree_nodes.into_iter().collect(),
            });
        }
        let open = lock(&self.open_pages).clone();
        for (id, update) in pages {
            if open.contains(&id) {
                self.events.emit(CoreEvent::PageRemoteUpdate {
                    page_id: id.to_string(),
                    update,
                });
            }
        }
        Ok(())
    }
}
