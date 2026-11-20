//! Where the vault's data lives: the sealed document in the workspace store
//! (queued for sync like any other document), the wrapped key record in the
//! settings table and, when signed in, on the server. The vault key itself is
//! never written anywhere.

use ne_crypto::account::VaultRecord;
use ne_store::{DocRef, Workspace};
use ne_vault::{LockPolicy, Vault, VaultDoc};

use super::sync_store::VAULT_DOC;
use super::{AppCore, AppError, AppResult, lock};

const RECORD_KEY: &str = "vault.record";
const PENDING_KEY: &str = "vault.record_pending";
/// Fold the stored update log into a snapshot once it holds this many updates.
const COMPACT_AFTER: usize = 100;

fn vault_err() -> AppError {
    AppError::new("vault_error", "The vault could not be read or written.")
}

/// The stored vault document: its snapshot plus the updates logged since.
pub fn load_doc(ws: &Workspace) -> AppResult<VaultDoc> {
    let Some(stored) = ws.vault_doc(VAULT_DOC)? else {
        return Ok(VaultDoc::new());
    };
    let doc = if stored.snapshot.is_empty() {
        VaultDoc::new()
    } else {
        VaultDoc::from_snapshot(&stored.snapshot).map_err(|_| vault_err())?
    };
    for update in &stored.updates {
        doc.import(update).map_err(|_| vault_err())?;
    }
    Ok(doc)
}

impl AppCore {
    pub(crate) fn lock_policy(&self) -> LockPolicy {
        let seconds = self.settings_now().vault_idle_lock_seconds;
        LockPolicy {
            idle_timeout: (seconds > 0).then(|| std::time::Duration::from_secs(seconds)),
            reauth_grace: self.inner.cfg.reauth_grace,
            clipboard_clear_after: self.inner.cfg.clipboard_clear_after,
            ..LockPolicy::default()
        }
    }

    pub(crate) fn apply_vault_policy(&self) {
        let policy = self.lock_policy();
        if let Some(vault) = lock(&self.inner.vault).as_mut() {
            vault.set_policy(policy);
        }
    }

    pub(crate) fn local_vault_record(&self) -> AppResult<Option<VaultRecord>> {
        if lock(&self.inner.ws).is_none() {
            return Ok(None);
        }
        let Some(json) = self.store(|ws| ws.setting(RECORD_KEY))? else {
            return Ok(None);
        };
        serde_json::from_str(&json)
            .map(Some)
            .map_err(|_| vault_err())
    }

    pub(crate) fn store_vault_record(&self, record: &VaultRecord) -> AppResult<()> {
        let json = serde_json::to_string(record).map_err(|_| vault_err())?;
        self.store(|ws| ws.set_setting(RECORD_KEY, &json))
    }

    /// Builds the locked vault from the stored record and document, if the
    /// device has a vault.
    pub(crate) fn load_vault(&self) -> AppResult<()> {
        let Some(record) = self.local_vault_record()? else {
            return Ok(());
        };
        let doc = self.with_ws(|ws| load_doc(ws))?;
        let vault = Vault::new(
            doc,
            record,
            self.inner.cfg.kdf_policy,
            self.lock_policy(),
            self.inner.cfg.vault_clock.clone(),
        );
        *lock(&self.inner.vault) = Some(vault);
        Ok(())
    }

    /// Takes over the account's vault on a device that has none yet. Its
    /// document arrives through sync.
    pub(crate) fn adopt_vault_record(&self, record: &VaultRecord) -> AppResult<()> {
        self.store_vault_record(record)?;
        self.load_vault()
    }

    pub(crate) fn mark_vault_record_pending(&self) -> AppResult<()> {
        self.store(|ws| ws.set_setting(PENDING_KEY, "1"))
    }

    /// Uploads the wrapped vault key if the last attempt could not.
    pub(crate) async fn push_pending_vault_record(&self) -> AppResult<()> {
        let Some(runtime) = self.runtime() else {
            return Ok(());
        };
        if self.store(|ws| ws.setting(PENDING_KEY))?.is_none() {
            return Ok(());
        }
        if let Some(record) = self.local_vault_record()? {
            runtime.client.put_vault_record(&record).await?;
        }
        self.store(|ws| ws.remove_setting(PENDING_KEY))
    }

    /// Stores and queues what changed in the vault since `before`. Called with
    /// the vault locked by the caller, so the order is always vault, then
    /// workspace.
    pub(crate) fn persist_vault_delta(&self, vault: &Vault, before: &[u8]) -> AppResult<()> {
        if vault.version() == before {
            return Ok(());
        }
        let update = vault.export_updates_since(before)?;
        self.with_ws(|ws| {
            let doc = DocRef::Vault(VAULT_DOC.to_owned());
            // Queued first: a crash before the local write only repeats an
            // update the next sync already carries.
            ws.outbox_enqueue(&doc, &update)?;
            ws.apply_remote_update(&doc, &update)?;
            if ws.update_log_size(&doc)?.0 >= COMPACT_AFTER {
                let full = load_doc(ws)?;
                let snapshot = full.export_snapshot().map_err(|_| vault_err())?;
                ws.put_vault_doc(VAULT_DOC, &snapshot, &full.version())?;
            }
            Ok(())
        })?;
        self.after_local_change();
        Ok(())
    }

    /// The stored vault document as `(snapshot, version)`, for server compaction.
    pub(crate) fn vault_snapshot(&self, ws: &Workspace) -> AppResult<(Vec<u8>, Vec<u8>)> {
        let doc = load_doc(ws)?;
        Ok((
            doc.export_snapshot().map_err(|_| vault_err())?,
            doc.version(),
        ))
    }

    /// Writes the wrapped key record to the server, remembering to retry if
    /// the server cannot be reached.
    pub(crate) async fn publish_vault_record(&self, record: &VaultRecord) -> AppResult<()> {
        self.store_vault_record(record)?;
        let Some(runtime) = self.runtime() else {
            return Ok(());
        };
        if runtime.client.put_vault_record(record).await.is_err() {
            self.mark_vault_record_pending()?;
        } else {
            self.store(|ws| ws.remove_setting(PENDING_KEY))?;
        }
        Ok(())
    }
}
