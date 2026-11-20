//! The vault's synchronised state: one Loro document of sealed records.
//!
//! # Layout
//!
//! * `items`: item id -> one binary value (see the record module). The whole
//!   record is a single map value, so concurrent edits of the same item
//!   resolve last-writer-wins to exactly one complete sealed record; two
//!   ciphertexts are never combined field by field.
//! * `tombs`: item id -> the update counter at deletion. A tombstone wins over
//!   any edit of the same id, concurrent or later, so a deleted item cannot
//!   come back through a sync with a replica that still had it. Ids are random
//!   and never reused, so nothing legitimate is hidden by a tombstone.
//!
//! Without the vault key only ids, sealed bytes, update counters and which ids
//! are deleted can be read. The document is key-agnostic: importing and
//! exporting never need the vault to be unlocked.

use loro::{ExportMode, LoroDoc, LoroMap, LoroValue, ValueOrContainer, VersionVector};

use crate::error::{Result, VaultError};
use crate::item::ItemId;
use crate::record::SealedRecord;

const ITEMS: &str = "items";
const TOMBS: &str = "tombs";

/// Result of [`VaultDoc::import`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOutcome {
    /// True when the import depends on history this replica does not have yet;
    /// the missing part must arrive before everything takes effect.
    pub has_missing_dependencies: bool,
}

/// The synchronised vault document. Holds only sealed data.
pub struct VaultDoc {
    doc: LoroDoc,
}

impl Default for VaultDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultDoc {
    /// An empty document with a random replica id.
    pub fn new() -> Self {
        Self {
            doc: LoroDoc::new(),
        }
    }

    /// Loads a document from bytes produced by [`Self::export_snapshot`].
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        Ok(Self { doc })
    }

    /// Exports the full state including history.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// Exports what a replica at `version` (bytes from [`Self::version`]) lacks.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let vv = VersionVector::decode(version)?;
        self.doc.commit();
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    /// Applies a snapshot or updates from another replica.
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

    /// Ids of all live (not deleted) records, in no particular order. Visible
    /// without the key.
    pub fn item_ids(&self) -> Vec<ItemId> {
        self.live_entries().into_iter().map(|(id, _)| id).collect()
    }

    /// Ids that have been deleted. Visible without the key.
    pub fn deleted_ids(&self) -> Vec<ItemId> {
        let mut out = Vec::new();
        self.tombs().for_each(|key, _| {
            if let Ok(id) = key.parse() {
                out.push(id);
            }
        });
        out
    }

    fn items(&self) -> LoroMap {
        self.doc.get_map(ITEMS)
    }

    fn tombs(&self) -> LoroMap {
        self.doc.get_map(TOMBS)
    }

    fn is_deleted(&self, id: &ItemId) -> bool {
        self.tombs().get(&id.to_string()).is_some()
    }

    /// Stores `record` for `id`. Refuses ids that are already deleted.
    pub(crate) fn put(&self, id: &ItemId, record: &SealedRecord) -> Result<()> {
        if self.is_deleted(id) {
            return Err(VaultError::NotFound(*id));
        }
        self.items().insert(&id.to_string(), record.encode())?;
        self.doc.commit();
        Ok(())
    }

    /// Replaces the record of `id` with a tombstone.
    pub(crate) fn tombstone(&self, id: &ItemId, counter: u64) -> Result<()> {
        let key = id.to_string();
        let counter = i64::try_from(counter).unwrap_or(i64::MAX);
        self.tombs().insert(&key, LoroValue::I64(counter))?;
        self.items().delete(&key)?;
        self.doc.commit();
        Ok(())
    }

    /// The current update counter of `id` (0 when absent or unreadable).
    pub(crate) fn counter_of(&self, id: &ItemId) -> u64 {
        let live = self.get(id).ok().flatten().map_or(0, |r| r.counter);
        let tomb = self.tombs().get(&id.to_string()).map_or(0, |v| match v {
            ValueOrContainer::Value(LoroValue::I64(n)) => u64::try_from(n).unwrap_or(0),
            _ => 0,
        });
        live.max(tomb)
    }

    /// The live record of `id`, `None` when absent or deleted.
    pub(crate) fn get(&self, id: &ItemId) -> Result<Option<SealedRecord>> {
        if self.is_deleted(id) {
            return Ok(None);
        }
        match self.items().get(&id.to_string()) {
            None => Ok(None),
            Some(value) => decode_value(value).map(Some),
        }
    }

    /// Every live record; records that cannot be decoded come back as errors.
    pub(crate) fn live_entries(&self) -> Vec<(ItemId, Result<SealedRecord>)> {
        let mut out = Vec::new();
        self.items().for_each(|key, value| {
            if let Ok(id) = key.parse::<ItemId>()
                && !self.is_deleted(&id)
            {
                out.push((id, decode_value(value)));
            }
        });
        out
    }
}

fn decode_value(value: ValueOrContainer) -> Result<SealedRecord> {
    match value {
        ValueOrContainer::Value(LoroValue::Binary(bytes)) => SealedRecord::decode(&bytes),
        _ => Err(VaultError::Corrupt),
    }
}
