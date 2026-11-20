//! Events the core pushes to the UI, and the sink they go through.
//!
//! The Tauri shell implements [`EventSink`] with `emit`; tests capture.

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde_json::{Value, json};

use super::types::{ReminderEntryDto, SessionStatus, SyncStatus};
use super::update::UpdateStatus;

#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    TreeChanged {
        node_ids: Vec<String>,
    },
    PageRemoteUpdate {
        page_id: String,
        update: Vec<u8>,
    },
    SyncStatus(SyncStatus),
    SessionChanged(SessionStatus),
    VaultState {
        state: &'static str,
    },
    VoiceDownloadProgress {
        name: String,
        received_bytes: u64,
        total_bytes: u64,
    },
    VoiceHotkey {
        pressed: bool,
    },
    DatabaseChanged {
        database_id: String,
        row_ids: Vec<String>,
    },
    RemindersChanged,
    ReminderDue {
        reminders: Vec<ReminderEntryDto>,
        /// A native notification was shown for every one of them.
        native_shown: bool,
    },
    UpdateStatus(UpdateStatus),
    UpdateProgress {
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
}

impl CoreEvent {
    /// The Tauri event name, as listed in the contract's `CoreEventMap`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TreeChanged { .. } => "tree:changed",
            Self::PageRemoteUpdate { .. } => "page:remote-update",
            Self::SyncStatus(_) => "sync:status",
            Self::SessionChanged(_) => "session:changed",
            Self::VaultState { .. } => "vault:state",
            Self::VoiceDownloadProgress { .. } => "voice:download-progress",
            Self::VoiceHotkey { .. } => "voice:hotkey",
            Self::DatabaseChanged { .. } => "database:changed",
            Self::RemindersChanged => "reminders:changed",
            Self::ReminderDue { .. } => "reminder:due",
            Self::UpdateStatus(_) => "update:status",
            Self::UpdateProgress { .. } => "update:progress",
        }
    }

    /// The payload as camelCase JSON.
    pub fn payload(&self) -> Value {
        match self {
            Self::TreeChanged { node_ids } => json!({ "nodeIds": node_ids }),
            Self::PageRemoteUpdate { page_id, update } => {
                json!({ "pageId": page_id, "update": update })
            }
            Self::SyncStatus(status) => json!(status),
            Self::SessionChanged(status) => json!(status),
            Self::VaultState { state } => json!({ "state": state }),
            Self::VoiceDownloadProgress {
                name,
                received_bytes,
                total_bytes,
            } => {
                json!({ "name": name, "receivedBytes": received_bytes, "totalBytes": total_bytes })
            }
            Self::VoiceHotkey { pressed } => json!({ "pressed": pressed }),
            Self::DatabaseChanged {
                database_id,
                row_ids,
            } => json!({ "databaseId": database_id, "rowIds": row_ids }),
            Self::RemindersChanged => json!({}),
            Self::ReminderDue {
                reminders,
                native_shown,
            } => json!({ "reminders": reminders, "nativeShown": native_shown }),
            Self::UpdateStatus(status) => json!(status),
            Self::UpdateProgress {
                received_bytes,
                total_bytes,
            } => json!({ "receivedBytes": received_bytes, "totalBytes": total_bytes }),
        }
    }
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: CoreEvent);
}

/// Keeps every event, for tests.
#[derive(Default)]
pub struct CapturingSink {
    events: Mutex<Vec<CoreEvent>>,
}

impl CapturingSink {
    pub fn all(&self) -> Vec<CoreEvent> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    pub fn named(&self, name: &str) -> Vec<CoreEvent> {
        self.all()
            .into_iter()
            .filter(|e| e.name() == name)
            .collect()
    }

    pub fn names(&self) -> BTreeSet<&'static str> {
        self.all().iter().map(CoreEvent::name).collect()
    }

    pub fn clear(&self) {
        if let Ok(mut events) = self.events.lock() {
            events.clear();
        }
    }
}

impl EventSink for CapturingSink {
    fn emit(&self, event: CoreEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}
