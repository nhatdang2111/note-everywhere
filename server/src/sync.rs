//! WebSocket sync: push, pull, snapshot compaction and live fan-out.
//!
//! The log is one table. Every row has a global sequence number. A snapshot is
//! a row of its own (with a fresh sequence number) that carries the state of
//! one document up to `upto`; accepting it turns that document's older rows
//! into tombstones (payload removed, id kept so a retried push is still
//! recognised). See `docs/sync-protocol.md` for the rules and why they keep
//! every device convergent.

use ne_sync_protocol::frame::{
    ClientFrame, EntryKind, ErrorCode, LogEntry, ProtocolError, PushEntry, ServerFrame,
    SnapshotOutcome,
};
use ne_sync_protocol::ids::is_valid_id;
use ne_sync_protocol::limits::{MAX_FRAME_BYTES, MAX_PAGE_ENTRIES, MAX_PUSH_BYTES};
use serde::{Deserialize, Serialize};
use worker::{
    Request, Response, WebSocket, WebSocketIncomingMessage, WebSocketPair, console_error,
};

use crate::db::{self, b, n, s, uint};
use crate::http::{ApiError, ApiResult};
use crate::workspace_do::Workspace;
use worker::Date;

/// Connections kept at once (one person, a handful of devices).
const MAX_SOCKETS: usize = 16;
/// Tombstones are forgotten after this long; until then a retried push whose
/// original was compacted away is still recognised.
const TOMBSTONE_RETENTION_MS: u64 = 30 * 24 * 3600 * 1000;
/// Tombstones removed per compaction, to bound the work of one message.
const TOMBSTONE_PURGE_BATCH: u64 = 200;

/// Per-connection state, kept in the WebSocket attachment so it survives
/// hibernation.
#[derive(Serialize, Deserialize)]
struct Conn {
    session: String,
    device: String,
    expires_at: u64,
    /// True once a pull has reached the head; only then does fan-out apply.
    live: bool,
}

fn send(ws: &WebSocket, frame: &ServerFrame) -> worker::Result<()> {
    let bytes = frame
        .encode()
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    ws.send_with_bytes(bytes)
}

fn error_code(err: ProtocolError) -> ErrorCode {
    match err {
        ProtocolError::TooLarge => ErrorCode::TooLarge,
        _ => ErrorCode::Malformed,
    }
}

impl Workspace {
    pub(crate) fn accept_sync(&self, req: &Request, now: u64) -> ApiResult<Response> {
        let upgrade = req.headers().get("Upgrade").ok().flatten();
        if !upgrade.is_some_and(|v| v.eq_ignore_ascii_case("websocket")) {
            return Err(ApiError::BadRequest);
        }
        let session = self.authenticate(req, now)?;
        let device = req
            .url()?
            .query_pairs()
            .find(|(key, _)| key == "device")
            .map(|(_, value)| value.into_owned())
            .filter(|device| is_valid_id(device))
            .ok_or(ApiError::BadRequest)?;
        let device_tag = format!("d-{device}");
        // A device that reconnects replaces its previous, probably dead, socket.
        for stale in self.state.get_websockets_with_tag(&device_tag) {
            let _ = stale.close(Some(4000), Some("replaced"));
        }
        if self.state.get_websockets().len() >= MAX_SOCKETS {
            return Err(ApiError::Unavailable);
        }
        let pair = WebSocketPair::new()?;
        // Accepting through the state lets the object hibernate between
        // messages, so idle connections cost no duration.
        self.state
            .accept_websocket_with_tags(&pair.server, &[&session.id, &device_tag]);
        pair.server.serialize_attachment(Conn {
            session: session.id,
            device,
            expires_at: session.expires_at,
            live: false,
        })?;
        Ok(Response::from_websocket(pair.client)?)
    }

    /// Handles one incoming frame. Failures are reported to the client as
    /// error frames; nothing here propagates an error to the runtime.
    pub(crate) fn on_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) {
        let WebSocketIncomingMessage::Binary(bytes) = message else {
            let _ = send(
                &ws,
                &ServerFrame::Error {
                    req_id: 0,
                    code: ErrorCode::Malformed,
                },
            );
            let _ = ws.close(Some(1003), Some("binary frames only"));
            return;
        };
        let conn = match ws.deserialize_attachment::<Conn>() {
            Ok(Some(conn)) => conn,
            _ => {
                let _ = ws.close(Some(1011), Some("unknown connection"));
                return;
            }
        };
        let now = Date::now().as_millis();
        if now >= conn.expires_at {
            let _ = send(
                &ws,
                &ServerFrame::Error {
                    req_id: 0,
                    code: ErrorCode::Unauthorized,
                },
            );
            let _ = ws.close(Some(4001), Some("session ended"));
            return;
        }
        if bytes.len() > MAX_FRAME_BYTES {
            let _ = send(
                &ws,
                &ServerFrame::Error {
                    req_id: 0,
                    code: ErrorCode::TooLarge,
                },
            );
            let _ = ws.close(Some(1009), Some("frame too large"));
            return;
        }
        let frame = match ClientFrame::decode(&bytes) {
            Ok(frame) => frame,
            Err(err) => {
                let _ = send(
                    &ws,
                    &ServerFrame::Error {
                        req_id: 0,
                        code: error_code(err),
                    },
                );
                let _ = ws.close(Some(1008), Some("protocol violation"));
                return;
            }
        };
        let (req_id, outcome) = match frame {
            ClientFrame::Push { req_id, entries } => {
                (req_id, self.handle_push(&ws, &conn, req_id, entries, now))
            }
            ClientFrame::Pull { req_id, cursor } => {
                (req_id, self.handle_pull(&ws, conn, req_id, cursor))
            }
            ClientFrame::Snapshot {
                req_id,
                client_id,
                doc_id,
                upto,
                payload,
            } => (
                req_id,
                self.handle_snapshot(&conn, req_id, &client_id.0, &doc_id, upto, payload, now),
            ),
        };
        let reply = outcome.unwrap_or_else(|err| {
            console_error!("sync handler failed: {err}");
            ServerFrame::Error {
                req_id,
                code: ErrorCode::Unavailable,
            }
        });
        if send(&ws, &reply).is_err() {
            console_error!("could not send a reply");
        }
    }

    fn head(&self) -> worker::Result<u64> {
        self.db
            .one("SELECT COALESCE(MAX(seq), 0) FROM updates", vec![])?
            .map_or(Ok(0), |row| uint(&row, 0))
    }

    fn handle_push(
        &self,
        origin: &WebSocket,
        conn: &Conn,
        req_id: u32,
        entries: Vec<PushEntry>,
        now: u64,
    ) -> worker::Result<ServerFrame> {
        let total: usize = entries.iter().map(|e| e.payload.len()).sum();
        if total > MAX_PUSH_BYTES {
            return Ok(ServerFrame::Error {
                req_id,
                code: ErrorCode::TooLarge,
            });
        }
        let mut seqs = Vec::with_capacity(entries.len());
        let mut fresh = Vec::new();
        let mut failure = None;
        for entry in entries {
            let inserted = self.db.query(
                "INSERT OR IGNORE INTO updates (client_id, doc_id, device_id, kind, upto, payload, at)
                 VALUES (?, ?, ?, 0, 0, ?, ?) RETURNING seq",
                vec![b(&entry.client_id.0), s(&entry.doc_id), s(&conn.device), b(&entry.payload), n(now)],
            );
            match inserted {
                Ok(rows) => match rows.first() {
                    Some(row) => {
                        let seq = uint(row, 0)?;
                        seqs.push(seq);
                        fresh.push(LogEntry {
                            seq,
                            kind: EntryKind::Update,
                            upto: 0,
                            doc_id: entry.doc_id,
                            device_id: conn.device.clone(),
                            payload: entry.payload,
                        });
                    }
                    None => seqs.push(0),
                },
                Err(err) => {
                    failure = Some(err);
                    break;
                }
            }
        }
        let head = self.head()?;
        // Whatever was stored is delivered, even if a later entry failed, so a
        // live peer never misses a row that exists in the log.
        self.fan_out(origin, head, fresh);
        match failure {
            Some(err) => Err(err),
            None => Ok(ServerFrame::Ack { req_id, head, seqs }),
        }
    }

    fn fan_out(&self, origin: &WebSocket, head: u64, entries: Vec<LogEntry>) {
        if entries.is_empty() {
            return;
        }
        let frame = ServerFrame::Updates {
            req_id: 0,
            done: true,
            next_cursor: head,
            head,
            entries,
        };
        let Ok(bytes) = frame.encode() else {
            // Too large to relay live; peers catch up with their next pull.
            return;
        };
        let now = Date::now().as_millis();
        for peer in self.state.get_websockets() {
            if peer == *origin {
                continue;
            }
            let live = matches!(
                peer.deserialize_attachment::<Conn>(),
                Ok(Some(c)) if c.live && now < c.expires_at
            );
            if live {
                let _ = peer.send_with_bytes(&bytes);
            }
        }
    }

    fn handle_pull(
        &self,
        ws: &WebSocket,
        mut conn: Conn,
        req_id: u32,
        cursor: u64,
    ) -> worker::Result<ServerFrame> {
        let head = self.head()?;
        let mut entries: Vec<LogEntry> = Vec::new();
        let mut bytes = 0usize;
        let mut more = false;
        let mut failure = None;
        self.db.query_while(
            "SELECT seq, kind, upto, doc_id, device_id, payload FROM updates
             WHERE seq > ? AND payload IS NOT NULL ORDER BY seq",
            vec![n(cursor)],
            |row| {
                let entry = (|| {
                    Ok::<_, worker::Error>(LogEntry {
                        seq: uint(&row, 0)?,
                        kind: if db::int(&row, 1)? == 1 {
                            EntryKind::Snapshot
                        } else {
                            EntryKind::Update
                        },
                        upto: uint(&row, 2)?,
                        doc_id: db::text(&row, 3)?,
                        device_id: db::text(&row, 4)?,
                        payload: db::blob(&row, 5)?,
                    })
                })();
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        failure = Some(err);
                        return false;
                    }
                };
                // Always deliver at least one entry so a page makes progress.
                if !entries.is_empty()
                    && (entries.len() >= MAX_PAGE_ENTRIES
                        || bytes + entry.payload.len() > MAX_FRAME_BYTES - 8 * 1024)
                {
                    more = true;
                    return false;
                }
                bytes += entry.payload.len() + 128;
                entries.push(entry);
                true
            },
        )?;
        if let Some(err) = failure {
            return Err(err);
        }
        let next_cursor = if more {
            entries.last().map_or(cursor, |e| e.seq)
        } else {
            head
        };
        if !more && !conn.live {
            conn.live = true;
            ws.serialize_attachment(&conn)?;
        }
        Ok(ServerFrame::Updates {
            req_id,
            done: !more,
            next_cursor,
            head,
            entries,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_snapshot(
        &self,
        conn: &Conn,
        req_id: u32,
        client_id: &[u8; 16],
        doc_id: &str,
        upto: u64,
        payload: Vec<u8>,
        now: u64,
    ) -> worker::Result<ServerFrame> {
        let outcome = |outcome| Ok(ServerFrame::SnapshotAck { req_id, outcome });
        // A retry of a snapshot that was stored already.
        if self
            .db
            .one(
                "SELECT 1 FROM updates WHERE client_id = ?",
                vec![b(client_id)],
            )?
            .is_some()
        {
            return outcome(SnapshotOutcome::Accepted { dropped: 0 });
        }
        if upto > self.head()? {
            return outcome(SnapshotOutcome::Rejected);
        }
        let covered = self
            .db
            .one(
                "SELECT COALESCE(MAX(upto), 0) FROM updates WHERE doc_id = ? AND kind = 1 AND payload IS NOT NULL",
                vec![s(doc_id)],
            )?
            .map_or(Ok(0), |row| uint(&row, 0))?;
        if upto <= covered {
            return outcome(SnapshotOutcome::Stale);
        }
        let row = self
            .db
            .query(
                "INSERT INTO updates (client_id, doc_id, device_id, kind, upto, payload, at)
                 VALUES (?, ?, ?, 1, ?, ?, ?) RETURNING seq",
                vec![
                    b(client_id),
                    s(doc_id),
                    s(&conn.device),
                    n(upto),
                    b(&payload),
                    n(now),
                ],
            )?
            .into_iter()
            .next()
            .ok_or_else(|| worker::Error::RustError("snapshot insert returned no row".into()))?;
        let snapshot_seq = uint(&row, 0)?;
        // The new snapshot replaces every older snapshot of the document and
        // every update it covers.
        let released = self.db.query(
            "UPDATE updates SET payload = NULL
             WHERE doc_id = ? AND payload IS NOT NULL AND seq <> ? AND (kind = 1 OR seq <= ?)
             RETURNING kind",
            vec![s(doc_id), n(snapshot_seq), n(upto)],
        )?;
        let dropped = released
            .iter()
            .filter(|row| db::int(row, 0).is_ok_and(|k| k == 0))
            .count();
        self.db.execute(
            "DELETE FROM updates WHERE seq IN (
                SELECT seq FROM updates WHERE payload IS NULL AND at < ?
                  AND seq < (SELECT MAX(seq) FROM updates)
                ORDER BY at LIMIT ?)",
            vec![
                n(now.saturating_sub(TOMBSTONE_RETENTION_MS)),
                n(TOMBSTONE_PURGE_BATCH),
            ],
        )?;
        outcome(SnapshotOutcome::Accepted {
            dropped: u32::try_from(dropped).unwrap_or(u32::MAX),
        })
    }
}
