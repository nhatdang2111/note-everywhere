//! The OS-facing services `AppCore` needs, as traits so tests can substitute
//! in-memory versions and the Tauri shell can provide the real ones.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ne_platform::secure_store::{CredentialError, SecretBackend};
use zeroize::Zeroizing;

use super::events::EventSink;
use super::update::UpdateBackend;

/// The system clipboard (text only).
pub trait Clipboard: Send + Sync {
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: &str) -> Result<(), String>;
}

/// Native file and folder pickers. Blocking; `AppCore` calls them off the
/// async threads. `None` means the person cancelled.
pub trait Dialogs: Send + Sync {
    fn pick_folder(&self, title: &str) -> Option<PathBuf>;
    /// `extensions` are lowercase without the dot; empty means any file.
    fn pick_file(&self, title: &str, extensions: &[&str]) -> Option<PathBuf>;
}

/// Registers the global hold-to-talk shortcut.
pub trait Hotkeys: Send + Sync {
    /// Replaces any previous registration. `Err` carries a reason that is safe
    /// to show.
    fn register(&self, accelerator: &str) -> Result<(), String>;
}

/// Whether the system lets the app show notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyPermission {
    Granted,
    Denied,
    /// The person has not been asked yet.
    Undetermined,
}

/// Native notifications.
pub trait Notifier: Send + Sync {
    /// The current permission; never prompts.
    fn permission(&self) -> NotifyPermission;
    /// Asks the system for permission (it may prompt) and returns the answer.
    /// Blocking; `AppCore` calls it off the async threads.
    fn request_permission(&self) -> NotifyPermission;
    /// Shows one notification. `Err` carries a reason that is safe to log.
    fn show(&self, title: &str, body: &str) -> Result<(), String>;
}

/// A secure store the core can hand to `ne-platform`.
#[derive(Clone)]
pub struct SharedBackend(pub Arc<dyn SecretBackend + Send + Sync>);

impl SecretBackend for SharedBackend {
    fn set(&self, service: &str, user: &str, secret: &[u8]) -> Result<(), CredentialError> {
        self.0.set(service, user, secret)
    }

    fn get(
        &self,
        service: &str,
        user: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError> {
        self.0.get(service, user)
    }

    fn delete(&self, service: &str, user: &str) -> Result<bool, CredentialError> {
        self.0.delete(service, user)
    }
}

pub struct Deps {
    pub events: Arc<dyn EventSink>,
    pub clipboard: Arc<dyn Clipboard>,
    pub dialogs: Arc<dyn Dialogs>,
    pub hotkeys: Arc<dyn Hotkeys>,
    pub notifier: Arc<dyn Notifier>,
    pub updater: Arc<dyn UpdateBackend>,
    pub secrets: SharedBackend,
}

/// A plain in-memory secret store for tests and previews.
#[derive(Default)]
pub struct MemoryBackend(Mutex<std::collections::HashMap<(String, String), Vec<u8>>>);

impl SecretBackend for MemoryBackend {
    fn set(&self, service: &str, user: &str, secret: &[u8]) -> Result<(), CredentialError> {
        self.0
            .lock()
            .map_err(|_| CredentialError::Unavailable)?
            .insert((service.into(), user.into()), secret.to_vec());
        Ok(())
    }

    fn get(
        &self,
        service: &str,
        user: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| CredentialError::Unavailable)?
            .get(&(service.to_owned(), user.to_owned()))
            .cloned()
            .map(Zeroizing::new))
    }

    fn delete(&self, service: &str, user: &str) -> Result<bool, CredentialError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| CredentialError::Unavailable)?
            .remove(&(service.to_owned(), user.to_owned()))
            .is_some())
    }
}

/// An in-memory clipboard for tests.
#[derive(Default)]
pub struct MemoryClipboard(pub Mutex<Option<String>>);

impl Clipboard for MemoryClipboard {
    fn read_text(&self) -> Option<String> {
        self.0.lock().ok().and_then(|c| c.clone())
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        *self.0.lock().map_err(|_| "poisoned".to_owned())? = Some(text.to_owned());
        Ok(())
    }
}
