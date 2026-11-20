//! Reminders for the UI, and the scheduler that fires the due ones.
//!
//! The scheduler looks every [`REMINDER_TICK`] and at start. A reminder that
//! became due fires as a `reminder:due` event, once per device; one that was
//! due more than [`REMINDER_GRACE`] before the device looked (it was off or
//! asleep) does not fire, it just shows as overdue.
//!
//! A due reminder also gets a native notification when the system allows it,
//! and the event says so (`nativeShown`) so the UI does not show a second one.
//! The permission is asked for when the first reminder is created, never at
//! launch. Without permission the event is all there is. On macOS and Windows
//! the plugin reports "granted" without asking, so a person who turned the
//! app's notifications off in system settings is not detected: the
//! notification is then sent but not displayed by the system, and the UI still
//! gets `nativeShown: true`.

use std::time::Duration;

use ne_core::NodeId;
use ne_core::reminders::Reminder;
use serde::Deserialize;

use super::deps::NotifyPermission;
use super::types::ReminderEntryDto;
use super::{AppCore, AppResult, CoreEvent, node_id};

/// How often the scheduler looks for due reminders.
pub const REMINDER_TICK: Duration = Duration::from_secs(30);
/// How late a reminder may still fire.
pub const REMINDER_GRACE: Duration = Duration::from_secs(10 * 60);
/// Up to this many due reminders get a notification each; more are summed up in one.
const NOTIFY_INDIVIDUALLY: usize = 3;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderAddArgs {
    pub page_id: String,
    pub property_id: Option<String>,
    pub at: Option<i64>,
    pub utc_offset_minutes: i64,
    pub note: Option<String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

impl AppCore {
    fn reminders_changed(&self) {
        self.emit(CoreEvent::RemindersChanged);
        self.after_local_change();
    }

    pub async fn reminder_add(&self, args: ReminderAddArgs) -> AppResult<Reminder> {
        let page: NodeId = node_id(&args.page_id)?;
        let reminder = self.store(|ws| {
            ws.reminder_add(
                page,
                args.property_id.as_deref(),
                args.at,
                args.utc_offset_minutes,
                args.note.as_deref().unwrap_or_default(),
            )
        })?;
        self.reminders_changed();
        self.ask_notification_permission();
        Ok(reminder)
    }

    /// The moment the person first sets a reminder is when a notification
    /// permission prompt makes sense. Asks once: a person who answered, either
    /// way, is not asked again.
    fn ask_notification_permission(&self) {
        let notifier = self.inner.deps.notifier.clone();
        if notifier.permission() != NotifyPermission::Undetermined {
            return;
        }
        // The prompt blocks until the person answers.
        self.spawn(self.inner.rt.spawn_blocking(move || {
            notifier.request_permission();
        }));
    }

    /// Shows the native notifications for `due`; true when every one was shown.
    fn notify_due(&self, due: &[ReminderEntryDto]) -> bool {
        let notifier = &self.inner.deps.notifier;
        if notifier.permission() != NotifyPermission::Granted {
            return false;
        }
        if due.len() > NOTIFY_INDIVIDUALLY {
            let body = format!("{} reminders are due.", due.len());
            return notifier.show("Reminders", &body).is_ok();
        }
        due.iter().fold(true, |all, entry| {
            let title = if entry.page_title.trim().is_empty() {
                "Reminder"
            } else {
                entry.page_title.as_str()
            };
            let body = if entry.reminder.note.trim().is_empty() {
                "A reminder is due."
            } else {
                entry.reminder.note.as_str()
            };
            notifier.show(title, body).is_ok() && all
        })
    }

    pub async fn reminder_list(
        &self,
        include_done: Option<bool>,
    ) -> AppResult<Vec<ReminderEntryDto>> {
        self.store(|ws| ws.reminders(include_done.unwrap_or(false)))
            .map(|entries| entries.into_iter().map(ReminderEntryDto::from).collect())
    }

    pub async fn reminder_set_done(&self, id: &str, done: bool) -> AppResult<()> {
        self.store(|ws| ws.reminder_set_done(id, done))?;
        self.reminders_changed();
        Ok(())
    }

    pub async fn reminder_snooze(&self, id: &str, until: i64) -> AppResult<()> {
        self.store(|ws| ws.reminder_snooze(id, until))?;
        self.reminders_changed();
        Ok(())
    }

    pub async fn reminder_reschedule(&self, id: &str, at: i64) -> AppResult<()> {
        self.store(|ws| ws.reminder_reschedule(id, at))?;
        self.reminders_changed();
        Ok(())
    }

    pub async fn reminder_set_note(&self, id: &str, note: &str) -> AppResult<()> {
        self.store(|ws| ws.reminder_set_note(id, note))?;
        self.reminders_changed();
        Ok(())
    }

    pub async fn reminder_delete(&self, id: &str) -> AppResult<()> {
        self.store(|ws| ws.reminder_delete(id))?;
        self.reminders_changed();
        Ok(())
    }

    /// Fires what is due at `now_ms` (milliseconds since the Unix epoch) on
    /// this device: records it as fired, emits `reminder:due` and returns it.
    /// The scheduler calls this with the clock; tests call it with a chosen
    /// time.
    pub fn reminders_check(&self, now_ms: i64) -> AppResult<Vec<ReminderEntryDto>> {
        let grace = i64::try_from(REMINDER_GRACE.as_millis()).unwrap_or(i64::MAX);
        let due: Vec<ReminderEntryDto> = self
            .store(|ws| ws.reminders_take_due(now_ms, grace))?
            .into_iter()
            .map(ReminderEntryDto::from)
            .collect();
        if !due.is_empty() {
            let native_shown = self.notify_due(&due);
            self.emit(CoreEvent::ReminderDue {
                reminders: due.clone(),
                native_shown,
            });
        }
        Ok(due)
    }

    /// Starts the reminder scheduler, and collects synced blocks that nothing
    /// has shown for a long time. Call once after the app opens.
    pub fn start_reminder_scheduler(&self) {
        let core = self.clone();
        self.spawn(self.inner.rt.spawn(async move {
            core.synced_collect_orphans();
            let mut last_version: Option<Vec<u8>> = None;
            let mut tick = tokio::time::interval(REMINDER_TICK);
            loop {
                tick.tick().await;
                // No workspace yet, or a transient storage error: the next
                // tick tries again.
                let _ = core.reminders_check(now_ms());
                if let Ok(version) = core.store(|ws| ws.reminders_version()) {
                    // Another device changed the reminders since the last look.
                    if last_version.as_ref().is_some_and(|last| *last != version) {
                        core.emit(CoreEvent::RemindersChanged);
                    }
                    last_version = Some(version);
                }
            }
        }));
    }
}
