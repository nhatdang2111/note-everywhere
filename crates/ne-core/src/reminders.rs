//! Reminders: a date and time attached to a page, or to a date property of a
//! database row.
//!
//! All reminders live in one document that syncs like any other. A reminder
//! stores the page it is about, optionally a date property of that page (a row
//! of a database), the moment to fire, the viewer's UTC offset and a note.
//!
//! # When a reminder is due
//!
//! A reminder on a page is due at its stored moment. A reminder on a date
//! property follows the property: the due moment is the start of the row's
//! date, read at the time (a date without a time of day counts as
//! [`DEFAULT_REMINDER_MINUTES`] after midnight in the reminder's UTC offset), so
//! changing the date moves the reminder; when the cell is empty or no longer a
//! date, the stored moment from creation is used. Snoozing sets a later moment
//! that wins as long as it is after the due moment.
//!
//! Which reminders fire on this device, once, and which are too old to fire
//! after the device was off, is device state that lives in the store.
//!
//! Concurrency: every field is its own key under the reminder's id, so marking
//! one done on a phone while editing its note on a laptop keeps both; a delete
//! wins over concurrent edits.

use loro::{LoroMap, LoroValue, ValueOrContainer};
use serde::Serialize;

use crate::database::{DateValue, fresh_id};
use crate::error::{Error, Result};
use crate::kv_doc::{ImportOutcome, KvDoc};
use crate::roles::Role;

const REMINDERS: &str = "reminders";
const KEY_PAGE: &str = "page";
const KEY_PROPERTY: &str = "property";
const KEY_AT: &str = "at";
const KEY_OFFSET: &str = "offset";
const KEY_NOTE: &str = "note";
const KEY_DONE: &str = "done";
const KEY_SNOOZED: &str = "snoozed_until";
const KEY_CREATED_AT: &str = "created_at";

/// A date without a time of day fires this many minutes after midnight.
pub const DEFAULT_REMINDER_MINUTES: i64 = 9 * 60;
/// Longest note, in characters.
pub const MAX_NOTE_CHARS: usize = 500;

/// One reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    /// Stable id.
    pub id: String,
    /// Node id of the page (or database row) it is about.
    pub page_id: String,
    /// The date property it follows, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_id: Option<String>,
    /// The moment to fire, in ms since the Unix epoch (for a property
    /// reminder, the moment when it was created; see the module docs).
    pub at: i64,
    /// UTC offset of the person's clock when it was created, in minutes.
    pub utc_offset_minutes: i64,
    /// A short note shown with the reminder.
    pub note: String,
    /// Marked done.
    pub done: bool,
    /// Snoozed until this moment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<i64>,
    /// Creation time in ms.
    pub created_at: i64,
}

impl Reminder {
    /// The moment this reminder is due, given the current value of the date
    /// property it follows (if it follows one).
    pub fn due_at(&self, property_date: Option<&DateValue>) -> i64 {
        let base = property_date
            .and_then(|d| d.start_ms(self.utc_offset_minutes, DEFAULT_REMINDER_MINUTES))
            .unwrap_or(self.at);
        base.max(self.snoozed_until.unwrap_or(i64::MIN))
    }
}

/// The document that holds every reminder.
pub struct RemindersDoc {
    base: KvDoc,
}

fn text(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn number(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::I64(n)) => Some(n),
        _ => None,
    }
}

impl RemindersDoc {
    /// An empty document. Devices that each create one merge when they sync.
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: KvDoc::new(Role::Reminders)?,
        })
    }

    /// Loads the document from a snapshot or its first update.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            base: KvDoc::from_snapshot(bytes)?,
        })
    }

    /// Sets this replica's id.
    pub fn set_peer_id(&self, peer_id: u64) -> Result<()> {
        self.base.set_peer_id(peer_id)
    }

    /// Full state for storage.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.base.export_snapshot()
    }

    /// The whole history as updates, for the first push.
    pub fn export_all_updates(&self) -> Result<Vec<u8>> {
        self.base.export_all_updates()
    }

    /// Changes a replica at `version` lacks.
    pub fn export_updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        self.base.export_updates_since(version)
    }

    /// Applies a snapshot or updates.
    pub fn import(&self, bytes: &[u8]) -> Result<ImportOutcome> {
        self.base.import(bytes)
    }

    /// The version vector.
    pub fn version(&self) -> Vec<u8> {
        self.base.version()
    }

    /// Runs `edit` and returns the update holding exactly its changes.
    pub fn edit<R>(&self, edit: impl FnOnce(&Self) -> Result<R>) -> Result<(R, Vec<u8>)> {
        self.base.edit(|_| edit(self))
    }

    /// Adds a reminder and returns its id.
    pub fn add(
        &self,
        page_id: &str,
        property_id: Option<&str>,
        at: i64,
        utc_offset_minutes: i64,
        note: &str,
        now: i64,
    ) -> Result<String> {
        if page_id.is_empty() {
            return Err(Error::Invalid("a reminder needs a page".into()));
        }
        if !(-14 * 60..=14 * 60).contains(&utc_offset_minutes) {
            return Err(Error::Invalid("the UTC offset is out of range".into()));
        }
        let note = check_note(note)?;
        let id = fresh_id();
        let map = self
            .base
            .map(REMINDERS)
            .insert_container(&id, LoroMap::new())?;
        map.insert(KEY_PAGE, page_id)?;
        if let Some(property) = property_id {
            map.insert(KEY_PROPERTY, property)?;
        }
        map.insert(KEY_AT, at)?;
        map.insert(KEY_OFFSET, utc_offset_minutes)?;
        map.insert(KEY_NOTE, note.as_str())?;
        map.insert(KEY_DONE, false)?;
        map.insert(KEY_CREATED_AT, now)?;
        self.base.doc.commit();
        Ok(id)
    }

    fn map_of(&self, id: &str) -> Result<LoroMap> {
        self.base
            .map(REMINDERS)
            .get(id)
            .and_then(|v| v.into_container().ok())
            .and_then(|c| c.into_map().ok())
            .ok_or_else(|| Error::Missing(format!("reminder {id}")))
    }

    fn read(id: &str, map: &LoroMap) -> Option<Reminder> {
        Some(Reminder {
            id: id.to_owned(),
            page_id: text(map, KEY_PAGE)?,
            property_id: text(map, KEY_PROPERTY),
            at: number(map, KEY_AT)?,
            utc_offset_minutes: number(map, KEY_OFFSET).unwrap_or(0),
            note: text(map, KEY_NOTE).unwrap_or_default(),
            done: matches!(
                map.get(KEY_DONE),
                Some(ValueOrContainer::Value(LoroValue::Bool(true)))
            ),
            snoozed_until: number(map, KEY_SNOOZED),
            created_at: number(map, KEY_CREATED_AT).unwrap_or(0),
        })
    }

    /// One reminder.
    pub fn get(&self, id: &str) -> Result<Reminder> {
        Self::read(id, &self.map_of(id)?).ok_or_else(|| Error::Missing(format!("reminder {id}")))
    }

    /// Every reminder, oldest first.
    pub fn list(&self) -> Vec<Reminder> {
        let all = self.base.map(REMINDERS);
        let mut out: Vec<Reminder> = all
            .keys()
            .filter_map(|id| {
                let map = all.get(&id)?.into_container().ok()?.into_map().ok()?;
                Self::read(&id, &map)
            })
            .collect();
        out.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        out
    }

    /// Marks a reminder done or open again. Doing so clears a snooze.
    pub fn set_done(&self, id: &str, done: bool) -> Result<()> {
        let map = self.map_of(id)?;
        map.insert(KEY_DONE, done)?;
        if map.get(KEY_SNOOZED).is_some() {
            map.delete(KEY_SNOOZED)?;
        }
        self.base.doc.commit();
        Ok(())
    }

    /// Snoozes until `until` (ms): the reminder is open again and due then.
    pub fn snooze(&self, id: &str, until: i64) -> Result<()> {
        let map = self.map_of(id)?;
        map.insert(KEY_SNOOZED, until)?;
        map.insert(KEY_DONE, false)?;
        self.base.doc.commit();
        Ok(())
    }

    /// Moves a reminder to a new moment; clears a snooze.
    pub fn reschedule(&self, id: &str, at: i64) -> Result<()> {
        let map = self.map_of(id)?;
        map.insert(KEY_AT, at)?;
        if map.get(KEY_SNOOZED).is_some() {
            map.delete(KEY_SNOOZED)?;
        }
        map.insert(KEY_DONE, false)?;
        self.base.doc.commit();
        Ok(())
    }

    /// Changes the note.
    pub fn set_note(&self, id: &str, note: &str) -> Result<()> {
        let note = check_note(note)?;
        self.map_of(id)?.insert(KEY_NOTE, note.as_str())?;
        self.base.doc.commit();
        Ok(())
    }

    /// Deletes a reminder.
    pub fn delete(&self, id: &str) -> Result<()> {
        self.map_of(id)?;
        self.base.map(REMINDERS).delete(id)?;
        self.base.doc.commit();
        Ok(())
    }
}

fn check_note(note: &str) -> Result<String> {
    let note = note.trim();
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(Error::Invalid(format!(
            "a note is at most {MAX_NOTE_CHARS} characters"
        )));
    }
    Ok(note.to_owned())
}
