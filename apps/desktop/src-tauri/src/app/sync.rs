//! Running the sync engine: lifecycle, status for the UI, and the background
//! upkeep that follows a successful sync (attachment uploads, server log
//! compaction).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ne_core::tree::NodeKind;
use ne_crypto::keys::{SecretKey, WorkspaceKey};
use ne_store::DocRef;
use ne_sync::{
    Backoff, SnapshotOutcome, SyncClient, SyncEngine, SyncFailure, SyncStatus as EngineStatus,
};
use tokio::task::JoinHandle;

use super::sync_store::{VAULT_DOC, WorkspaceStore, wire_id};
use super::types::{SessionState, SyncState, SyncStatus};
use super::{AppCore, AppError, AppResult, CoreEvent, lock};

/// Faster timings for tests.
#[derive(Debug, Clone, Copy)]
pub struct EngineTuning {
    pub batch_delay: Duration,
    pub backoff: Backoff,
}

pub struct SyncRuntime {
    pub client: SyncClient,
    pub key: WorkspaceKey,
    pub engine: SyncEngine,
    watcher: JoinHandle<()>,
}

/// A document's local update log this long means the server holds about as
/// many rows for it, so a snapshot is worth uploading.
const SNAPSHOT_AFTER_UPDATES: usize = 40;
const COMPACTION_INTERVAL: Duration = Duration::from_secs(30);

fn failure_message(failure: SyncFailure) -> &'static str {
    match failure {
        SyncFailure::Unauthorized => "The session ended. Sign in again to resume syncing.",
        SyncFailure::ServerReset => {
            "The server's history is behind this device, as if it was reset or restored from an \
             old backup. Sync is stopped so nothing here is overwritten; your data on this \
             device is untouched."
        }
        SyncFailure::CorruptData => {
            "The server sent data that failed verification. Sync is stopped."
        }
        SyncFailure::Protocol => "The server and this app disagree about the sync protocol.",
        SyncFailure::Storage => "The local data could not be updated. Sync is stopped.",
    }
}

impl AppCore {
    pub(crate) fn runtime(&self) -> Option<Arc<SyncRuntime>> {
        lock(&self.inner.runtime).clone()
    }

    pub(crate) fn device_id(&self) -> AppResult<String> {
        self.store(|ws| {
            if let Some(id) = ws.setting("sync.device_id")? {
                return Ok(id);
            }
            let id = format!("d-{}", uuid::Uuid::new_v4().simple());
            ws.set_setting("sync.device_id", &id)?;
            Ok(id)
        })
    }

    pub(crate) fn start_runtime(&self, client: SyncClient, key: WorkspaceKey) -> AppResult<()> {
        let device_id = self.device_id()?;
        let mut config = client.engine_config(&device_id)?;
        if let Some(tuning) = &self.inner.cfg.engine_tuning {
            config.batch_delay = tuning.batch_delay;
            config.backoff = tuning.backoff;
        }
        let store = Arc::new(WorkspaceStore {
            ws: self.inner.ws.clone(),
            vault: self.inner.vault.clone(),
            open_pages: self.inner.open_pages.clone(),
            events: self.inner.deps.events.clone(),
        });
        let engine = SyncEngine::start(config, &key, store)?;
        let mut status = engine.watch_status();
        let core = self.clone();
        let watcher = self.inner.rt.spawn(async move {
            loop {
                let current = status.borrow_and_update().clone();
                core.on_engine_status(current);
                if status.changed().await.is_err() {
                    break;
                }
            }
        });
        let runtime = Arc::new(SyncRuntime {
            client,
            key,
            engine,
            watcher,
        });
        *lock(&self.inner.runtime) = Some(runtime);
        self.publish_sync_status();
        Ok(())
    }

    pub(crate) async fn stop_runtime(&self) {
        let Some(runtime) = lock(&self.inner.runtime).take() else {
            return;
        };
        runtime.watcher.abort();
        if let Ok(runtime) = Arc::try_unwrap(runtime) {
            runtime.engine.shutdown().await;
        }
    }

    async fn restart_runtime(&self) -> AppResult<()> {
        let Some(old) = self.runtime() else {
            return Ok(());
        };
        let client = old.client.clone();
        let key = WorkspaceKey::from_bytes(old.key.as_bytes());
        drop(old);
        self.stop_runtime().await;
        self.start_runtime(client, key)
    }

    fn on_engine_status(&self, status: EngineStatus) {
        match status {
            EngineStatus::Synced => {
                let now = self.inner.cfg_now();
                *lock(&self.inner.last_synced_at) = Some(now);
                self.schedule_maintenance();
            }
            EngineStatus::Error(SyncFailure::Unauthorized) => {
                let core = self.clone();
                self.inner.rt.spawn(async move {
                    core.enter_needs_sign_in().await;
                });
            }
            _ => {}
        }
        self.publish_sync_status();
    }

    /// The status for the UI, computed from the session and the engine.
    pub fn sync_status_now(&self) -> SyncStatus {
        let state = lock(&self.inner.session).state;
        let pending = self
            .with_ws(|ws| Ok(ws.outbox_len().unwrap_or(0)))
            .unwrap_or(0);
        let last = *lock(&self.inner.last_synced_at);
        let make = |state, pending_updates, last_error: Option<&str>| SyncStatus {
            state,
            pending_updates,
            last_synced_at: last,
            last_error: last_error.map(str::to_owned),
        };
        match state {
            SessionState::Empty | SessionState::Local => make(SyncState::Disabled, 0, None),
            SessionState::NeedsSignIn => make(
                SyncState::Error,
                pending,
                Some(failure_message(SyncFailure::Unauthorized)),
            ),
            SessionState::SignedIn => match self.runtime().map(|r| r.engine.status()) {
                None => make(SyncState::Offline, pending, None),
                Some(EngineStatus::Synced) => make(SyncState::Synced, pending, None),
                Some(EngineStatus::Pending { .. }) => make(SyncState::Pending, pending, None),
                Some(EngineStatus::Offline) => make(SyncState::Offline, pending, None),
                Some(EngineStatus::Error(f)) => {
                    make(SyncState::Error, pending, Some(failure_message(f)))
                }
            },
        }
    }

    pub async fn sync_status(&self) -> AppResult<SyncStatus> {
        Ok(self.sync_status_now())
    }

    /// Emits `sync:status` when it differs from the last one emitted.
    pub(crate) fn publish_sync_status(&self) {
        let status = self.sync_status_now();
        let mut last = lock(&self.inner.last_status);
        if last.as_ref() != Some(&status) {
            *last = Some(status.clone());
            drop(last);
            self.emit(CoreEvent::SyncStatus(status));
        }
    }

    /// Called after every local write: sends it soon and refreshes the status.
    pub(crate) fn after_local_change(&self) {
        if let Some(runtime) = self.runtime() {
            runtime.engine.wake();
        }
        self.publish_sync_status();
    }

    /// Forces a sync cycle and waits until everything is sent and received.
    pub async fn sync_now(&self) -> AppResult<()> {
        let mut runtime = self
            .runtime()
            .ok_or_else(|| AppError::new("not_signed_in", "Sync is not set up on this device."))?;
        if runtime.engine.status() == EngineStatus::Offline {
            // Reconnect now instead of waiting out the backoff.
            self.restart_runtime().await?;
            runtime = self.runtime().ok_or_else(|| {
                AppError::new("not_signed_in", "Sync is not set up on this device.")
            })?;
        }
        runtime.engine.wake();
        let mut status = runtime.engine.watch_status();
        let deadline = tokio::time::Instant::now() + self.inner.cfg.sync_wait;
        loop {
            let current = status.borrow_and_update().clone();
            match current {
                EngineStatus::Synced if self.pending_updates() == 0 => return Ok(()),
                EngineStatus::Error(failure) => {
                    return Err(AppError::new("sync_failed", failure_message(failure)));
                }
                _ => {}
            }
            let step = tokio::time::timeout_at(
                deadline.min(tokio::time::Instant::now() + Duration::from_millis(100)),
                status.changed(),
            )
            .await;
            if step.is_err() && tokio::time::Instant::now() >= deadline {
                return Err(if runtime.engine.status() == EngineStatus::Offline {
                    AppError::new("offline", "The server cannot be reached.")
                } else {
                    AppError::new(
                        "sync_timeout",
                        "Sync is taking longer than expected and continues in the background.",
                    )
                });
            }
        }
    }

    /// Updates waiting to be sent (queued even before an account exists).
    pub fn pending_updates(&self) -> usize {
        self.with_ws(|ws| Ok(ws.outbox_len().unwrap_or(0)))
            .unwrap_or(0)
    }

    // ---- upkeep ----

    /// Runs the upkeep once at a time, in the background.
    pub(crate) fn schedule_maintenance(&self) {
        if self.runtime().is_none()
            || self
                .inner
                .maintenance_busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return;
        }
        let core = self.clone();
        self.spawn(self.inner.rt.spawn(async move {
            // Failures (offline, session ended) are retried on the next cycle.
            let _ = core.push_pending_vault_record().await;
            let _ = core.upload_attachments().await;
            let due = {
                let mut last = lock(&core.inner.last_compaction);
                let due = last.is_none_or(|t: Instant| t.elapsed() >= COMPACTION_INTERVAL);
                if due {
                    *last = Some(Instant::now());
                }
                due
            };
            if due {
                let _ = core.compact_server_log(SNAPSHOT_AFTER_UPDATES).await;
            }
            core.inner.maintenance_busy.store(false, Ordering::SeqCst);
        }));
    }

    /// Uploads a snapshot for every document whose update log has grown past
    /// `threshold` updates, so the server can drop the rows it replaces, and
    /// folds the local log too. Returns how many snapshots the server took.
    pub async fn compact_server_log(&self, threshold: usize) -> AppResult<usize> {
        let runtime = self
            .runtime()
            .ok_or_else(|| AppError::new("not_signed_in", "Sync is not set up on this device."))?;
        let candidates = self.with_ws(|ws| {
            let mut docs = vec![DocRef::Tree];
            let mut stack = vec![None];
            while let Some(parent) = stack.pop() {
                for info in ws.tree().children(parent, true)? {
                    stack.push(Some(info.id));
                    if info.kind == NodeKind::Page {
                        docs.push(DocRef::Page(info.id));
                    }
                }
            }
            docs.push(DocRef::Vault(VAULT_DOC.to_owned()));
            docs.retain(|doc| {
                ws.update_log_size(doc)
                    .is_ok_and(|(count, _)| count >= threshold)
            });
            Ok(docs)
        })?;
        let mut taken = 0;
        for doc in candidates {
            // The position is read before the state is exported, so the state
            // covers at least everything up to it.
            let upto = runtime.engine.snapshot_point()?;
            let state = self.with_ws(|ws| match &doc {
                DocRef::Tree => Ok((
                    ws.tree().export_snapshot().map_err(ne_store::Error::from)?,
                    ws.tree().version(),
                )),
                DocRef::Page(id) => {
                    let snapshot = ws.page_snapshot(*id)?;
                    let version = ws.open_page(*id)?.version();
                    Ok((snapshot, version))
                }
                DocRef::Vault(_) => self.vault_snapshot(ws),
            })?;
            match runtime
                .engine
                .upload_snapshot(&wire_id(&doc), upto, &state.0)
                .await
            {
                Ok(SnapshotOutcome::Accepted { .. }) => {
                    taken += 1;
                    self.with_ws(|ws| match &doc {
                        DocRef::Vault(name) => ws
                            .put_vault_doc(name, &state.0, &state.1)
                            .map_err(Into::into),
                        other => ws.compact(other).map_err(Into::into),
                    })?;
                }
                Ok(_) => {}
                Err(ne_sync::SyncError::TooLarge) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(taken)
    }
}

impl super::Inner {
    pub(crate) fn cfg_now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0)
    }
}
