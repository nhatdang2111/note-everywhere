//! The background task behind [`SyncEngine`](crate::SyncEngine): one
//! connection at a time, reconnecting with backoff, sending the outbox in
//! batches and applying what arrives.
//!
//! Crash safety rests on three rules. An outbox item is removed only after the
//! server acknowledged it, and a retry reuses its client id, so the server
//! stores it once. An incoming page is applied and the cursor moved in a
//! single store call. Rows the device itself wrote are never applied again.

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use ne_crypto::aead;
use ne_crypto::envelope::{open_update, seal_update};
use ne_crypto::keys::WorkspaceKey;
use ne_sync_protocol::limits::{MAX_FRAME_BYTES, MAX_PUSH_BYTES, MAX_PUSH_ENTRIES};
use ne_sync_protocol::{
    ClientFrame, ClientId, EntryKind, ErrorCode, LogEntry, PushEntry, ServerFrame, SnapshotOutcome,
};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Duration, Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
};

use crate::engine::EngineConfig;
use crate::error::{Result, SyncError};
use crate::random;
use crate::status::{SyncFailure, SyncStatus};
use crate::store::{IncomingBatch, IncomingKind, IncomingUpdate, StoreError, SyncStore};
use crate::tls;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Ws, Message>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// First byte of every sealed plaintext: what kind of row it is.
const PLAIN_UPDATE: u8 = 0;
const PLAIN_SNAPSHOT: u8 = 1;
/// Bytes sealing adds to an update: the kind byte plus the envelope.
pub(crate) const SEAL_OVERHEAD: usize = 1 + aead::OVERHEAD;
/// Close code the server uses when a session ends.
const CLOSE_SESSION_ENDED: u16 = 4001;

pub(crate) enum Command {
    /// An update was queued or the engine just started.
    Wake,
    Snapshot {
        doc_id: String,
        upto: u64,
        state: Vec<u8>,
        reply: oneshot::Sender<Result<SnapshotOutcome>>,
    },
    Shutdown(oneshot::Sender<()>),
}

/// Why a connection ended.
enum End {
    Shutdown(oneshot::Sender<()>),
    /// Network trouble or a transient server answer: reconnect with backoff.
    Dropped,
    /// Retrying cannot help.
    Fatal(SyncFailure),
}

struct InFlight {
    req_id: u32,
    ids: Vec<ClientId>,
}

/// State of one connection.
struct Link {
    sink: Sink,
    /// True once the initial catch-up finished; only then is the log position
    /// known to track the server and pushes are allowed.
    live: bool,
    pull_req: Option<u32>,
    in_flight: Option<InFlight>,
    waiters: HashMap<u32, oneshot::Sender<Result<SnapshotOutcome>>>,
    flush_at: Option<Instant>,
    next_req: u32,
    last_rx: Instant,
}

impl Link {
    fn new(sink: Sink) -> Self {
        Self {
            sink,
            live: false,
            pull_req: None,
            in_flight: None,
            waiters: HashMap::new(),
            flush_at: None,
            next_req: 1,
            last_rx: Instant::now(),
        }
    }

    /// Request ids start at 1; 0 marks unsolicited server frames.
    fn req(&mut self) -> u32 {
        let id = self.next_req;
        self.next_req = self.next_req.checked_add(1).unwrap_or(1);
        id
    }
}

pub(crate) struct Runner {
    cfg: EngineConfig,
    key: WorkspaceKey,
    store: Arc<dyn SyncStore>,
    inbox: mpsc::UnboundedReceiver<Command>,
    status: watch::Sender<SyncStatus>,
    tls: Arc<rustls::ClientConfig>,
}

fn storage(_: StoreError) -> End {
    End::Fatal(SyncFailure::Storage)
}

fn gone() -> oneshot::Sender<()> {
    oneshot::channel().0
}

impl Runner {
    pub(crate) fn new(
        cfg: EngineConfig,
        key: WorkspaceKey,
        store: Arc<dyn SyncStore>,
        inbox: mpsc::UnboundedReceiver<Command>,
        status: watch::Sender<SyncStatus>,
    ) -> Result<Self> {
        Ok(Self {
            cfg,
            key,
            store,
            inbox,
            status,
            tls: tls::client_config()?,
        })
    }

    pub(crate) async fn run(mut self) {
        let mut attempt = 0u32;
        loop {
            match self.connection(&mut attempt).await {
                End::Shutdown(reply) => {
                    self.status.send_replace(SyncStatus::Offline);
                    let _ = reply.send(());
                    return;
                }
                End::Fatal(failure) => {
                    self.status.send_replace(SyncStatus::Error(failure));
                    self.idle_until_shutdown().await;
                    return;
                }
                End::Dropped => {
                    self.status.send_replace(SyncStatus::Offline);
                    let delay = self.cfg.backoff.next(attempt);
                    attempt = attempt.saturating_add(1);
                    if let Some(reply) = self.pause(delay).await {
                        let _ = reply.send(());
                        return;
                    }
                }
            }
        }
    }

    /// Waits out a backoff while still answering commands.
    async fn pause(&mut self, delay: Duration) -> Option<oneshot::Sender<()>> {
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                () = &mut sleep => return None,
                command = self.inbox.recv() => match command {
                    Some(Command::Wake) => {}
                    Some(Command::Snapshot { reply, .. }) => {
                        let _ = reply.send(Err(SyncError::Offline));
                    }
                    Some(Command::Shutdown(reply)) => return Some(reply),
                    None => return Some(gone()),
                },
            }
        }
    }

    async fn idle_until_shutdown(&mut self) {
        while let Some(command) = self.inbox.recv().await {
            match command {
                Command::Wake => {}
                Command::Snapshot { reply, .. } => {
                    let _ = reply.send(Err(SyncError::Stopped));
                }
                Command::Shutdown(reply) => {
                    let _ = reply.send(());
                    return;
                }
            }
        }
    }

    async fn connect(&self) -> std::result::Result<Ws, End> {
        let mut url = self.cfg.server.clone();
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|()| End::Fatal(SyncFailure::Protocol))?;
        url.set_path("/v1/sync");
        url.query_pairs_mut()
            .clear()
            .append_pair("device", &self.cfg.device_id);

        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| End::Fatal(SyncFailure::Protocol))?;
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", self.cfg.token.expose()))
            .map_err(|_| End::Fatal(SyncFailure::Protocol))?;
        auth.set_sensitive(true);
        let agent = HeaderValue::from_str(&self.cfg.user_agent)
            .map_err(|_| End::Fatal(SyncFailure::Protocol))?;
        request.headers_mut().insert(header::AUTHORIZATION, auth);
        request.headers_mut().insert(header::USER_AGENT, agent);

        let limit = Some(MAX_FRAME_BYTES + 64 * 1024);
        let config = WebSocketConfig::default()
            .max_message_size(limit)
            .max_frame_size(limit);
        let attempt = connect_async_tls_with_config(
            request,
            Some(config),
            true,
            Some(Connector::Rustls(Arc::clone(&self.tls))),
        );
        match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
            Ok(Ok((ws, _))) => Ok(ws),
            Ok(Err(WsError::Http(response))) if matches!(response.status().as_u16(), 401 | 403) => {
                Err(End::Fatal(SyncFailure::Unauthorized))
            }
            _ => Err(End::Dropped),
        }
    }

    async fn connection(&mut self, attempt: &mut u32) -> End {
        let ws = match self.connect().await {
            Ok(ws) => ws,
            Err(end) => return end,
        };
        let (sink, mut stream) = ws.split();
        let mut link = Link::new(sink);
        let mut ping = tokio::time::interval(self.cfg.ping_interval);
        ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ping.tick().await;

        let started = match self.store.cursor() {
            Ok(cursor) => self.send_pull(&mut link, cursor).await,
            Err(err) => Err(storage(err)),
        };
        if let Err(end) = started {
            return end;
        }
        self.publish(&link);

        let end = loop {
            let flush_at = link.flush_at;
            let flush = async move {
                match flush_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            let step = tokio::select! {
                incoming = stream.next() => self.on_incoming(incoming, &mut link, attempt).await,
                command = self.inbox.recv() => self.on_command(command, &mut link).await,
                _ = ping.tick() => self.on_ping(&mut link).await,
                () = flush, if link.live && link.in_flight.is_none() => {
                    link.flush_at = None;
                    self.push(&mut link).await
                }
            };
            if let Err(end) = step {
                break end;
            }
            self.publish(&link);
        };
        for (_, waiter) in link.waiters.drain() {
            let _ = waiter.send(Err(SyncError::Offline));
        }
        let _ = link.sink.close().await;
        end
    }

    fn publish(&self, link: &Link) {
        let outbox = self.store.outbox_len().unwrap_or(0);
        let status = if link.live && link.in_flight.is_none() && outbox == 0 {
            SyncStatus::Synced
        } else {
            SyncStatus::Pending { outbox }
        };
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            if changed {
                *current = status;
            }
            changed
        });
    }

    async fn send(&self, link: &mut Link, frame: ClientFrame) -> std::result::Result<(), End> {
        let bytes = frame
            .encode()
            .map_err(|_| End::Fatal(SyncFailure::Protocol))?;
        link.sink
            .send(Message::binary(bytes))
            .await
            .map_err(|_| End::Dropped)
    }

    async fn send_pull(&self, link: &mut Link, cursor: u64) -> std::result::Result<(), End> {
        let req_id = link.req();
        link.pull_req = Some(req_id);
        self.send(link, ClientFrame::Pull { req_id, cursor }).await
    }

    async fn on_ping(&self, link: &mut Link) -> std::result::Result<(), End> {
        if link.last_rx.elapsed() > self.cfg.ping_interval * 3 {
            return Err(End::Dropped);
        }
        link.sink
            .send(Message::text("ping"))
            .await
            .map_err(|_| End::Dropped)
    }

    async fn on_command(
        &self,
        command: Option<Command>,
        link: &mut Link,
    ) -> std::result::Result<(), End> {
        match command {
            None => Err(End::Shutdown(gone())),
            Some(Command::Shutdown(reply)) => Err(End::Shutdown(reply)),
            Some(Command::Wake) => {
                // The window opens at the first queued update, so latency is bounded.
                link.flush_at
                    .get_or_insert_with(|| Instant::now() + self.cfg.batch_delay);
                Ok(())
            }
            Some(Command::Snapshot {
                doc_id,
                upto,
                state,
                reply,
            }) => self.send_snapshot(link, doc_id, upto, state, reply).await,
        }
    }

    async fn send_snapshot(
        &self,
        link: &mut Link,
        doc_id: String,
        upto: u64,
        state: Vec<u8>,
        reply: oneshot::Sender<Result<SnapshotOutcome>>,
    ) -> std::result::Result<(), End> {
        if !link.live {
            let _ = reply.send(Err(SyncError::Offline));
            return Ok(());
        }
        // The covered position travels inside the sealed state so the server
        // cannot alter it unnoticed.
        let mut plain = vec![PLAIN_SNAPSHOT];
        plain.extend_from_slice(&upto.to_be_bytes());
        plain.extend_from_slice(&state);
        let sealed = seal_update(
            &self.key,
            &self.cfg.workspace_id,
            &doc_id,
            &self.cfg.device_id,
            &plain,
        );
        let Ok(payload) = sealed else {
            let _ = reply.send(Err(SyncError::Crypto));
            return Ok(());
        };
        let req_id = link.req();
        let frame = ClientFrame::Snapshot {
            req_id,
            client_id: random::client_id(),
            doc_id,
            upto,
            payload,
        };
        let Ok(bytes) = frame.encode() else {
            let _ = reply.send(Err(SyncError::TooLarge));
            return Ok(());
        };
        link.waiters.insert(req_id, reply);
        link.sink
            .send(Message::binary(bytes))
            .await
            .map_err(|_| End::Dropped)
    }

    async fn push(&self, link: &mut Link) -> std::result::Result<(), End> {
        let budget = MAX_PUSH_BYTES - MAX_PUSH_ENTRIES * SEAL_OVERHEAD;
        let batch = self
            .store
            .outbox_next(MAX_PUSH_ENTRIES, budget)
            .map_err(storage)?;
        if batch.is_empty() {
            return Ok(());
        }
        let mut entries = Vec::with_capacity(batch.len());
        let mut ids = Vec::with_capacity(batch.len());
        for item in batch {
            let mut plain = Vec::with_capacity(item.update.len() + 1);
            plain.push(PLAIN_UPDATE);
            plain.extend_from_slice(&item.update);
            let payload = seal_update(
                &self.key,
                &self.cfg.workspace_id,
                &item.doc_id,
                &self.cfg.device_id,
                &plain,
            )
            .map_err(|_| End::Fatal(SyncFailure::Protocol))?;
            ids.push(item.client_id);
            entries.push(PushEntry {
                client_id: item.client_id,
                doc_id: item.doc_id,
                payload,
            });
        }
        let req_id = link.req();
        self.send(link, ClientFrame::Push { req_id, entries })
            .await?;
        link.in_flight = Some(InFlight { req_id, ids });
        Ok(())
    }

    async fn on_incoming(
        &self,
        incoming: Option<std::result::Result<Message, WsError>>,
        link: &mut Link,
        attempt: &mut u32,
    ) -> std::result::Result<(), End> {
        let Some(Ok(message)) = incoming else {
            return Err(End::Dropped);
        };
        link.last_rx = Instant::now();
        match message {
            Message::Binary(bytes) => {
                let frame =
                    ServerFrame::decode(&bytes).map_err(|_| End::Fatal(SyncFailure::Protocol))?;
                self.on_frame(frame, link, attempt).await
            }
            Message::Close(frame) => Err(match frame {
                Some(frame) if u16::from(frame.code) == CLOSE_SESSION_ENDED => {
                    End::Fatal(SyncFailure::Unauthorized)
                }
                _ => End::Dropped,
            }),
            // Text carries the keepalive answer; control frames are handled by the library.
            _ => Ok(()),
        }
    }

    async fn on_frame(
        &self,
        frame: ServerFrame,
        link: &mut Link,
        attempt: &mut u32,
    ) -> std::result::Result<(), End> {
        match frame {
            ServerFrame::Updates {
                req_id,
                done,
                next_cursor,
                head,
                entries,
            } => {
                let cursor = self.store.cursor().map_err(storage)?;
                if head < cursor {
                    return Err(End::Fatal(SyncFailure::ServerReset));
                }
                let updates = self.open_entries(entries, cursor)?;
                self.store
                    .apply_incoming(IncomingBatch {
                        updates,
                        cursor: next_cursor,
                    })
                    .map_err(storage)?;
                if req_id != 0 && link.pull_req == Some(req_id) {
                    if done {
                        link.live = true;
                        link.pull_req = None;
                        *attempt = 0;
                        if self.store.outbox_len().map_err(storage)? > 0 {
                            link.flush_at = Some(Instant::now());
                        }
                    } else {
                        self.send_pull(link, next_cursor).await?;
                    }
                }
                Ok(())
            }
            ServerFrame::Ack { req_id, head, seqs } => {
                let Some(flight) = link.in_flight.take_if(|f| f.req_id == req_id) else {
                    return Ok(());
                };
                if seqs.len() != flight.ids.len() {
                    return Err(End::Fatal(SyncFailure::Protocol));
                }
                self.store.outbox_remove(&flight.ids).map_err(storage)?;
                if link.live {
                    // Rows before `head` reached this connection ahead of the
                    // ack (one ordered stream), so the position may move there.
                    self.store
                        .apply_incoming(IncomingBatch {
                            updates: vec![],
                            cursor: head,
                        })
                        .map_err(storage)?;
                }
                if self.store.outbox_len().map_err(storage)? > 0 {
                    link.flush_at = Some(Instant::now());
                }
                Ok(())
            }
            ServerFrame::SnapshotAck { req_id, outcome } => {
                if let Some(waiter) = link.waiters.remove(&req_id) {
                    let _ = waiter.send(Ok(outcome));
                }
                Ok(())
            }
            ServerFrame::Error { req_id, code } => {
                if let Some(waiter) = link.waiters.remove(&req_id) {
                    let _ = waiter.send(Err(match code {
                        ErrorCode::TooLarge => SyncError::TooLarge,
                        ErrorCode::Unauthorized => SyncError::Unauthorized,
                        _ => SyncError::Unavailable,
                    }));
                    if code != ErrorCode::Unauthorized {
                        return Ok(());
                    }
                }
                Err(match code {
                    ErrorCode::Unauthorized => End::Fatal(SyncFailure::Unauthorized),
                    ErrorCode::Unavailable => End::Dropped,
                    _ => End::Fatal(SyncFailure::Protocol),
                })
            }
        }
    }

    /// Decrypts a page. Rows at or before the cursor and rows this device
    /// wrote are not applied; a row that fails authentication stops sync
    /// rather than being skipped, since skipping would lose data silently.
    fn open_entries(
        &self,
        entries: Vec<LogEntry>,
        cursor: u64,
    ) -> std::result::Result<Vec<IncomingUpdate>, End> {
        let corrupt = || End::Fatal(SyncFailure::CorruptData);
        let mut updates = Vec::new();
        for entry in entries {
            if entry.seq <= cursor || entry.device_id == self.cfg.device_id {
                continue;
            }
            let plain = open_update(
                &self.key,
                &self.cfg.workspace_id,
                &entry.doc_id,
                &entry.device_id,
                &entry.payload,
            )
            .map_err(|_| corrupt())?;
            // The first plaintext byte says what the row is. It is sealed, so
            // the server cannot relabel an update as a snapshot or back.
            let (tag, body) = plain.split_first().ok_or_else(corrupt)?;
            let (kind, plaintext) = match (entry.kind, *tag) {
                (EntryKind::Update, PLAIN_UPDATE) => (IncomingKind::Update, body.to_vec()),
                (EntryKind::Snapshot, PLAIN_SNAPSHOT) => {
                    let (head, state) = body.split_at_checked(8).ok_or_else(corrupt)?;
                    let upto = u64::from_be_bytes(head.try_into().map_err(|_| corrupt())?);
                    if upto != entry.upto {
                        return Err(corrupt());
                    }
                    (IncomingKind::Snapshot { upto }, state.to_vec())
                }
                _ => return Err(corrupt()),
            };
            updates.push(IncomingUpdate {
                seq: entry.seq,
                doc_id: entry.doc_id,
                device_id: entry.device_id,
                kind,
                plaintext,
            });
        }
        Ok(updates)
    }
}
