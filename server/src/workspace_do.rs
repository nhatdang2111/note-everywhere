//! The single Durable Object: account, sessions, key records, the encrypted
//! update log, snapshots and the attachment index all live in its SQLite
//! database. The front Worker forwards each request here with one call.

use ne_sync_protocol::ids::hex_decode;
use worker::*;

use crate::config::Config;
use crate::db::Db;
use crate::http::{ApiError, ApiResult, bearer_token, client_ip, no_content};
use crate::sessions::{self, Session};
use crate::testing::EventKind;

#[durable_object]
pub struct Workspace {
    pub(crate) state: State,
    pub(crate) env: Env,
    pub(crate) db: Db,
    pub(crate) cfg: Config,
}

impl Workspace {
    pub(crate) fn authenticate(&self, req: &Request, now: u64) -> ApiResult<Session> {
        let token = bearer_token(req).ok_or(ApiError::Unauthorized)?;
        sessions::authenticate(&self.db, &token, now)?.ok_or(ApiError::Unauthorized)
    }

    /// Closes every WebSocket that belongs to one of the sessions.
    pub(crate) fn close_session_sockets(&self, session_ids: &[String]) {
        for id in session_ids {
            for ws in self.state.get_websockets_with_tag(id) {
                // A socket that is already gone is exactly what we want.
                let _ = ws.close(Some(4001), Some("session ended"));
            }
        }
    }

    async fn route(&self, mut req: Request) -> ApiResult<Response> {
        let now = Date::now().as_millis();
        let ip = client_ip(&req);
        let path = req.path();
        match (req.method(), path.as_str()) {
            (Method::Post, "/v1/account/salt") => self.salt(&mut req).await,
            (Method::Post, "/v1/account/register") => self.register(&mut req, &ip, now).await,
            (Method::Post, "/v1/account/recovery") => self.recovery_lookup(&mut req).await,
            (Method::Post, "/v1/account/recover") => self.recover(&mut req, &ip, now).await,
            (Method::Post, "/v1/session/login") => self.login(&mut req, &ip, now).await,
            (Method::Post, "/v1/session/logout") => self.logout(&req, now),
            (Method::Get, "/v1/sessions") => self.list_sessions(&req, now),
            (Method::Delete, p) if p.starts_with("/v1/sessions/") => {
                self.revoke_session(&req, &p["/v1/sessions/".len()..], now)
            }
            (Method::Post, "/v1/account/password") => {
                self.change_password(&mut req, &ip, now).await
            }
            (Method::Get, "/v1/account/record") => self.account_record(&req, now),
            (Method::Put, "/v1/vault") => self.put_vault(&mut req, now).await,
            (Method::Get, "/v1/vault") => self.get_vault(&req, now),
            (Method::Put, "/v1/workspace-key") => self.put_workspace_key(&mut req, now).await,
            (Method::Get, "/v1/workspace-key") => self.get_workspace_key(&req, now),
            (Method::Get, "/v1/sync") => self.accept_sync(&req, now),
            (method, p) if p.starts_with("/v1/parts/") => {
                self.part_request(method, req, &p["/v1/parts/".len()..], now)
                    .await
            }
            (method, p) if p.starts_with("/v1/blobs/") => {
                let hash = hex_decode::<32>(&p["/v1/blobs/".len()..]).ok_or(ApiError::NotFound)?;
                self.blob_request(method, req, hash, now).await
            }
            (method, p) if self.cfg.test_inspect && p.starts_with("/v1/_test/") => {
                self.test_route(method, &p["/v1/_test/".len()..]).await
            }
            _ => Err(ApiError::NotFound),
        }
    }
}

impl DurableObject for Workspace {
    fn new(state: State, env: Env) -> Self {
        // The schema statements are idempotent. If they fail the storage is
        // unusable and the object has to be reset, so there is nothing better
        // to do than stop.
        let db = Db::open(state.storage().sql()).expect("durable object schema");
        // Answered by the runtime without waking the object.
        let pair = worker_sys::WebSocketRequestResponsePair::new("ping", "pong")
            .expect("auto-response pair");
        state.set_websocket_auto_response(&pair);
        let cfg = Config::from_env(&env);
        let object = Self {
            state,
            env,
            db,
            cfg,
        };
        object.record_wake();
        object
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let response = match self.route(req).await {
            Ok(response) => response,
            Err(err) => err.into_response()?,
        };
        self.record_event(EventKind::Fetch);
        Ok(response)
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        self.on_message(ws, message);
        self.record_event(EventKind::Message);
        Ok(())
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        // Completes the closing handshake; the peer may already be gone.
        let _ = ws.close(Some(1000), Some("bye"));
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, _error: Error) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn ok_empty() -> ApiResult<Response> {
    Ok(no_content()?)
}
