//! Tauri shell: wires the Rust core to the webview UI.
//!
//! All behaviour lives in [`app`], which knows nothing about Tauri; this file
//! registers the commands, the attachment URI scheme and the plugins.

pub mod app;
mod commands;
mod shell;

use std::borrow::Cow;
use std::sync::Arc;

use tauri::http::Response;
use tauri::{Manager, RunEvent};

use app::{AppConfig, AppCore, Deps, EventSink, SharedBackend};
use ne_platform::dirs::AppDirs;
use ne_platform::secure_store::KeyringBackend;

fn build_core(app: &tauri::AppHandle) -> Result<AppCore, Box<dyn std::error::Error>> {
    let dirs = AppDirs::discover()?;
    let events: Arc<dyn EventSink> = Arc::new(shell::TauriEvents(app.clone()));
    let deps = Deps {
        events: events.clone(),
        clipboard: Arc::new(shell::TauriClipboard(app.clone())),
        dialogs: Arc::new(shell::TauriDialogs(app.clone())),
        hotkeys: Arc::new(shell::TauriHotkeys {
            app: app.clone(),
            events,
        }),
        secrets: SharedBackend(Arc::new(KeyringBackend)),
    };
    let core = tauri::async_runtime::block_on(AppCore::open(AppConfig::for_dirs(&dirs), deps))?;
    core.start_background();
    Ok(core)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    // Registered first: a second launch must be caught before anything opens
    // the database, so two processes never share it.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));
    }
    builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }
    builder
        .register_asynchronous_uri_scheme_protocol(
            app::ATTACHMENT_SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let reply = match app.try_state::<AppCore>() {
                        Some(core) => {
                            let range = request
                                .headers()
                                .get("range")
                                .and_then(|value| value.to_str().ok());
                            core.serve_attachment(request.uri().path(), range).await
                        }
                        None => app::AttachmentResponse::unavailable(),
                    };
                    let mut response = Response::builder().status(reply.status);
                    for (name, value) in &reply.headers {
                        response = response.header(*name, value.as_str());
                    }
                    let built = response
                        .body(Cow::Owned(reply.body))
                        .unwrap_or_else(|_| Response::new(Cow::Owned(Vec::new())));
                    responder.respond(built);
                });
            },
        )
        .setup(|app| {
            let core = build_core(app.handle())?;
            app.manage(core);
            Ok(())
        })
        .invoke_handler(command_handler())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event
                && let Some(core) = app.try_state::<AppCore>()
            {
                let core = core.inner().clone();
                tauri::async_runtime::block_on(async move { core.shutdown().await });
            }
        });
}

/// Every command of the UI contract. `tests/contract_conformance.rs` reads this
/// list, so a command must be added here to count as implemented.
pub fn command_handler<R: tauri::Runtime>()
-> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::app_version,
        commands::session_status,
        commands::workspace_create_local,
        commands::sign_up,
        commands::sign_in,
        commands::recover_account,
        commands::sign_out,
        commands::change_password,
        commands::sessions_list,
        commands::sessions_revoke,
        commands::tree_snapshot,
        commands::tree_create,
        commands::tree_rename,
        commands::tree_set_icon,
        commands::tree_set_favorite,
        commands::tree_move,
        commands::tree_trash,
        commands::tree_restore,
        commands::tree_delete_permanently,
        commands::tree_list_trash,
        commands::tree_empty_trash,
        commands::page_open,
        commands::page_apply_update,
        commands::page_close,
        commands::page_backlinks,
        commands::search_query,
        commands::attachment_add_bytes,
        commands::attachment_pick,
        commands::attachment_url,
        commands::export_markdown,
        commands::import_markdown,
        commands::sync_status,
        commands::sync_now,
        commands::vault_state,
        commands::vault_create,
        commands::vault_unlock,
        commands::vault_recover,
        commands::vault_lock,
        commands::vault_change_password,
        commands::vault_list,
        commands::vault_save,
        commands::vault_delete,
        commands::vault_reveal,
        commands::vault_copy,
        commands::vault_generate_password,
        commands::vault_import_csv,
        commands::voice_status,
        commands::voice_download_model,
        commands::voice_delete_model,
        commands::voice_select_model,
        commands::voice_start,
        commands::voice_stop,
        commands::voice_cancel,
        commands::settings_get,
        commands::settings_set
    ]
}
