//! What the sync engine needs from local persistence.
//!
//! The storage crate implements [`SyncStore`] on top of its database; this
//! crate ships [`MemoryStore`] for tests. The contract is what makes sync
//! crash-safe, so every method states what must be durable.

use std::collections::VecDeque;
use std::sync::Mutex;

use ne_sync_protocol::ClientId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(pub String);

pub type StoreResult<T> = std::result::Result<T, StoreError>;

/// A local update waiting to be sent, kept as plaintext in local storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    pub client_id: ClientId,
    pub doc_id: String,
    pub update: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingKind {
    Update,
    /// Full document state; replaces everything the log held up to `upto`.
    Snapshot {
        upto: u64,
    },
}

/// A decrypted update from another device, ready to apply to a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingUpdate {
    pub seq: u64,
    pub doc_id: String,
    pub device_id: String,
    pub kind: IncomingKind,
    pub plaintext: Vec<u8>,
}

/// Everything one received page contributes, applied as a unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingBatch {
    pub updates: Vec<IncomingUpdate>,
    /// The log position after this batch: every row up to it is accounted for.
    pub cursor: u64,
}

pub trait SyncStore: Send + Sync + 'static {
    /// Durably appends an update to the outbox. Returning `Ok` is the promise
    /// that the update survives a crash and will be sent.
    fn enqueue(&self, item: OutboxItem) -> StoreResult<()>;

    /// The oldest items, at most `max_items` and about `max_bytes` of updates
    /// (always at least one item when the outbox is not empty). Does not
    /// remove them: until [`outbox_remove`](Self::outbox_remove) they are
    /// returned again, in the same order, with the same ids.
    fn outbox_next(&self, max_items: usize, max_bytes: usize) -> StoreResult<Vec<OutboxItem>>;

    fn outbox_len(&self) -> StoreResult<usize>;

    /// Removes acknowledged items. Unknown ids are ignored, so a repeated
    /// call after a crash is harmless.
    fn outbox_remove(&self, ids: &[ClientId]) -> StoreResult<()>;

    /// The persisted log position; 0 for a store that has never synced.
    fn cursor(&self) -> StoreResult<u64>;

    /// **Atomically** applies the updates to the documents and moves the
    /// cursor to `batch.cursor` (never backwards). Atomicity is what prevents
    /// both losing an update and applying one twice after a crash.
    fn apply_incoming(&self, batch: IncomingBatch) -> StoreResult<()>;
}

#[derive(Default)]
struct Inner {
    outbox: VecDeque<OutboxItem>,
    cursor: u64,
    applied: Vec<IncomingUpdate>,
}

/// Non-durable store for tests; it records what was applied.
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> StoreResult<std::sync::MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| StoreError("store lock poisoned".into()))
    }

    /// Every update applied so far, in application order.
    pub fn applied(&self) -> Vec<IncomingUpdate> {
        self.lock()
            .map(|inner| inner.applied.clone())
            .unwrap_or_default()
    }

    /// Items currently waiting to be sent.
    pub fn outbox(&self) -> Vec<OutboxItem> {
        self.lock()
            .map(|inner| inner.outbox.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl SyncStore for MemoryStore {
    fn enqueue(&self, item: OutboxItem) -> StoreResult<()> {
        self.lock()?.outbox.push_back(item);
        Ok(())
    }

    fn outbox_next(&self, max_items: usize, max_bytes: usize) -> StoreResult<Vec<OutboxItem>> {
        let inner = self.lock()?;
        let mut taken = Vec::new();
        let mut bytes = 0usize;
        for item in inner.outbox.iter().take(max_items) {
            if !taken.is_empty() && bytes + item.update.len() > max_bytes {
                break;
            }
            bytes += item.update.len();
            taken.push(item.clone());
        }
        Ok(taken)
    }

    fn outbox_len(&self) -> StoreResult<usize> {
        Ok(self.lock()?.outbox.len())
    }

    fn outbox_remove(&self, ids: &[ClientId]) -> StoreResult<()> {
        self.lock()?
            .outbox
            .retain(|item| !ids.contains(&item.client_id));
        Ok(())
    }

    fn cursor(&self) -> StoreResult<u64> {
        Ok(self.lock()?.cursor)
    }

    fn apply_incoming(&self, batch: IncomingBatch) -> StoreResult<()> {
        let mut inner = self.lock()?;
        inner.applied.extend(batch.updates);
        inner.cursor = inner.cursor.max(batch.cursor);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: u8, len: usize) -> OutboxItem {
        OutboxItem {
            client_id: ClientId([n; 16]),
            doc_id: "d".into(),
            update: vec![n; len],
        }
    }

    #[test]
    fn outbox_returns_the_same_items_until_removed() {
        let store = MemoryStore::new();
        for n in 1..=3 {
            store.enqueue(item(n, 10)).expect("enqueue");
        }
        let first = store.outbox_next(10, 1000).expect("next");
        assert_eq!(first, store.outbox_next(10, 1000).expect("next"));
        store
            .outbox_remove(&[first[0].client_id, ClientId([99; 16])])
            .expect("remove");
        assert_eq!(store.outbox_len().expect("len"), 2);
        assert_eq!(
            store.outbox_next(10, 1000).expect("next")[0].client_id,
            ClientId([2; 16])
        );
    }

    #[test]
    fn batches_respect_limits_but_always_make_progress() {
        let store = MemoryStore::new();
        store.enqueue(item(1, 100)).expect("enqueue");
        store.enqueue(item(2, 100)).expect("enqueue");
        assert_eq!(store.outbox_next(10, 50).expect("next").len(), 1);
        assert_eq!(store.outbox_next(10, 250).expect("next").len(), 2);
        assert_eq!(store.outbox_next(1, 250).expect("next").len(), 1);
    }

    #[test]
    fn the_cursor_never_moves_backwards() {
        let store = MemoryStore::new();
        store
            .apply_incoming(IncomingBatch {
                updates: vec![],
                cursor: 7,
            })
            .expect("apply");
        store
            .apply_incoming(IncomingBatch {
                updates: vec![],
                cursor: 3,
            })
            .expect("apply");
        assert_eq!(store.cursor().expect("cursor"), 7);
    }
}
