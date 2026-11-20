//! Data shapes of the UI contract (`src/api/contract.ts`), as camelCase JSON.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Empty,
    Local,
    SignedIn,
    NeedsSignIn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub state: SessionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignUpArgs {
    pub server_url: String,
    pub setup_code: String,
    pub username: String,
    pub password: String,
    pub device_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInArgs {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub device_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverAccountArgs {
    pub server_url: String,
    pub username: String,
    pub recovery_key: String,
    pub new_password: String,
    pub device_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSession {
    pub id: String,
    pub device_label: String,
    pub created_at: u64,
    pub last_seen_at: u64,
    pub current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Folder,
    Page,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: NodeKind,
    pub title: String,
    pub icon: Option<String>,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    #[serde(flatten)]
    pub node: TreeNode,
    pub trashed_at: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNodeArgs {
    pub kind: NodeKind,
    pub parent_id: Option<String>,
    pub index: Option<usize>,
    pub title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveNodeArgs {
    pub id: String,
    pub parent_id: Option<String>,
    pub index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenPage {
    pub snapshot: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub page_id: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page_id: String,
    pub title: String,
    pub breadcrumb: Vec<String>,
    pub snippet: String,
    pub highlights: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedAttachment {
    pub hash: String,
    pub mime: String,
    pub byte_length: u64,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Disabled,
    Synced,
    Pending,
    Offline,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: SyncState,
    pub pending_updates: usize,
    pub last_synced_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultItemInput {
    pub id: Option<String>,
    pub kind: String,
    pub title: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub value: Option<String>,
    pub urls: Option<Vec<String>>,
    pub notes: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultItemSummary {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub username: Option<String>,
    pub urls: Vec<String>,
    pub tags: Vec<String>,
    pub updated_at: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePasswordArgs {
    pub mode: String,
    pub length: usize,
    pub lowercase: Option<bool>,
    pub uppercase: Option<bool>,
    pub digits: Option<bool>,
    pub symbols: Option<bool>,
    pub exclude_ambiguous: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPassword {
    pub value: String,
    pub entropy_bits: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedRow {
    pub row: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VaultImportReport {
    pub imported: usize,
    pub skipped: Vec<SkippedRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceModel {
    pub name: String,
    pub byte_length: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub available: bool,
    pub recording: bool,
    pub active_model: Option<String>,
    pub models: Vec<VoiceModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoiceText {
    pub text: String,
    /// The kept recording when "keep audio" is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AddedAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub vault_idle_lock_seconds: u64,
    pub voice_hotkey: String,
    pub voice_language: String,
    pub voice_keep_audio: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            vault_idle_lock_seconds: 300,
            voice_hotkey: "CommandOrControl+Shift+Space".into(),
            voice_language: "auto".into(),
            voice_keep_audio: false,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub theme: Option<String>,
    pub vault_idle_lock_seconds: Option<u64>,
    pub voice_hotkey: Option<String>,
    pub voice_language: Option<String>,
    pub voice_keep_audio: Option<bool>,
}

/// A reminder with what a list or a notification needs next to it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderEntryDto {
    pub reminder: ne_core::reminders::Reminder,
    pub due_at: i64,
    pub page_title: String,
    pub page_visible: bool,
}

impl From<ne_store::ReminderEntry> for ReminderEntryDto {
    fn from(entry: ne_store::ReminderEntry) -> Self {
        Self {
            reminder: entry.reminder,
            due_at: entry.due_at,
            page_title: entry.page_title,
            page_visible: entry.page_visible,
        }
    }
}
