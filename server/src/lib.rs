//! Note Everywhere sync server for Cloudflare Workers.
//!
//! Spike state: one Durable Object keeps an append-only log of opaque update
//! blobs in SQLite and relays them between connected devices over hibernating
//! WebSockets. There is no account or session check yet, so the sync route
//! stays disabled unless `ALLOW_UNAUTHENTICATED_SPIKE` is set for local runs.

use worker::*;

const WORKSPACE_BINDING: &str = "WORKSPACE";
const SPIKE_FLAG: &str = "ALLOW_UNAUTHENTICATED_SPIKE";
const MAX_UPDATE_BYTES: usize = 256 * 1024;

#[event(fetch, respond_with_errors)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match req.path().as_str() {
        "/health" => Response::ok("ok"),
        "/sync" => {
            let spike_enabled = env
                .var(SPIKE_FLAG)
                .map(|value| value.to_string() == "true")
                .unwrap_or(false);
            if !spike_enabled {
                return Response::error("sync is not available yet", 503);
            }
            let stub = env
                .durable_object(WORKSPACE_BINDING)?
                .get_by_name("default")?;
            stub.fetch_with_request(req).await
        }
        _ => Response::error("not found", 404),
    }
}

/// Holds the encrypted update log for one workspace and fans updates out to
/// every connected device.
#[durable_object]
pub struct Workspace {
    state: State,
}

impl Workspace {
    fn ensure_schema(&self) -> Result<()> {
        self.state.storage().sql().exec(
            "CREATE TABLE IF NOT EXISTS updates (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                payload BLOB NOT NULL
            )",
            None,
        )?;
        Ok(())
    }

    fn append(&self, payload: Vec<u8>) -> Result<i64> {
        #[derive(serde::Deserialize)]
        struct Inserted {
            seq: i64,
        }
        let row: Inserted = self
            .state
            .storage()
            .sql()
            .exec(
                "INSERT INTO updates (payload) VALUES (?) RETURNING seq",
                vec![SqlStorageValue::Blob(payload)],
            )?
            .one()?;
        Ok(row.seq)
    }
}

impl DurableObject for Workspace {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let is_upgrade = req
            .headers()
            .get("Upgrade")?
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
        if !is_upgrade {
            return Response::error("expected a WebSocket upgrade", 426);
        }
        self.ensure_schema()?;
        let pair = WebSocketPair::new()?;
        // Accepting through the state (not `server.accept()`) lets the object
        // hibernate between messages, so idle connections cost no duration.
        self.state.accept_web_socket(&pair.server);
        Response::from_websocket(pair.client)
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let WebSocketIncomingMessage::Binary(payload) = message else {
            return ws.send_with_str("error: updates must be binary frames");
        };
        if payload.len() > MAX_UPDATE_BYTES {
            return ws.send_with_str("error: update too large");
        }
        let seq = self.append(payload.clone())?;
        for peer in self.state.get_websockets() {
            if peer != ws {
                peer.send_with_bytes(&payload)?;
            }
        }
        ws.send_with_str(format!("ack {seq}"))
    }

    async fn websocket_close(
        &self,
        _ws: WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, _error: Error) -> Result<()> {
        Ok(())
    }
}
