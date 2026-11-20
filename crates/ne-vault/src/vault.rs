//! The vault state machine: locked or unlocked, with the key in memory only
//! while unlocked.
//!
//! [`Vault`] owns a [`VaultDoc`] (sealed data, always available for sync) and,
//! only while unlocked, a `Session` holding the vault key and the decrypted
//! items. [`Vault::lock`] drops the session, which wipes the key and every
//! decrypted string. Item operations live in the `vault_items` module.
//!
//! Wiping is best effort: the allocator, swap files and copies the OS made of
//! the process's memory are outside the program's control (see the security
//! model).

use std::collections::HashMap;
use std::sync::Arc;

use ne_crypto::account::{
    VaultRecord, change_vault_password, unlock_vault, unlock_vault_with_recovery_key,
};
use ne_crypto::keys::VaultKey;
use ne_crypto::password::{KdfParams, KdfPolicy};
use ne_crypto::recovery::RecoveryKey;
use zeroize::Zeroizing;

use crate::clock::Clock;
use crate::doc::{ImportOutcome, VaultDoc};
use crate::error::{Result, VaultError};
use crate::item::{ItemContent, ItemId};
use crate::lock::{LockEvent, LockPolicy, LockState, SensitiveAction};
use crate::record::open_content;
use crate::search::search_text;

/// A decrypted item plus its search text.
pub(crate) struct Entry {
    pub content: ItemContent,
    pub search: Zeroizing<String>,
}

impl Entry {
    pub fn new(content: ItemContent) -> Self {
        let search = search_text(&content);
        Self { content, search }
    }
}

/// Everything that exists only while unlocked.
pub(crate) struct Session {
    pub key: VaultKey,
    pub items: HashMap<ItemId, Entry>,
    /// Live records that could not be opened (corrupt or from a newer format).
    pub unreadable: Vec<ItemId>,
    pub authenticated_at_ms: i64,
}

impl Session {
    fn load(key: VaultKey, doc: &VaultDoc, now_ms: i64) -> Self {
        let mut items = HashMap::new();
        let mut unreadable = Vec::new();
        for (id, record) in doc.live_entries() {
            match record.and_then(|r| open_content(&key, &id, &r)) {
                Ok(content) => {
                    items.insert(id, Entry::new(content));
                }
                Err(_) => unreadable.push(id),
            }
        }
        Self {
            key,
            items,
            unreadable,
            authenticated_at_ms: now_ms,
        }
    }
}

/// A secrets vault: sealed document plus lock state.
pub struct Vault {
    pub(crate) doc: VaultDoc,
    record: VaultRecord,
    kdf_policy: KdfPolicy,
    pub(crate) policy: LockPolicy,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) session: Option<Session>,
}

impl Vault {
    /// Opens a vault in the locked state. `record` is the server-stored
    /// [`VaultRecord`]; `kdf_policy` bounds the key-derivation settings read
    /// from it.
    pub fn new(
        doc: VaultDoc,
        record: VaultRecord,
        kdf_policy: KdfPolicy,
        policy: LockPolicy,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            doc,
            record,
            kdf_policy,
            policy,
            clock,
            session: None,
        }
    }

    /// Whether the vault is locked or unlocked.
    pub fn state(&self) -> LockState {
        if self.session.is_some() {
            LockState::Unlocked
        } else {
            LockState::Locked
        }
    }

    /// True while the key is in memory.
    pub fn is_unlocked(&self) -> bool {
        self.session.is_some()
    }

    /// The current auto-lock and re-authentication policy.
    pub fn policy(&self) -> &LockPolicy {
        &self.policy
    }

    /// Replaces the policy; takes effect for the next event or action.
    pub fn set_policy(&mut self, policy: LockPolicy) {
        self.policy = policy;
    }

    /// The record to upload to the server (changes after [`Self::change_password`]).
    pub fn record(&self) -> &VaultRecord {
        &self.record
    }

    /// Number of decrypted items held in memory; always 0 while locked.
    pub fn cached_item_count(&self) -> usize {
        self.session.as_ref().map_or(0, |s| s.items.len())
    }

    /// Ids of live records that could not be opened. Empty while locked.
    pub fn unreadable_items(&self) -> &[ItemId] {
        self.session.as_ref().map_or(&[], |s| &s.unreadable)
    }

    /// Unlocks with the second password. A failed attempt leaves the previous
    /// state untouched.
    pub fn unlock_with_password(&mut self, password: &[u8]) -> Result<()> {
        let key = unlock_vault(password, &self.record, &self.kdf_policy)?;
        self.start_session(key);
        Ok(())
    }

    /// Unlocks with the vault recovery key.
    pub fn unlock_with_recovery_key(&mut self, recovery_key: &RecoveryKey) -> Result<()> {
        let key = unlock_vault_with_recovery_key(recovery_key, &self.record)?;
        self.start_session(key);
        Ok(())
    }

    fn start_session(&mut self, key: VaultKey) {
        self.session = Some(Session::load(key, &self.doc, self.clock.now_ms()));
    }

    /// Locks: wipes the key and every decrypted item. Does nothing if already
    /// locked.
    pub fn lock(&mut self) {
        self.session = None;
    }

    /// Feeds a device event to the auto-lock policy. Returns whether the vault
    /// is locked afterwards.
    pub fn handle_event(&mut self, event: LockEvent) -> bool {
        if self.policy.should_lock(event) {
            self.lock();
        }
        !self.is_unlocked()
    }

    /// Proves the user still knows the second password, restarting the grace
    /// period for sensitive actions.
    pub fn reauthenticate_with_password(&mut self, password: &[u8]) -> Result<()> {
        let now = self.clock.now_ms();
        let session = self.session.as_mut().ok_or(VaultError::Locked)?;
        let key = unlock_vault(password, &self.record, &self.kdf_policy)?;
        if key != session.key {
            return Err(VaultError::Account(
                ne_crypto::account::AccountError::WrongSecret,
            ));
        }
        session.authenticated_at_ms = now;
        Ok(())
    }

    /// Fails with [`VaultError::Locked`] or [`VaultError::ReauthRequired`]
    /// unless `action` is allowed right now. Callers use it before any action
    /// that exposes plaintext outside this crate.
    pub fn ensure_fresh(&self, action: SensitiveAction) -> Result<()> {
        let session = self.session.as_ref().ok_or(VaultError::Locked)?;
        if self
            .policy
            .reauth_required(session.authenticated_at_ms, self.clock.now_ms())
        {
            return Err(VaultError::ReauthRequired(action));
        }
        Ok(())
    }

    /// Re-wraps the vault key under a new second password (fresh salt) and
    /// returns the record to upload. The vault recovery key keeps working and
    /// no item is re-encrypted. Counts as a fresh authentication.
    pub fn change_password(
        &mut self,
        new_password: &[u8],
        params: KdfParams,
    ) -> Result<VaultRecord> {
        let now = self.clock.now_ms();
        let session = self.session.as_mut().ok_or(VaultError::Locked)?;
        let record = change_vault_password(
            &session.key,
            &self.record,
            new_password,
            params,
            &self.kdf_policy,
        )?;
        self.record = record.clone();
        session.authenticated_at_ms = now;
        Ok(record)
    }

    /// Full sealed state for storage or first sync. Works while locked.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc.export_snapshot()
    }

    /// Sealed changes a replica at `version` lacks. Works while locked.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        self.doc.export_updates_since(version)
    }

    /// The current version vector as opaque bytes.
    pub fn version(&self) -> Vec<u8> {
        self.doc.version()
    }

    /// Applies sync bytes from another replica. Works while locked; while
    /// unlocked the decrypted cache is rebuilt so it reflects the result.
    pub fn import(&mut self, bytes: &[u8]) -> Result<ImportOutcome> {
        let outcome = self.doc.import(bytes)?;
        if let Some(old) = self.session.take() {
            let Session {
                key,
                authenticated_at_ms,
                ..
            } = old;
            self.session = Some(Session::load(key, &self.doc, authenticated_at_ms));
        }
        Ok(outcome)
    }
}
