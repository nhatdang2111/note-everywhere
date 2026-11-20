//! Tauri shell: wires the Rust core to the webview UI.
//!
//! All behaviour lives in [`app`], which knows nothing about Tauri; this file
//! registers the commands, the attachment URI scheme and the plugins.
//!
//! Plugins and what they are for: dialog (folder and file pickers),
//! clipboard-manager, global-shortcut (hold-to-talk), single-instance,
//! notification (due reminders) and updater (only when the build carries an
//! updater public key, see [`updater`]). All of them are driven from Rust, so
//! the webview's capability grants none of their permissions.

pub mod app;
mod commands;
mod shell;
#[cfg(desktop)]
pub mod updater;

use std::borrow::Cow;
use std::sync::Arc;

use tauri::http::Response;
use tauri::{Manager, RunEvent};

use app::{AppConfig, AppCore, Deps, EventSink, SharedBackend};
use ne_platform::dirs::AppDirs;
use ne_platform::secure_store::KeyringBackend;

fn build_core(
    app: &tauri::AppHandle,
    updater_configured: bool,
) -> Result<AppCore, Box<dyn std::error::Error>> {
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
        notifier: Arc::new(shell::TauriNotifier(app.clone())),
        updater: shell::updater_backend(app, updater_configured),
        secrets: SharedBackend(Arc::new(KeyringBackend)),
    };
    let core = tauri::async_runtime::block_on(AppCore::open(AppConfig::for_dirs(&dirs), deps))?;
    core.start_background();
    core.start_reminder_scheduler();
    core.start_update_check();
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
    let context = tauri::generate_context!();
    #[cfg_attr(not(desktop), allow(unused_variables))]
    let updater_configured = updater::updater_configured(context.config());
    builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init());
    // The plugin refuses to start without a public key in its configuration,
    // so a build without one leaves it out and reports updates as unavailable.
    #[cfg(desktop)]
    if updater_configured {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
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
        .setup(move |app| {
            let core = build_core(app.handle(), updater_configured)?;
            app.manage(core);
            Ok(())
        })
        .invoke_handler(command_handler())
        .build(context)
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
        commands::settings_set,
        commands::db_create,
        commands::db_schema,
        commands::db_query,
        commands::db_property_add,
        commands::db_property_rename,
        commands::db_property_delete,
        commands::db_property_move,
        commands::db_property_change_type,
        commands::db_relation_add,
        commands::db_option_add,
        commands::db_option_rename,
        commands::db_option_color,
        commands::db_option_delete,
        commands::db_option_move,
        commands::db_row_create,
        commands::db_cell_set,
        commands::db_card_move,
        commands::db_view_add,
        commands::db_view_save,
        commands::db_view_delete,
        commands::db_view_move,
        commands::template_list,
        commands::template_create_from_page,
        commands::template_create_for_database,
        commands::template_rename,
        commands::template_set_defaults,
        commands::template_delete,
        commands::template_instantiate_page,
        commands::template_instantiate_row,
        commands::page_history,
        commands::page_version_preview,
        commands::page_version_restore,
        commands::import_structured,
        commands::synced_create,
        commands::synced_insert,
        commands::synced_open,
        commands::synced_apply_update,
        commands::synced_close,
        commands::synced_unsync,
        commands::synced_occurrences,
        commands::synced_list,
        commands::reminder_add,
        commands::reminder_list,
        commands::reminder_set_done,
        commands::reminder_snooze,
        commands::reminder_reschedule,
        commands::reminder_set_note,
        commands::reminder_delete,
        commands::update_status,
        commands::update_check,
        commands::update_download,
        commands::update_install
    ]
}
