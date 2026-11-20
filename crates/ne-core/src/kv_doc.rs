//! A small CRDT document of root maps, the base of the template library and
//! the reminders: sync surface and edit tracking, no model of its own.

use loro::{ExportMode, LoroDoc, LoroMap, VersionVector};

use crate::error::Result;
use crate::roles::{Role, write_role};
pub use crate::tree::ImportOutcome;

/// A document with a role marker and named root maps.
pub(crate) struct KvDoc {
    pub(crate) doc: LoroDoc,
}

impl KvDoc {
    /// A new document marked with `role`.
    pub(crate) fn new(role: Role) -> Result<Self> {
        let doc = LoroDoc::new();
        write_role(&doc, role)?;
        doc.commit();
        Ok(Self { doc })
    }

    pub(crate) fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        Ok(Self { doc })
    }

    pub(crate) fn map(&self, name: &str) -> LoroMap {
        self.doc.get_map(name)
    }

    pub(crate) fn set_peer_id(&self, peer_id: u64) -> Result<()> {
        Ok(self.doc.set_peer_id(peer_id)?)
    }

    pub(crate) fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    pub(crate) fn export_all_updates(&self) -> Result<Vec<u8>> {
        self.doc.commit();
        Ok(self.doc.export(ExportMode::all_updates())?)
    }

    pub(crate) fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let vv = VersionVector::decode(version)?;
        self.doc.commit();
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    pub(crate) fn import(&self, bytes: &[u8]) -> Result<ImportOutcome> {
        let status = self.doc.import(bytes)?;
        Ok(ImportOutcome {
            has_missing_dependencies: status.pending.is_some(),
        })
    }

    pub(crate) fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    pub(crate) fn edit<R>(&self, edit: impl FnOnce(&Self) -> Result<R>) -> Result<(R, Vec<u8>)> {
        self.doc.commit();
        let before = self.doc.oplog_vv();
        let result = edit(self);
        self.doc.commit();
        let update = self.doc.export(ExportMode::updates(&before))?;
        result.map(|value| (value, update))
    }
}
