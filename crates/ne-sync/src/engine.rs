//! The sync engine's public handle. The work happens in [`crate::runner`].

use std::sync::Arc;
use std::time::Duration;

use ne_crypto::account::AccountSecrets;
use ne_crypto::keys::{SecretKey, WorkspaceKey};
use ne_sync_protocol::SnapshotOutcome;
use ne_sync_protocol::ids::is_valid_id;
use ne_sync_protocol::limits::MAX_UPDATE_PAYLOAD;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use url::Url;

use crate::backoff::Backoff;
use crate::client::SyncClient;
use crate::error::{Result, SyncError};
use crate::random;
use crate::runner::{Command, Runner, SEAL_OVERHEAD};
use crate::status::SyncStatus;
use crate::store::{OutboxItem, SyncStore};
use crate::transport::SessionToken;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub server: Url,
    pub token: SessionToken,
    pub workspace_id: String,
    /// Random id of this installation. Must stay the same for the life of the
    /// local data and never be shared with another install.
    pub device_id: String,
    pub user_agent: String,
    /// How long local updates are collected before they are pushed together.
    pub batch_delay: Duration,
    /// Interval of the application-level keepalive.
    pub ping_interval: Duration,
    pub backoff: Backoff,
}

impl EngineConfig {
    pub fn new(
        server: Url,
        token: SessionToken,
        workspace_id: String,
        device_id: String,
        user_agent: String,
    ) -> Self {
        Self {
            server,
            token,
            workspace_id,
            device_id,
            user_agent,
            batch_delay: Duration::from_millis(1500),
            ping_interval: Duration::from_secs(30),
            backoff: Backoff::default(),
        }
    }
}

pub struct SyncEngine {
    commands: mpsc::UnboundedSender<Command>,
    status: watch::Receiver<SyncStatus>,
    store: Arc<dyn SyncStore>,
    task: JoinHandle<()>,
}

impl SyncEngine {
    /// Starts connecting in the background. The engine reads its position and
    /// outbox from `store`, so restarting it resumes where the last run stopped.
    pub fn start(
        config: EngineConfig,
        key: &WorkspaceKey,
        store: Arc<dyn SyncStore>,
    ) -> Result<Self> {
        if !is_valid_id(&config.device_id) || !is_valid_id(&config.workspace_id) {
            return Err(SyncError::Input("invalid device or workspace id".into()));
        }
        let (commands, inbox) = mpsc::unbounded_channel();
        let (status_tx, status) = watch::channel(SyncStatus::Offline);
        let runner = Runner::new(
            config,
            WorkspaceKey::from_bytes(key.as_bytes()),
            Arc::clone(&store),
            inbox,
            status_tx,
        )?;
        let task = tokio::spawn(runner.run());
        let engine = Self {
            commands,
            status,
            store,
            task,
        };
        // A restart with a non-empty outbox starts sending as soon as it connects.
        let _ = engine.commands.send(Command::Wake);
        Ok(engine)
    }

    /// Queues a local update for `doc_id`. The update is durable in the store
    /// before this returns; sending is batched and happens in the background.
    pub fn submit(&self, doc_id: &str, update: &[u8]) -> Result<()> {
        if !is_valid_id(doc_id) {
            return Err(SyncError::Input("invalid document id".into()));
        }
        if update.is_empty() || update.len() + SEAL_OVERHEAD > MAX_UPDATE_PAYLOAD {
            return Err(SyncError::TooLarge);
        }
        self.store
            .enqueue(OutboxItem {
                client_id: random::client_id(),
                doc_id: doc_id.to_owned(),
                update: update.to_vec(),
            })
            .map_err(|err| SyncError::Store(err.to_string()))?;
        // The engine may have stopped; the update stays queued for the next run.
        let _ = self.commands.send(Command::Wake);
        Ok(())
    }

    /// Tells the engine that the store's outbox gained updates it queued
    /// itself (not through [`submit`](Self::submit)), so they are sent soon.
    pub fn wake(&self) {
        let _ = self.commands.send(Command::Wake);
    }

    pub fn status(&self) -> SyncStatus {
        self.status.borrow().clone()
    }

    /// A receiver that wakes on every status change.
    pub fn watch_status(&self) -> watch::Receiver<SyncStatus> {
        self.status.clone()
    }

    /// The log position to pass as `upto` when uploading a snapshot. Read it
    /// **before** exporting the document state: the state then contains at
    /// least everything up to this position.
    pub fn snapshot_point(&self) -> Result<u64> {
        self.store
            .cursor()
            .map_err(|err| SyncError::Store(err.to_string()))
    }

    /// Uploads `state` (the full state of `doc_id`) as a snapshot covering the
    /// log up to `upto`, letting the server drop the document's older rows.
    /// Fails with [`SyncError::Offline`] unless connected and caught up.
    pub async fn upload_snapshot(
        &self,
        doc_id: &str,
        upto: u64,
        state: &[u8],
    ) -> Result<SnapshotOutcome> {
        if !is_valid_id(doc_id) {
            return Err(SyncError::Input("invalid document id".into()));
        }
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Snapshot {
                doc_id: doc_id.to_owned(),
                upto,
                state: state.to_vec(),
                reply,
            })
            .map_err(|_| SyncError::Stopped)?;
        answer.await.map_err(|_| SyncError::Stopped)?
    }

    /// Stops the engine cleanly. Dropping the engine instead stops it
    /// immediately, like a crash; queued updates stay in the store either way.
    pub async fn shutdown(self) {
        let (reply, done) = oneshot::channel();
        if self.commands.send(Command::Shutdown(reply)).is_ok() {
            let _ = done.await;
        }
    }
}

impl Drop for SyncEngine {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SyncClient {
    /// Engine settings for the current session.
    pub fn engine_config(&self, device_id: &str) -> Result<EngineConfig> {
        let session = self.transport.session().ok_or(SyncError::Unauthorized)?;
        Ok(EngineConfig::new(
            self.transport.base.clone(),
            session.token,
            session.workspace_id,
            device_id.to_owned(),
            self.transport.user_agent.clone(),
        ))
    }

    /// Starts syncing the workspace with the keys of an unlocked account.
    pub fn start_engine(
        &self,
        secrets: &AccountSecrets,
        device_id: &str,
        store: Arc<dyn SyncStore>,
    ) -> Result<SyncEngine> {
        SyncEngine::start(
            self.engine_config(device_id)?,
            &secrets.workspace_key,
            store,
        )
    }
}
