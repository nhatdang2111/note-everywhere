//! The secrets vault: creation, unlocking, items, clipboard and auto-lock.
//!
//! Lock order is vault, then workspace; the sync adapter never holds both.
//! Key derivation runs on the blocking pool while the vault mutex is held,
//! which only delays other vault calls.

use std::str::FromStr;
use std::time::Duration;

use ne_crypto::account::{create_vault, unlock_with_password};
use ne_crypto::recovery::RecoveryKey;
use ne_platform::activity::{ActivityMonitor, IdleSource};
use ne_vault::generator::{self, PassphraseSpec, PasswordSpec};
use ne_vault::import::{ColumnMapping, CsvFormat, parse_csv};
use ne_vault::{Field, ItemBody, ItemDraft, ItemId, ItemKind, ItemView, LockEvent, Vault};
use zeroize::Zeroizing;

use super::sync_store::VAULT_DOC;
use super::types::{
    GeneratePasswordArgs, GeneratedPassword, SkippedRow, VaultImportReport, VaultItemInput,
    VaultItemSummary,
};
use super::{AppCore, AppError, AppResult, CoreEvent, lock};

const MAX_CSV_BYTES: u64 = 16 * 1024 * 1024;
/// Items per stored update while importing, to keep each sync update small.
const IMPORT_CHUNK: usize = 40;

fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Login => "login",
        ItemKind::SecureNote => "secure_note",
        ItemKind::ApiKey => "api_key",
    }
}

fn summary(view: &ItemView) -> VaultItemSummary {
    VaultItemSummary {
        id: view.id.to_string(),
        kind: kind_name(view.kind),
        title: view.title.clone(),
        username: view.username.clone(),
        urls: view.urls.clone(),
        tags: view.tags.clone(),
        updated_at: view.updated_ms,
    }
}

fn item_id(text: &str) -> AppResult<ItemId> {
    ItemId::from_str(text).map_err(|_| AppError::not_found("That vault item"))
}

fn uninitialised() -> AppError {
    AppError::new(
        "vault_uninitialised",
        "There is no vault on this device yet.",
    )
}

/// Builds the body of an item from the input, keeping what an update leaves out.
fn merge(input: &VaultItemInput, stored: Option<&ItemBody>) -> AppResult<ItemBody> {
    let pick = |new: &Option<String>, old: Option<String>| new.clone().or(old).unwrap_or_default();
    Ok(match (input.kind.as_str(), stored) {
        (
            "login",
            Some(ItemBody::Login {
                username,
                password,
                urls,
                notes,
            }),
        ) => ItemBody::Login {
            username: pick(&input.username, Some(username.clone())),
            password: pick(&input.password, Some(password.clone())),
            urls: input.urls.clone().unwrap_or_else(|| urls.clone()),
            notes: pick(&input.notes, Some(notes.clone())),
        },
        ("login", _) => ItemBody::Login {
            username: pick(&input.username, None),
            password: pick(&input.password, None),
            urls: input.urls.clone().unwrap_or_default(),
            notes: pick(&input.notes, None),
        },
        ("secure_note", Some(ItemBody::SecureNote { text })) => ItemBody::SecureNote {
            text: pick(&input.notes, Some(text.clone())),
        },
        ("secure_note", _) => ItemBody::SecureNote {
            text: pick(&input.notes, None),
        },
        ("api_key", Some(ItemBody::ApiKey { value, notes })) => ItemBody::ApiKey {
            value: pick(&input.value, Some(value.clone())),
            notes: pick(&input.notes, Some(notes.clone())),
        },
        ("api_key", _) => ItemBody::ApiKey {
            value: pick(&input.value, None),
            notes: pick(&input.notes, None),
        },
        _ => {
            return Err(AppError::invalid(
                "The item kind must be login, secure_note or api_key.",
            ));
        }
    })
}

impl AppCore {
    pub fn vault_state_now(&self) -> &'static str {
        match lock(&self.inner.vault).as_ref() {
            None => "uninitialised",
            Some(v) if v.is_unlocked() => "unlocked",
            Some(_) => "locked",
        }
    }

    fn emit_vault_state(&self) {
        self.emit(CoreEvent::VaultState {
            state: self.vault_state_now(),
        });
    }

    pub async fn vault_state(&self) -> AppResult<&'static str> {
        Ok(self.vault_state_now())
    }

    /// Runs `f` on the vault, then stores and queues whatever it changed.
    fn vault_edit<R>(&self, f: impl FnOnce(&mut Vault) -> AppResult<R>) -> AppResult<R> {
        let mut guard = lock(&self.inner.vault);
        let vault = guard.as_mut().ok_or_else(uninitialised)?;
        let before = vault.version();
        let out = f(vault);
        self.persist_vault_delta(vault, &before)?;
        out
    }

    fn vault_read<R>(&self, f: impl FnOnce(&Vault) -> AppResult<R>) -> AppResult<R> {
        let guard = lock(&self.inner.vault);
        f(guard.as_ref().ok_or_else(uninitialised)?)
    }

    // ---- create, unlock, lock ----

    pub async fn vault_create(&self, password: String) -> AppResult<(String, bool)> {
        if password.is_empty() {
            return Err(AppError::invalid("The vault password must not be empty."));
        }
        if lock(&self.inner.vault).is_some() {
            return Err(AppError::new(
                "vault_exists",
                "This device already has a vault.",
            ));
        }
        let account_record = self.store(|ws| ws.setting("session.record"))?;
        let (params, policy) = (self.inner.cfg.kdf_params, self.inner.cfg.kdf_policy);
        let (created, same_as_account) = self
            .blocking(move |_| {
                let created = create_vault(password.as_bytes(), params, &policy)?;
                // It equals the account password exactly when it opens the account record.
                let same = account_record
                    .and_then(|json| serde_json::from_str(&json).ok())
                    .is_some_and(|record| {
                        unlock_with_password(password.as_bytes(), &record, &policy).is_ok()
                    });
                Ok((created, same))
            })
            .await?;
        let doc = ne_vault::VaultDoc::new();
        let snapshot = doc
            .export_snapshot()
            .map_err(|_| AppError::internal("The vault could not be created."))?;
        self.store(|ws| ws.put_vault_doc(VAULT_DOC, &snapshot, &doc.version()))?;
        self.publish_vault_record(&created.record).await?;
        self.load_vault()?;
        {
            let mut guard = lock(&self.inner.vault);
            let vault = guard.as_mut().ok_or_else(uninitialised)?;
            vault.unlock_with_recovery_key(&created.recovery_key)?;
        }
        self.emit_vault_state();
        Ok((
            created.recovery_key.to_display_string().to_string(),
            same_as_account,
        ))
    }

    pub async fn vault_unlock(&self, password: String) -> AppResult<()> {
        let core = self.clone();
        self.blocking(move |_| {
            core.vault_edit(|v| Ok(v.unlock_with_password(password.as_bytes())?))
        })
        .await?;
        self.emit_vault_state();
        Ok(())
    }

    pub async fn vault_recover(&self, recovery_key: &str, new_password: String) -> AppResult<()> {
        if new_password.is_empty() {
            return Err(AppError::invalid(
                "The new vault password must not be empty.",
            ));
        }
        let key = RecoveryKey::parse(recovery_key).map_err(|_| {
            AppError::new(
                "invalid_recovery_key",
                "That recovery key is not valid. Check it for typing mistakes.",
            )
        })?;
        let params = self.inner.cfg.kdf_params;
        let core = self.clone();
        let record = self
            .blocking(move |_| {
                core.vault_edit(|v| {
                    v.unlock_with_recovery_key(&key)
                        .map_err(|e| match AppError::from(e) {
                            e if e.code == "wrong_password" => AppError::new(
                                "wrong_recovery_key",
                                "That recovery key does not open this vault.",
                            ),
                            other => other,
                        })?;
                    Ok(v.change_password(new_password.as_bytes(), params)?)
                })
            })
            .await?;
        self.publish_vault_record(&record).await?;
        self.emit_vault_state();
        Ok(())
    }

    pub async fn vault_lock(&self) -> AppResult<()> {
        self.vault_read(|_| Ok(()))?;
        if let Some(vault) = lock(&self.inner.vault).as_mut() {
            vault.lock();
        }
        self.emit_vault_state();
        Ok(())
    }

    pub async fn vault_change_password(&self, current: String, new: String) -> AppResult<()> {
        if new.is_empty() {
            return Err(AppError::invalid(
                "The new vault password must not be empty.",
            ));
        }
        let params = self.inner.cfg.kdf_params;
        let core = self.clone();
        let record = self
            .blocking(move |_| {
                core.vault_edit(|v| {
                    v.reauthenticate_with_password(current.as_bytes())?;
                    Ok(v.change_password(new.as_bytes(), params)?)
                })
            })
            .await?;
        self.publish_vault_record(&record).await
    }

    // ---- auto-lock ----

    /// Feeds a device event to the auto-lock policy. Returns whether the vault
    /// is locked afterwards.
    pub fn vault_lock_event(&self, event: LockEvent) -> bool {
        let (was, now) = {
            let mut guard = lock(&self.inner.vault);
            let Some(vault) = guard.as_mut() else {
                return true;
            };
            let was = vault.is_unlocked();
            (was, vault.handle_event(event))
        };
        if was && now {
            self.emit_vault_state();
        }
        now
    }

    /// One poll of the activity monitor; locks the vault if it says so.
    pub fn poll_activity<S: IdleSource>(&self, monitor: &mut ActivityMonitor<S>) {
        monitor.set_idle_report_after(*self.inner.idle_after.borrow());
        for event in monitor.poll().events {
            self.vault_lock_event(event);
        }
    }

    /// Starts the background work that is not tied to a command: the vault
    /// auto-lock poll. Needs a tokio runtime.
    pub fn start_background(&self) {
        self.register_saved_hotkey();
        let core = self.clone();
        self.spawn(self.inner.rt.spawn(async move {
            let mut monitor = ActivityMonitor::new(ne_platform::activity::MonitorConfig::default());
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                core.poll_activity(&mut monitor);
            }
        }));
    }

    // ---- items ----

    pub async fn vault_list(&self, query: Option<&str>) -> AppResult<Vec<VaultItemSummary>> {
        self.vault_read(|v| {
            let views = match query.map(str::trim).filter(|q| !q.is_empty()) {
                Some(q) => v.search(q)?,
                None => v.list()?,
            };
            Ok(views.iter().map(summary).collect())
        })
    }

    pub async fn vault_save(&self, item: VaultItemInput) -> AppResult<VaultItemSummary> {
        self.vault_edit(|v| {
            let existing = item.id.as_deref().map(item_id).transpose()?;
            let stored = match &existing {
                Some(id) => Some(v.draft_of(id)?),
                None => None,
            };
            let tags = item
                .tags
                .clone()
                .or_else(|| stored.as_ref().map(|d| d.tags.clone()))
                .unwrap_or_default();
            let body = merge(&item, stored.as_ref().map(|d| &d.body))?;
            let draft = ItemDraft {
                title: item.title.clone(),
                tags,
                body,
            };
            let id = match existing {
                Some(id) => {
                    v.update(&id, draft)?;
                    id
                }
                None => v.add(draft)?,
            };
            Ok(summary(&v.get(&id)?))
        })
    }

    pub async fn vault_delete(&self, id: &str) -> AppResult<()> {
        let id = item_id(id)?;
        self.vault_edit(|v| Ok(v.delete(&id)?))
    }

    fn field_of(vault: &Vault, id: &ItemId, field: &str) -> AppResult<Field> {
        Ok(match field {
            "password" => Field::Password,
            "value" => Field::Value,
            "username" => Field::Username,
            // A secure note's text is carried in the contract's `notes` field.
            "notes" if vault.get(id)?.kind == ItemKind::SecureNote => Field::Text,
            "notes" => Field::Notes,
            _ => return Err(AppError::invalid("Unknown vault field.")),
        })
    }

    pub async fn vault_reveal(
        &self,
        id: &str,
        field: &str,
        password: Option<String>,
    ) -> AppResult<String> {
        let id = item_id(id)?;
        let field = field.to_owned();
        let core = self.clone();
        self.blocking(move |_| {
            let value = core.vault_secret(&id, &field, password, false)?;
            Ok(value.to_string())
        })
        .await
    }

    /// Fetches a field after re-authenticating when a password is supplied.
    fn vault_secret(
        &self,
        id: &ItemId,
        field: &str,
        password: Option<String>,
        copy: bool,
    ) -> AppResult<Zeroizing<String>> {
        let mut guard = lock(&self.inner.vault);
        let vault = guard.as_mut().ok_or_else(uninitialised)?;
        if let Some(password) = password {
            vault.reauthenticate_with_password(password.as_bytes())?;
        }
        let field = Self::field_of(vault, id, field)?;
        if copy {
            Ok(vault.copy(id, field)?.value)
        } else {
            Ok(vault.reveal(id, field)?)
        }
    }

    pub async fn vault_copy(
        &self,
        id: &str,
        field: &str,
        password: Option<String>,
    ) -> AppResult<u64> {
        let id = item_id(id)?;
        let field = field.to_owned();
        let core = self.clone();
        let value = self
            .blocking(move |_| core.vault_secret(&id, &field, password, true))
            .await?;
        let wait = self.inner.cfg.clipboard_clear_after;
        self.inner.deps.clipboard.write_text(&value).map_err(|_| {
            AppError::new(
                "clipboard_unavailable",
                "The clipboard could not be written.",
            )
        })?;
        self.schedule_clipboard_clear(value, wait);
        Ok(wait.as_secs())
    }

    /// Clears the clipboard after `wait`, but only if it still holds `value`
    /// (the person may have copied something else since).
    fn schedule_clipboard_clear(&self, value: Zeroizing<String>, wait: Duration) {
        let clipboard = self.inner.deps.clipboard.clone();
        let task = self.inner.rt.spawn(async move {
            tokio::time::sleep(wait).await;
            if clipboard.read_text().as_deref() == Some(value.as_str()) {
                let _ = clipboard.write_text("");
            }
        });
        if let Some(previous) = lock(&self.inner.clipboard_task).replace(task) {
            previous.abort();
        }
    }

    pub async fn vault_generate_password(
        &self,
        args: GeneratePasswordArgs,
    ) -> AppResult<GeneratedPassword> {
        let generated = match args.mode.as_str() {
            "characters" => {
                let defaults = PasswordSpec::default();
                generator::generate_password(&PasswordSpec {
                    length: args.length,
                    lowercase: args.lowercase.unwrap_or(defaults.lowercase),
                    uppercase: args.uppercase.unwrap_or(defaults.uppercase),
                    digits: args.digits.unwrap_or(defaults.digits),
                    symbols: args.symbols.unwrap_or(defaults.symbols),
                    exclude_ambiguous: args.exclude_ambiguous.unwrap_or(defaults.exclude_ambiguous),
                })
            }
            "passphrase" => generator::generate_passphrase(&PassphraseSpec {
                words: args.length,
                ..PassphraseSpec::default()
            }),
            _ => {
                return Err(AppError::invalid(
                    "The mode must be characters or passphrase.",
                ));
            }
        }
        .map_err(|e| AppError::invalid(e.to_string()))?;
        Ok(GeneratedPassword {
            value: generated.value.to_string(),
            entropy_bits: generated.entropy_bits,
        })
    }

    pub async fn vault_import_csv(&self, format: &str) -> AppResult<Option<VaultImportReport>> {
        let csv_format = match format {
            "generic" => CsvFormat::Generic(ColumnMapping::vault_export()),
            "bitwarden" => CsvFormat::Bitwarden,
            "one_password" => CsvFormat::OnePassword,
            "last_pass" => CsvFormat::LastPass,
            _ => return Err(AppError::invalid("Unknown CSV format.")),
        };
        self.vault_read(|v| v.ensure_fresh_unlocked())?;
        let dialogs = self.inner.deps.dialogs.clone();
        let picked = self
            .blocking(move |_| Ok(dialogs.pick_file("Choose a CSV file to import", &["csv"])))
            .await?;
        let Some(path) = picked else {
            return Ok(None);
        };
        let bytes = self
            .blocking(move |_| {
                let io =
                    |_: std::io::Error| AppError::new("io_error", "The file could not be read.");
                if std::fs::metadata(&path).map_err(io)?.len() > MAX_CSV_BYTES {
                    return Err(AppError::new(
                        "too_large",
                        "That file is larger than 16 MiB.",
                    ));
                }
                Ok(Zeroizing::new(std::fs::read(&path).map_err(io)?))
            })
            .await?;
        let batch = parse_csv(&bytes, &csv_format)?;
        let report = batch.report.clone();
        let mut drafts = batch.drafts.into_iter().peekable();
        while drafts.peek().is_some() {
            let chunk: Vec<ItemDraft> = drafts.by_ref().take(IMPORT_CHUNK).collect();
            self.vault_edit(|v| {
                for draft in chunk {
                    v.add(draft)?;
                }
                Ok(())
            })?;
        }
        Ok(Some(VaultImportReport {
            imported: report.imported,
            skipped: report
                .skipped
                .iter()
                .map(|s| SkippedRow {
                    row: s.row,
                    reason: s.reason.to_string(),
                })
                .collect(),
        }))
    }
}

trait UnlockedCheck {
    fn ensure_fresh_unlocked(&self) -> AppResult<()>;
}

impl UnlockedCheck for Vault {
    /// Importing needs an unlocked vault; it does not expose a secret, so it
    /// does not ask for the password again.
    fn ensure_fresh_unlocked(&self) -> AppResult<()> {
        if self.is_unlocked() {
            Ok(())
        } else {
            Err(AppError::from(ne_vault::VaultError::Locked))
        }
    }
}
