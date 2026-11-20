//! Which server object holds which attachment.
//!
//! The server stores an attachment under the hash of its *ciphertext*, which
//! other devices cannot derive from the content hash a page refers to. This
//! small synchronised document maps the content hash to the server address,
//! so any device can fetch an attachment it lacks. It is kept as an opaque
//! document in the store and travels through the normal encrypted sync.

use loro::{ExportMode, LoroDoc, LoroValue, ValueOrContainer};
use ne_store::{DocRef, Workspace};
use serde::{Deserialize, Serialize};

use super::sync_store::BLOB_INDEX_DOC;
use super::{AppError, AppResult};

const MAP: &str = "blobs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobEntry {
    /// Hash of the ciphertext: the object's address on the server.
    pub cipher_hash: String,
    pub size: u64,
    pub file_name: String,
}

fn crdt(_: impl std::fmt::Display) -> AppError {
    AppError::new("storage_error", "The attachment index could not be read.")
}

fn load(ws: &Workspace) -> AppResult<LoroDoc> {
    let doc = LoroDoc::new();
    if let Some(stored) = ws.vault_doc(BLOB_INDEX_DOC)? {
        if !stored.snapshot.is_empty() {
            doc.import(&stored.snapshot).map_err(crdt)?;
        }
        for update in &stored.updates {
            doc.import(update).map_err(crdt)?;
        }
    }
    Ok(doc)
}

fn entry_of(value: ValueOrContainer) -> Option<BlobEntry> {
    match value {
        ValueOrContainer::Value(LoroValue::Binary(bytes)) => serde_json::from_slice(&bytes).ok(),
        _ => None,
    }
}

/// The server address recorded for the attachment with this content hash.
pub fn lookup(ws: &Workspace, content_hash: &str) -> AppResult<Option<BlobEntry>> {
    let doc = load(ws)?;
    Ok(doc.get_map(MAP).get(content_hash).and_then(entry_of))
}

/// Content hashes that already have an entry.
pub fn known(ws: &Workspace) -> AppResult<Vec<String>> {
    let doc = load(ws)?;
    let mut out = Vec::new();
    doc.get_map(MAP).for_each(|key, _| out.push(key.to_owned()));
    Ok(out)
}

/// Records entries: queued for sync first, then stored locally. A crash in
/// between only means the upload step runs again and finds the object there.
pub fn record(ws: &mut Workspace, entries: &[(String, BlobEntry)]) -> AppResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let doc = load(ws)?;
    let before = doc.oplog_vv();
    let map = doc.get_map(MAP);
    for (hash, entry) in entries {
        let bytes = serde_json::to_vec(entry).map_err(crdt)?;
        map.insert(hash, bytes).map_err(crdt)?;
    }
    doc.commit();
    let update = doc.export(ExportMode::updates(&before)).map_err(crdt)?;
    let doc_ref = DocRef::Vault(BLOB_INDEX_DOC.to_owned());
    ws.outbox_enqueue(&doc_ref, &update)?;
    ws.apply_remote_update(&doc_ref, &update)?;
    Ok(())
}
