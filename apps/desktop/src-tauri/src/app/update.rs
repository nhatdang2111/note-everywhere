//! Application updates.
//!
//! The core owns the flow and the rules; the signed download itself is done by
//! an [`UpdateBackend`] (the Tauri updater plugin in the app). The rules:
//!
//! - nothing is ever installed without the person asking: a check only learns
//!   that a version exists, a download only fetches and verifies it, and the
//!   install is its own command;
//! - the check at launch runs at most once a day (the time of the last
//!   completed check is kept in the data directory);
//! - a build without an updater public key reports `unavailable` and never
//!   touches the network.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{AppCore, AppError, AppResult, CoreEvent, lock};

/// The launch check does nothing if the last completed check is newer than this.
pub const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// File in the data directory holding the time of the last completed check, in ms.
const LAST_CHECK_FILE: &str = "last-update-check";
/// Progress events are sent at most this often (the final one always is).
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A newer version the update server announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
    pub version: String,
    pub notes: Option<String>,
}

/// Why an update step failed. Carries no server text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFailure {
    /// The server could not be reached or sent something unusable.
    Unreachable,
    /// The package does not match its signature, or the key.
    BadSignature,
    Other,
}

/// Reports bytes received so far and the total size when the server said it.
pub type ProgressFn = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

/// What `AppCore` needs from the updater.
pub trait UpdateBackend: Send + Sync {
    /// False when this build carries no updater public key.
    fn configured(&self) -> bool;
    /// Asks the update endpoint whether a newer version exists.
    fn check(&self) -> BoxFuture<'_, Result<Option<UpdateOffer>, UpdateFailure>>;
    /// Downloads the package of the offer the last `check` returned and
    /// verifies its signature with the public key. Nothing is installed.
    fn download(&self, progress: ProgressFn) -> BoxFuture<'_, Result<(), UpdateFailure>>;
    /// Installs the verified package. Blocking. May end the process.
    fn install(&self) -> Result<(), UpdateFailure>;
    /// Starts the new version in place of this one.
    fn restart(&self);
}

/// For platforms without an updater: reports updates as unavailable.
pub struct NoUpdates;

impl UpdateBackend for NoUpdates {
    fn configured(&self) -> bool {
        false
    }

    fn check(&self) -> BoxFuture<'_, Result<Option<UpdateOffer>, UpdateFailure>> {
        Box::pin(async { Err(UpdateFailure::Other) })
    }

    fn download(&self, _progress: ProgressFn) -> BoxFuture<'_, Result<(), UpdateFailure>> {
        Box::pin(async { Err(UpdateFailure::Other) })
    }

    fn install(&self) -> Result<(), UpdateFailure> {
        Err(UpdateFailure::Other)
    }

    fn restart(&self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    /// This build cannot update itself.
    Unavailable,
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    /// Downloaded and verified; `update_install` applies it.
    Ready,
    /// The last check failed.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub state: UpdateState,
    pub current_version: String,
    /// The announced version while one is available, downloading or ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub last_checked_at: Option<i64>,
    /// Why updates are unavailable, why the last check or download failed.
    pub message: Option<String>,
}

pub(crate) struct UpdateData {
    state: UpdateState,
    offer: Option<UpdateOffer>,
    last_checked_at: Option<i64>,
    message: Option<String>,
}

const UNAVAILABLE: &str = "This build cannot check for updates because it carries no update \
signing key. Download new versions from the releases page.";

impl UpdateData {
    pub(crate) fn new(configured: bool, last_checked_at: Option<i64>) -> Self {
        Self {
            state: if configured {
                UpdateState::Idle
            } else {
                UpdateState::Unavailable
            },
            offer: None,
            last_checked_at,
            message: (!configured).then(|| UNAVAILABLE.to_owned()),
        }
    }
}

fn failure_message(failure: UpdateFailure) -> &'static str {
    match failure {
        UpdateFailure::Unreachable => "The update server could not be reached.",
        UpdateFailure::BadSignature => {
            "The downloaded update failed signature verification and was discarded."
        }
        UpdateFailure::Other => "The update could not be completed.",
    }
}

pub(crate) fn read_last_check(dir: &std::path::Path) -> Option<i64> {
    std::fs::read_to_string(dir.join(LAST_CHECK_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

impl AppCore {
    pub fn update_status_now(&self) -> UpdateStatus {
        let data = lock(&self.inner.update);
        UpdateStatus {
            state: data.state,
            current_version: self.app_version(),
            version: data.offer.as_ref().map(|o| o.version.clone()),
            notes: data.offer.as_ref().and_then(|o| o.notes.clone()),
            last_checked_at: data.last_checked_at,
            message: data.message.clone(),
        }
    }

    pub async fn update_status(&self) -> AppResult<UpdateStatus> {
        Ok(self.update_status_now())
    }

    /// Changes the state under the lock and tells the UI.
    fn set_update(&self, change: impl FnOnce(&mut UpdateData)) -> UpdateStatus {
        change(&mut lock(&self.inner.update));
        let status = self.update_status_now();
        self.emit(CoreEvent::UpdateStatus(status.clone()));
        status
    }

    /// Moves to `next` unless a check or download is already running.
    fn begin_update_step(&self, next: UpdateState) -> AppResult<()> {
        {
            let mut data = lock(&self.inner.update);
            if matches!(data.state, UpdateState::Checking | UpdateState::Downloading) {
                return Err(AppError::new(
                    "update_busy",
                    "An update check or download is already running.",
                ));
            }
            data.state = next;
            data.message = None;
        }
        self.emit(CoreEvent::UpdateStatus(self.update_status_now()));
        Ok(())
    }

    /// Asks the server whether a newer version exists. A failure is reported
    /// in the returned status (`error`), not as a rejection.
    pub async fn update_check(&self) -> AppResult<UpdateStatus> {
        let updater = &self.inner.deps.updater;
        if !updater.configured() {
            return Ok(self.update_status_now());
        }
        if lock(&self.inner.update).state == UpdateState::Ready {
            // A verified package is waiting; checking again would drop it.
            return Ok(self.update_status_now());
        }
        let previous = lock(&self.inner.update).state;
        self.begin_update_step(UpdateState::Checking)?;
        let result = updater.check().await;
        let checked_at = now_ms();
        let status = match result {
            Ok(offer) => {
                self.remember_check(checked_at);
                self.set_update(|data| {
                    data.last_checked_at = Some(checked_at);
                    data.state = if offer.is_some() {
                        UpdateState::Available
                    } else {
                        UpdateState::UpToDate
                    };
                    data.offer = offer;
                })
            }
            Err(failure) => self.set_update(|data| {
                // Keep what was learned before: an offer stays downloadable.
                data.state = if previous == UpdateState::Available && data.offer.is_some() {
                    UpdateState::Available
                } else {
                    UpdateState::Error
                };
                data.message = Some(failure_message(failure).to_owned());
            }),
        };
        Ok(status)
    }

    /// Downloads and verifies the announced version, reporting progress as
    /// `update:progress`. Installs nothing. A failure leaves the version
    /// available to try again, with the reason in `message`.
    pub async fn update_download(&self) -> AppResult<UpdateStatus> {
        if lock(&self.inner.update).state != UpdateState::Available {
            return Err(AppError::new(
                "no_update",
                "There is no update to download. Check for updates first.",
            ));
        }
        self.begin_update_step(UpdateState::Downloading)?;
        let events = self.inner.deps.events.clone();
        let last_sent = Mutex::new(None::<Instant>);
        let progress: ProgressFn = Arc::new(move |received, total| {
            let mut last = lock(&last_sent);
            let done = total.is_some_and(|t| received >= t);
            if done || last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
                *last = Some(Instant::now());
                events.emit(CoreEvent::UpdateProgress {
                    received_bytes: received,
                    total_bytes: total,
                });
            }
        });
        let status = match self.inner.deps.updater.download(progress).await {
            Ok(()) => self.set_update(|data| data.state = UpdateState::Ready),
            Err(failure) => self.set_update(|data| {
                data.state = UpdateState::Available;
                data.message = Some(failure_message(failure).to_owned());
            }),
        };
        Ok(status)
    }

    /// Installs the downloaded version and restarts the app. Only valid in
    /// the `ready` state. The process may end inside the install (the Windows
    /// installer takes over), which is safe because the local store commits
    /// every change as it happens.
    pub async fn update_install(&self) -> AppResult<()> {
        if lock(&self.inner.update).state != UpdateState::Ready {
            return Err(AppError::new(
                "update_not_ready",
                "No verified update is waiting to be installed.",
            ));
        }
        let updater = self.inner.deps.updater.clone();
        let installed = tokio::task::spawn_blocking(move || updater.install())
            .await
            .map_err(|_| AppError::internal("A background task failed."))?;
        if let Err(failure) = installed {
            self.set_update(|data| {
                data.state = UpdateState::Available;
                data.message = Some(failure_message(failure).to_owned());
            });
            return Err(AppError::new("update_failed", failure_message(failure)));
        }
        self.shutdown().await;
        self.inner.deps.updater.restart();
        Ok(())
    }

    fn remember_check(&self, at_ms: i64) {
        let dir = &self.inner.cfg.data_dir;
        // Best effort: if it cannot be written the next launch checks again.
        if std::fs::create_dir_all(dir).is_ok() {
            let _ = std::fs::write(dir.join(LAST_CHECK_FILE), at_ms.to_string());
        }
    }

    /// The launch check: runs only when this build can update and the last
    /// completed check is more than a day old at `now_ms`.
    pub async fn update_check_if_due(&self, now_ms: i64) -> AppResult<UpdateStatus> {
        let interval = i64::try_from(UPDATE_CHECK_INTERVAL.as_millis()).unwrap_or(i64::MAX);
        let last = lock(&self.inner.update).last_checked_at;
        if last.is_some_and(|last| now_ms.saturating_sub(last) < interval) {
            return Ok(self.update_status_now());
        }
        self.update_check().await
    }

    /// Runs the launch check in the background. Call once after the app opens.
    pub fn start_update_check(&self) {
        let core = self.clone();
        self.spawn(self.inner.rt.spawn(async move {
            // Offline, busy or unavailable: the status carries the outcome.
            let _ = core.update_check_if_due(now_ms()).await;
        }));
    }
}
