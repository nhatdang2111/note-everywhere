//! Reminders on the [`Workspace`]: the shared document of reminders, and the
//! per-device record of which ones already fired.
//!
//! A reminder fires on a device at most once for a given due moment: the
//! device records `(reminder, due moment)` the first time it decides, so a
//! restart does not fire it again, and a snooze or a new date (a new due
//! moment) fires again. A due moment older than the grace window when the
//! device looks (it was off or asleep) is recorded as skipped and never fires;
//! such a reminder still shows as overdue in the list.

use ne_core::NodeId;
use ne_core::database::CellValue;
use ne_core::reminders::{Reminder, RemindersDoc};
use ne_core::tree::NodeKind;
use rusqlite::params;

use crate::error::{Error, Result};
use crate::workspace::Workspace;

/// A reminder with what a list or a notification needs next to it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReminderEntry {
    /// The reminder.
    pub reminder: Reminder,
    /// When it is due now (follows the date property, honours a snooze).
    pub due_at: i64,
    /// Title of the page it is about; empty if the page is gone.
    pub page_title: String,
    /// False when the page is gone or in the trash.
    pub page_visible: bool,
}

impl Workspace {
    fn edit_reminders<R>(
        &mut self,
        edit: impl FnOnce(&RemindersDoc) -> ne_core::Result<R>,
    ) -> Result<R> {
        let doc = self.load_reminders()?;
        let before = doc.version();
        let (value, update) = match doc.edit(edit) {
            Ok(done) => done,
            Err(err) => {
                self.reminders = None;
                return Err(err.into());
            }
        };
        let doc = self.load_reminders()?;
        let version = doc.version();
        if version == before {
            return Ok(value);
        }
        let state = (doc.export_snapshot()?, version.clone());
        self.commit_singleton_update(NodeId::reminders(), &update, &version, true, || Ok(state))?;
        Ok(value)
    }

    /// The date a row's property holds, if the page is a row with a date there.
    fn property_date(
        &mut self,
        page: NodeId,
        property: &str,
    ) -> Option<ne_core::database::DateValue> {
        let parent = self.node(page).ok()??.parent?;
        match self.database_cell(parent, page, property).ok()?? {
            CellValue::Date(date) => Some(date),
            _ => None,
        }
    }

    /// Adds a reminder on a page, or on a date property of a row (then `at`
    /// may be left out and is read from the cell). `utc_offset_minutes` is the
    /// person's clock offset, used to place a date without a time.
    pub fn reminder_add(
        &mut self,
        page: NodeId,
        property: Option<&str>,
        at: Option<i64>,
        utc_offset_minutes: i64,
        note: &str,
    ) -> Result<Reminder> {
        let Some(info) = self.node(page)? else {
            return Err(Error::NotFound(format!("page {page}")));
        };
        if info.kind == NodeKind::Folder {
            return Err(Error::InvalidInput("a reminder is about a page".into()));
        }
        let at = match (property, at) {
            (Some(prop), at) => {
                let date = self
                    .property_date(page, prop)
                    .ok_or_else(|| Error::InvalidInput("that property holds no date".into()))?;
                let from_cell = date.start_ms(
                    utc_offset_minutes,
                    ne_core::reminders::DEFAULT_REMINDER_MINUTES,
                );
                at.or(from_cell).ok_or_else(|| {
                    Error::InvalidInput("the date cannot be placed in time".into())
                })?
            }
            (None, Some(at)) => at,
            (None, None) => return Err(Error::InvalidInput("a reminder needs a time".into())),
        };
        let now = self.now();
        let key = page.to_string();
        let id =
            self.edit_reminders(|doc| doc.add(&key, property, at, utc_offset_minutes, note, now))?;
        Ok(self.load_reminders()?.get(&id)?)
    }

    fn entry(&mut self, reminder: Reminder) -> ReminderEntry {
        let node = reminder
            .page_id
            .parse::<NodeId>()
            .ok()
            .and_then(|id| self.node(id).ok().flatten().map(|row| (id, row)));
        let date = match (&node, &reminder.property_id) {
            (Some((id, _)), Some(prop)) => self.property_date(*id, prop),
            _ => None,
        };
        ReminderEntry {
            due_at: reminder.due_at(date.as_ref()),
            page_title: node
                .as_ref()
                .map(|(_, n)| n.title.clone())
                .unwrap_or_default(),
            page_visible: node.as_ref().is_some_and(|(_, n)| !n.hidden),
            reminder,
        }
    }

    /// Reminders sorted by due moment. Done reminders are left out unless
    /// `include_done` is set.
    pub fn reminders(&mut self, include_done: bool) -> Result<Vec<ReminderEntry>> {
        if !self.has_document(NodeId::reminders())? {
            return Ok(Vec::new());
        }
        let all = self.load_reminders()?.list();
        let mut entries: Vec<ReminderEntry> = all
            .into_iter()
            .filter(|r| include_done || !r.done)
            .map(|r| self.entry(r))
            .collect();
        entries.sort_by(|a, b| (a.due_at, &a.reminder.id).cmp(&(b.due_at, &b.reminder.id)));
        Ok(entries)
    }

    /// Marks a reminder done, or open again.
    pub fn reminder_set_done(&mut self, id: &str, done: bool) -> Result<()> {
        self.edit_reminders(|doc| doc.set_done(id, done))
    }

    /// Snoozes a reminder until `until` (ms since the Unix epoch).
    pub fn reminder_snooze(&mut self, id: &str, until: i64) -> Result<()> {
        self.edit_reminders(|doc| doc.snooze(id, until))
    }

    /// Moves a reminder to a new moment.
    pub fn reminder_reschedule(&mut self, id: &str, at: i64) -> Result<()> {
        self.edit_reminders(|doc| doc.reschedule(id, at))
    }

    /// Changes a reminder's note.
    pub fn reminder_set_note(&mut self, id: &str, note: &str) -> Result<()> {
        self.edit_reminders(|doc| doc.set_note(id, note))
    }

    /// Deletes a reminder.
    pub fn reminder_delete(&mut self, id: &str) -> Result<()> {
        self.edit_reminders(|doc| doc.delete(id))
    }

    /// The reminders that should fire now on this device: due at or before
    /// `now`, not done, about a visible page, not fired for this due moment
    /// before, and not older than `grace_ms`. Each is recorded as fired, so
    /// calling again returns only what became due since. Reminders found
    /// older than the grace window are recorded as skipped.
    pub fn reminders_take_due(&mut self, now: i64, grace_ms: i64) -> Result<Vec<ReminderEntry>> {
        if !self.has_document(NodeId::reminders())? {
            return Ok(Vec::new());
        }
        let entries = self.reminders(false)?;
        let mut fire = Vec::new();
        for entry in entries {
            if entry.due_at > now || !entry.page_visible {
                continue;
            }
            let outcome = if now - entry.due_at > grace_ms {
                "skipped"
            } else {
                "fired"
            };
            let inserted = self.conn.execute(
                "INSERT OR IGNORE INTO reminder_state (reminder_id, due_at, outcome, decided_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![entry.reminder.id, entry.due_at, outcome, now],
            )?;
            if inserted == 1 && outcome == "fired" {
                fire.push(entry);
            }
        }
        // Drop the record of reminders that no longer exist.
        let live: Vec<String> = self
            .load_reminders()?
            .list()
            .into_iter()
            .map(|r| r.id)
            .collect();
        let known: Vec<String> = self
            .conn
            .prepare_cached("SELECT DISTINCT reminder_id FROM reminder_state")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in known.iter().filter(|id| !live.contains(id)) {
            self.conn
                .execute("DELETE FROM reminder_state WHERE reminder_id = ?1", [id])?;
        }
        Ok(fire)
    }

    /// The version of the reminders document, for noticing that another
    /// device changed it.
    pub fn reminders_version(&mut self) -> Result<Vec<u8>> {
        if !self.has_document(NodeId::reminders())? {
            return Ok(Vec::new());
        }
        Ok(self.load_reminders()?.version())
    }
}
