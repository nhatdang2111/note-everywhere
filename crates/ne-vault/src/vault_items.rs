//! Item operations of an unlocked [`Vault`].
//!
//! Every method fails with [`VaultError::Locked`] while the vault is locked.
//! Methods that hand out secret text (`reveal`, `copy`, `resolve`,
//! `export_csv`) also require recent authentication, see
//! [`Vault::ensure_fresh`].

use zeroize::Zeroizing;

use crate::csv::write_record;
use crate::doc::VaultDoc;
use crate::error::{Result, VaultError};
use crate::import::{CsvFormat, ImportReport, parse_csv};
use crate::item::{Field, ItemBody, ItemContent, ItemDraft, ItemId, ItemView};
use crate::lock::SensitiveAction;
use crate::record::seal_content;
use crate::search::{matches, query_tokens};
use crate::secret_ref::{Resolution, SecretRef};
use crate::vault::{Entry, Session, Vault};

/// A value for the clipboard and when to take it off again.
pub struct ClipboardPayload {
    /// The text to put on the clipboard. Wiped on drop.
    pub value: Zeroizing<String>,
    /// When the caller must clear the clipboard (ms since the Unix epoch), if
    /// it still holds this value. The caller owns the real clipboard.
    pub clear_at_ms: i64,
}

impl std::fmt::Debug for ClipboardPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ClipboardPayload(<redacted>, clear_at_ms: {})",
            self.clear_at_ms
        )
    }
}

fn insert(doc: &VaultDoc, session: &mut Session, draft: ItemDraft, now: i64) -> Result<ItemId> {
    let draft = draft.normalized()?;
    let id = ItemId::generate();
    let content = ItemContent::new(draft, now);
    doc.put(&id, &seal_content(&session.key, &id, 1, &content)?)?;
    session.items.insert(id, Entry::new(content));
    Ok(id)
}

impl Vault {
    fn open_session(&self) -> Result<&Session> {
        self.session.as_ref().ok_or(VaultError::Locked)
    }

    fn entry(&self, id: &ItemId) -> Result<&Entry> {
        self.open_session()?
            .items
            .get(id)
            .ok_or(VaultError::NotFound(*id))
    }

    /// Adds an item and returns its new id.
    pub fn add(&mut self, draft: ItemDraft) -> Result<ItemId> {
        let now = self.clock.now_ms();
        let session = self.session.as_mut().ok_or(VaultError::Locked)?;
        insert(&self.doc, session, draft, now)
    }

    /// Replaces the contents of an item, keeping its id and creation time.
    pub fn update(&mut self, id: &ItemId, draft: ItemDraft) -> Result<()> {
        let now = self.clock.now_ms();
        let session = self.session.as_mut().ok_or(VaultError::Locked)?;
        let created = session
            .items
            .get(id)
            .ok_or(VaultError::NotFound(*id))?
            .content
            .created_ms;
        let content = ItemContent::from_draft(draft.normalized()?, created, now);
        let counter = self.doc.counter_of(id).saturating_add(1);
        self.doc
            .put(id, &seal_content(&session.key, id, counter, &content)?)?;
        session.items.insert(*id, Entry::new(content));
        Ok(())
    }

    /// Deletes an item. The deletion is a tombstone that syncs and cannot be
    /// undone by an older replica.
    pub fn delete(&mut self, id: &ItemId) -> Result<()> {
        let session = self.session.as_mut().ok_or(VaultError::Locked)?;
        if !session.items.contains_key(id) {
            return Err(VaultError::NotFound(*id));
        }
        self.doc
            .tombstone(id, self.doc.counter_of(id).saturating_add(1))?;
        session.items.remove(id);
        Ok(())
    }

    /// The full stored content of an item, for merging an edit into it. It
    /// needs no recent authentication because the caller is the application
    /// itself re-sealing the result; it must never be shown or sent anywhere.
    pub fn draft_of(&self, id: &ItemId) -> Result<ItemDraft> {
        let content = &self.entry(id)?.content;
        Ok(ItemDraft {
            title: content.title.clone(),
            tags: content.tags.clone(),
            body: content.body.clone(),
        })
    }

    /// One item without its secret fields.
    pub fn get(&self, id: &ItemId) -> Result<ItemView> {
        Ok(ItemView::of(*id, &self.entry(id)?.content))
    }

    /// All items without their secret fields, by title then id.
    pub fn list(&self) -> Result<Vec<ItemView>> {
        self.collect(|_| true)
    }

    /// Items whose title, username, tags or URLs contain every word of
    /// `query`, ignoring case and diacritics. A blank query lists everything.
    pub fn search(&self, query: &str) -> Result<Vec<ItemView>> {
        let tokens = query_tokens(query);
        self.collect(|entry| matches(&entry.search, &tokens))
    }

    fn collect(&self, keep: impl Fn(&Entry) -> bool) -> Result<Vec<ItemView>> {
        let mut found: Vec<(String, ItemView)> = self
            .open_session()?
            .items
            .iter()
            .filter(|(_, entry)| keep(entry))
            .map(|(id, entry)| {
                (
                    entry.content.title.to_lowercase(),
                    ItemView::of(*id, &entry.content),
                )
            })
            .collect();
        found.sort_by(|a, b| (&a.0, a.1.id).cmp(&(&b.0, b.1.id)));
        Ok(found.into_iter().map(|(_, view)| view).collect())
    }

    fn field_value(&self, id: &ItemId, field: Field) -> Result<Zeroizing<String>> {
        let text = self
            .entry(id)?
            .content
            .body
            .field(field)
            .ok_or(VaultError::NoSuchField)?;
        Ok(Zeroizing::new(text.to_owned()))
    }

    /// The text of one field, for display.
    pub fn reveal(&self, id: &ItemId, field: Field) -> Result<Zeroizing<String>> {
        self.ensure_fresh(SensitiveAction::Reveal)?;
        self.field_value(id, field)
    }

    /// The text of one field for the clipboard, with the time to clear it.
    pub fn copy(&self, id: &ItemId, field: Field) -> Result<ClipboardPayload> {
        self.ensure_fresh(SensitiveAction::Copy)?;
        let value = self.field_value(id, field)?;
        let clear_after =
            i64::try_from(self.policy.clipboard_clear_after.as_millis()).unwrap_or(i64::MAX);
        Ok(ClipboardPayload {
            value,
            clear_at_ms: self.clock.now_ms().saturating_add(clear_after),
        })
    }

    /// Looks up a [`SecretRef`]: the value while unlocked and recently
    /// authenticated, [`Resolution::Redacted`] otherwise (locked and
    /// needs-reauthentication are deliberately indistinguishable and reveal
    /// nothing about whether the item exists).
    pub fn resolve(&self, reference: &SecretRef) -> Resolution {
        if self.ensure_fresh(SensitiveAction::Reveal).is_err() {
            return Resolution::Redacted;
        }
        match self.field_value(&reference.item, reference.field) {
            Ok(value) => Resolution::Value(value),
            Err(_) => Resolution::Missing,
        }
    }

    /// Imports a CSV file and adds every valid row. Row-level problems are in
    /// the report, never in the error.
    pub fn import_csv(&mut self, csv: &[u8], format: &CsvFormat) -> Result<ImportReport> {
        let now = self.clock.now_ms();
        let session = self.session.as_mut().ok_or(VaultError::Locked)?;
        let batch = parse_csv(csv, format)?;
        for draft in batch.drafts {
            insert(&self.doc, session, draft, now)?;
        }
        Ok(batch.report)
    }

    /// Exports every item as CSV in the layout of
    /// [`crate::ColumnMapping::vault_export`], which `import_csv` reads back.
    /// The text is plaintext: protect and delete it.
    pub fn export_csv(&self) -> Result<Zeroizing<String>> {
        self.ensure_fresh(SensitiveAction::Export)?;
        let mut out = Zeroizing::new(String::new());
        write_record(
            &mut out,
            &[
                "type", "title", "username", "password", "url", "notes", "tags",
            ],
        );
        for view in self.list()? {
            let content = &self.entry(&view.id)?.content;
            let (kind, username, password, url, notes) = match &content.body {
                ItemBody::Login {
                    username,
                    password,
                    urls,
                    notes,
                } => (
                    "login",
                    username.as_str(),
                    password.as_str(),
                    urls.join("\n"),
                    notes.as_str(),
                ),
                ItemBody::SecureNote { text } => ("note", "", "", String::new(), text.as_str()),
                ItemBody::ApiKey { value, notes } => {
                    ("apikey", "", value.as_str(), String::new(), notes.as_str())
                }
            };
            let row = Zeroizing::new(vec![
                kind.to_owned(),
                content.title.clone(),
                username.to_owned(),
                password.to_owned(),
                url,
                notes.to_owned(),
                content.tags.join(","),
            ]);
            write_record(&mut out, &row);
        }
        Ok(out)
    }
}
