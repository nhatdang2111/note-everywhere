//! Everything the desktop app does, independent of the UI framework.
//!
//! [`AppCore`] owns the open [`ne_store::Workspace`], the session, the sync
//! engine, the vault, voice state and settings. Its async methods mirror the
//! UI contract (`src/api/contract.ts`); the Tauri commands in the shell are
//! thin wrappers around them.

mod attachments;
mod blob_index;
pub mod db_api;
pub mod deps;
pub mod error;
pub mod events;
mod history_api;
mod protocol;
pub mod reminder_api;
mod session;
mod settings;
mod sync;
mod sync_store;
pub mod synced_api;
mod template_api;
pub mod types;
pub mod update;
mod vault;
mod vault_persist;
mod voice;
mod workspace_api;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ne_core::NodeId;
use ne_crypto::password::{KdfParams, KdfPolicy};
use ne_platform::dirs::AppDirs;
use ne_platform::secure_store::DEFAULT_SERVICE;
use ne_store::{Durability, StoreConfig, Workspace};
use ne_vault::Vault;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub use deps::{
    Clipboard, Deps, Dialogs, Hotkeys, MemoryBackend, MemoryClipboard, Notifier, NotifyPermission,
    SharedBackend,
};
pub use error::{AppError, AppResult};
pub use events::{CapturingSink, CoreEvent, EventSink};
pub use protocol::{AttachmentResponse, SCHEME as ATTACHMENT_SCHEME};
pub use sync::EngineTuning;
pub use update::{
    BoxFuture, NoUpdates, ProgressFn, UpdateBackend, UpdateFailure, UpdateOffer, UpdateState,
    UpdateStatus,
};

use types::{SessionState, Settings, SyncStatus};

/// Directory name of the workspace inside the data directory.
const WORKSPACE_DIR: &str = "workspace";

#[derive(Clone)]
pub struct AppConfig {
    /// Holds the `workspace` directory (database and attachments).
    pub data_dir: PathBuf,
    pub models_dir: PathBuf,
    pub kdf_params: KdfParams,
    pub kdf_policy: KdfPolicy,
    pub vault_clock: Arc<dyn ne_vault::Clock>,
    /// How long after a password entry the vault reveals secrets without
    /// asking again.
    pub reauth_grace: Duration,
    pub clipboard_clear_after: Duration,
    /// Prefix of the URLs returned for attachments, without a trailing slash.
    pub attachment_url_base: String,
    /// How long `sync_now` waits for the engine to settle.
    pub sync_wait: Duration,
    pub durability: Durability,
    pub secret_service: String,
    pub engine_tuning: Option<EngineTuning>,
}

impl AppConfig {
    pub fn for_dirs(dirs: &AppDirs) -> Self {
        Self::new(dirs.data().to_owned(), dirs.models().to_owned())
    }

    pub fn new(data_dir: PathBuf, models_dir: PathBuf) -> Self {
        Self {
            data_dir,
            models_dir,
            kdf_params: KdfParams::DEFAULT,
            kdf_policy: KdfPolicy::PRODUCTION,
            vault_clock: Arc::new(ne_vault::SystemClock),
            reauth_grace: Duration::from_secs(60),
            clipboard_clear_after: Duration::from_secs(30),
            attachment_url_base: protocol::default_url_base(),
            sync_wait: Duration::from_secs(20),
            durability: Durability::Full,
            secret_service: DEFAULT_SERVICE.to_owned(),
            engine_tuning: None,
        }
    }

    fn workspace_dir(&self) -> PathBuf {
        self.data_dir.join(WORKSPACE_DIR)
    }

    fn store_config(&self) -> StoreConfig {
        StoreConfig {
            durability: self.durability,
            ..StoreConfig::default()
        }
    }
}

pub(crate) type SharedWorkspace = Arc<Mutex<Option<Workspace>>>;
pub(crate) type SharedVault = Arc<Mutex<Option<Vault>>>;
pub(crate) type OpenPages = Arc<Mutex<HashSet<NodeId>>>;

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) struct SessionData {
    pub state: SessionState,
    pub username: Option<String>,
    pub server_url: Option<String>,
}

pub(crate) struct Inner {
    pub cfg: AppConfig,
    pub deps: Deps,
    pub rt: Handle,
    pub ws: SharedWorkspace,
    pub vault: SharedVault,
    pub open_pages: OpenPages,
    pub session: Mutex<SessionData>,
    pub settings: Mutex<Settings>,
    pub runtime: Mutex<Option<Arc<sync::SyncRuntime>>>,
    pub last_status: Mutex<Option<SyncStatus>>,
    pub last_synced_at: Mutex<Option<i64>>,
    pub maintenance_busy: AtomicBool,
    pub clipboard_task: Mutex<Option<JoinHandle<()>>>,
    pub idle_after: watch::Sender<Option<Duration>>,
    #[cfg_attr(not(feature = "transcribe"), allow(dead_code))]
    pub voice: voice::VoiceState,
    pub tasks: Mutex<Vec<JoinHandle<()>>>,
    pub session_gate: tokio::sync::Mutex<()>,
    pub last_compaction: Mutex<Option<std::time::Instant>>,
    pub update: Mutex<update::UpdateData>,
}

/// Cheap to clone; every clone is the same application.
#[derive(Clone)]
pub struct AppCore {
    pub(crate) inner: Arc<Inner>,
}

impl AppCore {
    /// Opens the workspace in the data directory if there is one, restores the
    /// stored session and starts syncing. Must run inside a tokio runtime.
    pub async fn open(cfg: AppConfig, deps: Deps) -> AppResult<Self> {
        let (idle_after, _) = watch::channel(None);
        let update = update::UpdateData::new(
            deps.updater.configured(),
            update::read_last_check(&cfg.data_dir),
        );
        let core = Self {
            inner: Arc::new(Inner {
                cfg,
                deps,
                rt: Handle::current(),
                ws: Arc::new(Mutex::new(None)),
                vault: Arc::new(Mutex::new(None)),
                open_pages: Arc::new(Mutex::new(HashSet::new())),
                session: Mutex::new(SessionData {
                    state: SessionState::Empty,
                    username: None,
                    server_url: None,
                }),
                settings: Mutex::new(Settings::default()),
                runtime: Mutex::new(None),
                last_status: Mutex::new(None),
                last_synced_at: Mutex::new(None),
                maintenance_busy: AtomicBool::new(false),
                clipboard_task: Mutex::new(None),
                idle_after,
                voice: voice::VoiceState::default(),
                tasks: Mutex::new(Vec::new()),
                session_gate: tokio::sync::Mutex::new(()),
                last_compaction: Mutex::new(None),
                update: Mutex::new(update),
            }),
        };
        let dir = core.inner.cfg.workspace_dir();
        match Workspace::open(&dir, core.inner.cfg.store_config()) {
            Ok(ws) => {
                *lock(&core.inner.ws) = Some(ws);
                core.restore().await?;
            }
            Err(ne_store::Error::NoWorkspace(_)) => {}
            Err(err) => return Err(err.into()),
        }
        Ok(core)
    }

    /// Stops background work and releases the database. Call before the
    /// process exits or when a test simulates closing the app.
    pub async fn shutdown(&self) {
        self.stop_runtime().await;
        for task in lock(&self.inner.tasks).drain(..) {
            task.abort();
        }
        if let Some(task) = lock(&self.inner.clipboard_task).take() {
            task.abort();
        }
        self.voice_shutdown();
        lock(&self.inner.vault).take();
        lock(&self.inner.ws).take();
    }

    pub fn app_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    pub(crate) fn emit(&self, event: CoreEvent) {
        self.inner.deps.events.emit(event);
    }

    /// Runs `f` on the open workspace; fails with `no_workspace` if none is open.
    pub(crate) fn with_ws<R>(
        &self,
        f: impl FnOnce(&mut Workspace) -> AppResult<R>,
    ) -> AppResult<R> {
        let mut guard = lock(&self.inner.ws);
        let ws = guard.as_mut().ok_or_else(|| {
            AppError::new("no_workspace", "There is no workspace on this device yet.")
        })?;
        f(ws)
    }

    /// Like [`Self::with_ws`] for store calls, converting their errors.
    pub(crate) fn store<R>(
        &self,
        f: impl FnOnce(&mut Workspace) -> ne_store::Result<R>,
    ) -> AppResult<R> {
        self.with_ws(|ws| f(ws).map_err(AppError::from))
    }

    /// Runs blocking work (disk, Argon2, pickers) off the async threads.
    pub(crate) async fn blocking<R: Send + 'static>(
        &self,
        job: impl FnOnce(AppCore) -> AppResult<R> + Send + 'static,
    ) -> AppResult<R> {
        let core = self.clone();
        tokio::task::spawn_blocking(move || job(core))
            .await
            .map_err(|_| AppError::internal("A background task failed."))?
    }

    pub(crate) fn spawn(&self, task: JoinHandle<()>) {
        let mut tasks = lock(&self.inner.tasks);
        tasks.retain(|t| !t.is_finished());
        tasks.push(task);
    }
}

pub(crate) fn node_id(text: &str) -> AppResult<NodeId> {
    text.parse()
        .map_err(|_| AppError::not_found("That page or folder"))
}
