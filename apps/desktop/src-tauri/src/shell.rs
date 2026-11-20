//! The Tauri side of the services `AppCore` needs: events, clipboard, native
//! dialogs and the global hold-to-talk shortcut.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

use crate::app::{Clipboard, CoreEvent, Dialogs, EventSink, Hotkeys};

pub struct TauriEvents(pub AppHandle);

impl EventSink for TauriEvents {
    fn emit(&self, event: CoreEvent) {
        // The window may be closing; a lost event then has no reader anyway.
        let _ = self.0.emit(event.name(), event.payload());
    }
}

pub struct TauriClipboard(pub AppHandle);

impl Clipboard for TauriClipboard {
    fn read_text(&self) -> Option<String> {
        self.0.clipboard().read_text().ok()
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.0
            .clipboard()
            .write_text(text.to_owned())
            .map_err(|err| err.to_string())
    }
}

pub struct TauriDialogs(pub AppHandle);

impl Dialogs for TauriDialogs {
    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        self.0
            .dialog()
            .file()
            .set_title(title)
            .blocking_pick_folder()
            .and_then(|path| path.into_path().ok())
    }

    fn pick_file(&self, title: &str, extensions: &[&str]) -> Option<PathBuf> {
        let mut dialog = self.0.dialog().file().set_title(title);
        if !extensions.is_empty() {
            dialog = dialog.add_filter("Files", extensions);
        }
        dialog
            .blocking_pick_file()
            .and_then(|path| path.into_path().ok())
    }
}

/// Registers the hold-to-talk shortcut; press and release both become events.
pub struct TauriHotkeys {
    pub app: AppHandle,
    pub events: Arc<dyn EventSink>,
}

impl Hotkeys for TauriHotkeys {
    #[cfg(desktop)]
    fn register(&self, accelerator: &str) -> Result<(), String> {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

        let shortcuts = self.app.global_shortcut();
        shortcuts.unregister_all().map_err(|err| err.to_string())?;
        let events = self.events.clone();
        shortcuts
            .on_shortcut(accelerator, move |_, _, event| {
                events.emit(CoreEvent::VoiceHotkey {
                    pressed: event.state() == ShortcutState::Pressed,
                });
            })
            .map_err(|err| err.to_string())
    }

    #[cfg(not(desktop))]
    fn register(&self, _accelerator: &str) -> Result<(), String> {
        Err("global shortcuts are not available on this platform".into())
    }
}
