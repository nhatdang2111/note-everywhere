//! Tauri commands: one per method of the UI contract, named in snake_case.
//!
//! Each is a thin wrapper. The UI sends the contract's argument object as the
//! whole invoke payload, so a command reads its fields from the raw request
//! body instead of declaring one parameter per field.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::State;
use tauri::ipc::{InvokeBody, Request};

use crate::app::types::{
    AddedAttachment, Backlink, CreateNodeArgs, DeviceSession, GeneratePasswordArgs,
    GeneratedPassword, MoveNodeArgs, OpenPage, RecoverAccountArgs, SearchHit, SessionStatus,
    Settings, SettingsPatch, SignInArgs, SignUpArgs, SyncStatus, TrashEntry, TreeNode,
    VaultImportReport, VaultItemInput, VaultItemSummary, VoiceStatus, VoiceText,
};
use crate::app::{AppCore, AppError};

fn parse<T: DeserializeOwned>(request: &Request<'_>) -> Result<T, AppError> {
    match request.body() {
        InvokeBody::Json(value) => serde_json::from_value(value.clone())
            .map_err(|_| AppError::invalid("The command arguments are malformed.")),
        InvokeBody::Raw(_) => Err(AppError::invalid("The command arguments are malformed.")),
    }
}

macro_rules! command {
    // No arguments.
    ($name:ident -> $ret:ty, |$core:ident| $body:expr) => {
        #[tauri::command]
        pub async fn $name(state: State<'_, AppCore>) -> Result<$ret, AppError> {
            let $core = state.inner().clone();
            $body
        }
    };
    // Arguments are a contract type.
    ($name:ident ($args:ty) -> $ret:ty, |$core:ident, $a:ident| $body:expr) => {
        #[tauri::command]
        pub async fn $name(state: State<'_, AppCore>, request: Request<'_>) -> Result<$ret, AppError> {
            let $a: $args = parse(&request)?;
            let $core = state.inner().clone();
            $body
        }
    };
    // Arguments are the listed fields.
    ($name:ident { $($field:ident : $ty:ty),+ $(,)? } -> $ret:ty, |$core:ident, $a:ident| $body:expr) => {
        #[tauri::command]
        pub async fn $name(state: State<'_, AppCore>, request: Request<'_>) -> Result<$ret, AppError> {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Fields { $($field: $ty),+ }
            let $a: Fields = parse(&request)?;
            let $core = state.inner().clone();
            $body
        }
    };
}

command!(app_version -> String, |core| Ok(core.app_version()));

// Session
command!(session_status -> SessionStatus, |core| core.session_status().await);
command!(workspace_create_local -> (), |core| core.workspace_create_local().await);
command!(sign_up (SignUpArgs) -> Value, |core, a| {
    let recovery_key = core.sign_up(a).await?;
    Ok(json!({ "recoveryKey": recovery_key }))
});
command!(sign_in (SignInArgs) -> (), |core, a| core.sign_in(a).await);
command!(recover_account (RecoverAccountArgs) -> (), |core, a| core.recover_account(a).await);
command!(sign_out -> (), |core| core.sign_out().await);
command!(change_password { current_password: String, new_password: String } -> (), |core, a| {
    core.change_password(&a.current_password, &a.new_password).await
});
command!(sessions_list -> Vec<DeviceSession>, |core| core.sessions_list().await);
command!(sessions_revoke { id: String } -> (), |core, a| core.sessions_revoke(&a.id).await);

// Tree
command!(tree_snapshot -> Vec<TreeNode>, |core| core.tree_snapshot().await);
command!(tree_create (CreateNodeArgs) -> String, |core, a| core.tree_create(a).await);
command!(tree_rename { id: String, title: String } -> (), |core, a| core.tree_rename(&a.id, &a.title).await);
command!(tree_set_icon { id: String, icon: Option<String> } -> (), |core, a| {
    core.tree_set_icon(&a.id, a.icon.as_deref()).await
});
command!(tree_set_favorite { id: String, favorite: bool } -> (), |core, a| {
    core.tree_set_favorite(&a.id, a.favorite).await
});
command!(tree_move (MoveNodeArgs) -> (), |core, a| core.tree_move(a).await);
command!(tree_trash { id: String } -> (), |core, a| core.tree_trash(&a.id).await);
command!(tree_restore { id: String } -> (), |core, a| core.tree_restore(&a.id).await);
command!(tree_delete_permanently { id: String } -> (), |core, a| core.tree_delete_permanently(&a.id).await);
command!(tree_list_trash -> Vec<TrashEntry>, |core| core.tree_list_trash().await);
command!(tree_empty_trash -> (), |core| core.tree_empty_trash().await);

// Pages
command!(page_open { id: String } -> OpenPage, |core, a| core.page_open(&a.id).await);
command!(page_apply_update { id: String, update: Vec<u8> } -> (), |core, a| {
    core.page_apply_update(&a.id, &a.update).await
});
command!(page_close { id: String } -> (), |core, a| core.page_close(&a.id).await);
command!(page_backlinks { id: String } -> Vec<Backlink>, |core, a| core.page_backlinks(&a.id).await);

// Search
command!(search_query { text: String, limit: Option<usize> } -> Vec<SearchHit>, |core, a| {
    core.search_query(&a.text, a.limit).await
});

// Attachments
command!(attachment_add_bytes { bytes: Vec<u8>, mime: String, file_name: Option<String> } -> AddedAttachment, |core, a| {
    core.attachment_add_bytes(a.bytes, &a.mime, a.file_name.as_deref()).await
});
command!(attachment_pick { images_only: bool } -> Option<AddedAttachment>, |core, a| {
    core.attachment_pick(a.images_only).await
});
command!(attachment_url { hash: String } -> String, |core, a| core.attachment_url(&a.hash).await);

// Import and export
command!(export_markdown { id: Option<String> } -> Option<Value>, |core, a| {
    Ok(core.export_markdown(a.id.as_deref()).await?.map(|pages| json!({ "pagesWritten": pages })))
});
command!(import_markdown { parent_id: Option<String> } -> Option<Value>, |core, a| {
    Ok(core.import_markdown(a.parent_id.as_deref()).await?.map(|pages| json!({ "pagesCreated": pages })))
});

// Sync
command!(sync_status -> SyncStatus, |core| core.sync_status().await);
command!(sync_now -> (), |core| core.sync_now().await);

// Vault
command!(vault_state -> &'static str, |core| core.vault_state().await);
command!(vault_create { password: String } -> Value, |core, a| {
    let (recovery_key, same) = core.vault_create(a.password).await?;
    Ok(json!({ "recoveryKey": recovery_key, "sameAsAccountPassword": same }))
});
command!(vault_unlock { password: String } -> (), |core, a| core.vault_unlock(a.password).await);
command!(vault_recover { recovery_key: String, new_password: String } -> (), |core, a| {
    core.vault_recover(&a.recovery_key, a.new_password).await
});
command!(vault_lock -> (), |core| core.vault_lock().await);
command!(vault_change_password { current_password: String, new_password: String } -> (), |core, a| {
    core.vault_change_password(a.current_password, a.new_password).await
});
command!(vault_list { query: Option<String> } -> Vec<VaultItemSummary>, |core, a| {
    core.vault_list(a.query.as_deref()).await
});
command!(vault_save { item: VaultItemInput } -> VaultItemSummary, |core, a| core.vault_save(a.item).await);
command!(vault_delete { id: String } -> (), |core, a| core.vault_delete(&a.id).await);
command!(vault_reveal { id: String, field: String, password: Option<String> } -> String, |core, a| {
    core.vault_reveal(&a.id, &a.field, a.password).await
});
command!(vault_copy { id: String, field: String, password: Option<String> } -> Value, |core, a| {
    let seconds = core.vault_copy(&a.id, &a.field, a.password).await?;
    Ok(json!({ "clearAfterSeconds": seconds }))
});
command!(vault_generate_password (GeneratePasswordArgs) -> GeneratedPassword, |core, a| {
    core.vault_generate_password(a).await
});
command!(vault_import_csv { format: String } -> Option<VaultImportReport>, |core, a| {
    core.vault_import_csv(&a.format).await
});

// Voice
command!(voice_status -> VoiceStatus, |core| core.voice_status().await);
command!(voice_download_model { name: String } -> (), |core, a| core.voice_download_model(&a.name).await);
command!(voice_delete_model { name: String } -> (), |core, a| core.voice_delete_model(&a.name).await);
command!(voice_select_model { name: String } -> (), |core, a| core.voice_select_model(&a.name).await);
command!(voice_start -> (), |core| core.voice_start().await);
command!(voice_stop -> VoiceText, |core| core.voice_stop().await);
command!(voice_cancel -> (), |core| core.voice_cancel().await);

// Settings
command!(settings_get -> Settings, |core| core.settings_get().await);
command!(settings_set { settings: SettingsPatch } -> Settings, |core, a| core.settings_set(a.settings).await);
