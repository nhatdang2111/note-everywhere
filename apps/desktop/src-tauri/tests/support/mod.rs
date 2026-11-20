//! A simulated device: an `AppCore` over a temporary data directory with
//! in-memory stand-ins for the keychain, clipboard and file pickers.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ne_crypto::password::{KdfParams, KdfPolicy};
use ne_store::Durability;
use ne_sync::Backoff;
use note_everywhere_desktop_lib::app::{
    AppConfig, AppCore, CapturingSink, Clipboard, Deps, Dialogs, EngineTuning, Hotkeys,
    MemoryBackend, MemoryClipboard, SharedBackend,
};
use tempfile::TempDir;

pub mod proxy;

pub const SERVER_ENV: &str = "NE_SYNC_TEST_URL";
pub const PASSWORD: &str = "correct horse battery staple";
pub const USERNAME: &str = "alice@example.test";

/// Cheap Argon2 settings so tests stay fast; real accounts use the defaults.
pub const TEST_KDF: KdfParams = KdfParams {
    memory_kib: 8,
    iterations: 1,
    parallelism: 1,
};

#[derive(Default)]
pub struct ScriptedDialogs {
    folders: Mutex<VecDeque<Option<PathBuf>>>,
    files: Mutex<VecDeque<Option<PathBuf>>>,
}

impl ScriptedDialogs {
    pub fn queue_folder(&self, path: Option<PathBuf>) {
        self.folders.lock().expect("lock").push_back(path);
    }

    pub fn queue_file(&self, path: Option<PathBuf>) {
        self.files.lock().expect("lock").push_back(path);
    }
}

impl Dialogs for ScriptedDialogs {
    fn pick_folder(&self, _title: &str) -> Option<PathBuf> {
        self.folders.lock().expect("lock").pop_front().flatten()
    }

    fn pick_file(&self, _title: &str, _extensions: &[&str]) -> Option<PathBuf> {
        self.files.lock().expect("lock").pop_front().flatten()
    }
}

#[derive(Default)]
pub struct RecordingHotkeys(pub Mutex<Vec<String>>);

impl Hotkeys for RecordingHotkeys {
    fn register(&self, accelerator: &str) -> Result<(), String> {
        if accelerator.contains("bogus") {
            return Err("not a shortcut".into());
        }
        self.0.lock().expect("lock").push(accelerator.to_owned());
        Ok(())
    }
}

/// A vault clock the test moves by hand.
#[derive(Default)]
pub struct ManualClock(pub Mutex<i64>);

impl ne_vault::Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        *self.0.lock().expect("lock")
    }
}

impl ManualClock {
    pub fn advance(&self, by: Duration) {
        *self.0.lock().expect("lock") += i64::try_from(by.as_millis()).expect("fits");
    }
}

pub struct Device {
    pub core: AppCore,
    pub events: Arc<CapturingSink>,
    pub clipboard: Arc<MemoryClipboard>,
    pub dialogs: Arc<ScriptedDialogs>,
    pub hotkeys: Arc<RecordingHotkeys>,
    pub secrets: Arc<MemoryBackend>,
    pub clock: Arc<ManualClock>,
    pub dir: Arc<TempDir>,
}

pub fn test_config(dir: &TempDir, clock: Arc<ManualClock>) -> AppConfig {
    let mut cfg = AppConfig::new(dir.path().join("data"), dir.path().join("models"));
    cfg.kdf_params = TEST_KDF;
    cfg.kdf_policy = KdfPolicy::LENIENT;
    cfg.vault_clock = clock;
    cfg.durability = Durability::Normal;
    cfg.clipboard_clear_after = Duration::from_millis(300);
    cfg.sync_wait = Duration::from_secs(20);
    cfg.attachment_url_base = "ne-attachment://localhost".into();
    cfg.engine_tuning = Some(EngineTuning {
        batch_delay: Duration::from_millis(40),
        backoff: Backoff {
            base: Duration::from_millis(50),
            max: Duration::from_millis(400),
        },
    });
    cfg
}

impl Device {
    pub async fn new() -> Self {
        Self::open_in(
            Arc::new(TempDir::new().expect("tempdir")),
            Arc::new(MemoryBackend::default()),
        )
        .await
    }

    pub async fn open_in(dir: Arc<TempDir>, secrets: Arc<MemoryBackend>) -> Self {
        let events = Arc::new(CapturingSink::default());
        let clipboard = Arc::new(MemoryClipboard::default());
        let dialogs = Arc::new(ScriptedDialogs::default());
        let hotkeys = Arc::new(RecordingHotkeys::default());
        let clock = Arc::new(ManualClock::default());
        let deps = Deps {
            events: events.clone(),
            clipboard: clipboard.clone() as Arc<dyn Clipboard>,
            dialogs: dialogs.clone(),
            hotkeys: hotkeys.clone(),
            secrets: SharedBackend(secrets.clone()),
        };
        let core = AppCore::open(test_config(&dir, clock.clone()), deps)
            .await
            .expect("open");
        Self {
            core,
            events,
            clipboard,
            dialogs,
            hotkeys,
            secrets,
            clock,
            dir,
        }
    }

    /// Closes the app and starts it again on the same data and keychain.
    pub async fn restart(self) -> Self {
        self.core.shutdown().await;
        let (dir, secrets) = (self.dir.clone(), self.secrets.clone());
        drop(self);
        Self::open_in(dir, secrets).await
    }

    pub async fn local() -> Self {
        let device = Self::new().await;
        device
            .core
            .workspace_create_local()
            .await
            .expect("create workspace");
        device
    }
}
