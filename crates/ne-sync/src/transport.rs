//! HTTP plumbing: the HTTP client, URL building, session token and the mapping
//! from server error bodies to [`SyncError`].

use std::sync::{Arc, RwLock};
use std::time::Duration;

use ne_sync_protocol::wire::{ErrorBody, codes};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;
use zeroize::Zeroizing;

use crate::error::{Result, SyncError};
use crate::tls;

/// Bearer token of a session. Wiped from memory on drop and never printed.
#[derive(Clone)]
pub struct SessionToken(Arc<Zeroizing<String>>);

impl SessionToken {
    pub fn new(token: String) -> Self {
        Self(Arc::new(Zeroizing::new(token)))
    }

    /// The raw token, for storing in the OS keychain.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

/// The session a client currently holds.
#[derive(Debug, Clone)]
pub struct SessionState {
    pub token: SessionToken,
    pub session_id: String,
    pub workspace_id: String,
    pub expires_at: u64,
}

/// `NoteEverywhere/<version> (<what>)`. workers.dev blocks default library
/// agents, and the server only accepts this prefix.
pub fn user_agent(extra: Option<&str>) -> String {
    let base = format!("NoteEverywhere/{} (sync client)", env!("CARGO_PKG_VERSION"));
    match extra {
        Some(extra) if !extra.is_empty() => format!("{base} {extra}"),
        _ => base,
    }
}

pub(crate) struct Transport {
    pub(crate) base: Url,
    pub(crate) http: reqwest::Client,
    pub(crate) user_agent: String,
    session: RwLock<Option<SessionState>>,
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl Transport {
    pub(crate) fn new(base: Url, user_agent: String) -> Result<Self> {
        if !matches!(base.scheme(), "http" | "https") {
            return Err(SyncError::Input("server URL must be http or https".into()));
        }
        let tls = tls::client_config()?;
        let http = reqwest::Client::builder()
            .user_agent(&user_agent)
            .use_preconfigured_tls((*tls).clone())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            base,
            http,
            user_agent,
            session: RwLock::new(None),
        })
    }

    pub(crate) fn session(&self) -> Option<SessionState> {
        self.session.read().ok()?.clone()
    }

    pub(crate) fn set_session(&self, session: Option<SessionState>) {
        if let Ok(mut slot) = self.session.write() {
            *slot = session;
        }
    }

    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        self.base
            .join(path)
            .map_err(|_| SyncError::Input("invalid server URL".into()))
    }

    pub(crate) fn request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .http
            .request(method, self.url(path)?)
            .timeout(REQUEST_TIMEOUT))
    }

    pub(crate) fn authed(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let session = self.session().ok_or(SyncError::Unauthorized)?;
        Ok(self
            .request(method, path)?
            .bearer_auth(session.token.expose()))
    }
}

/// Turns a non-success response into the matching error.
pub(crate) async fn check(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body: Option<ErrorBody> = response.json().await.ok();
    let code = body.as_ref().map_or("", |b| b.error.as_str()).to_owned();
    Err(match (status, code.as_str()) {
        (StatusCode::UNAUTHORIZED, codes::INVALID_CREDENTIALS) => SyncError::InvalidCredentials,
        (StatusCode::UNAUTHORIZED, _) => SyncError::Unauthorized,
        (StatusCode::TOO_MANY_REQUESTS, _) => {
            SyncError::Locked(body.and_then(|b| b.retry_after).unwrap_or(0))
        }
        (StatusCode::FORBIDDEN, codes::REGISTRATION_CLOSED) => SyncError::RegistrationClosed,
        (StatusCode::FORBIDDEN, _) => SyncError::Forbidden,
        (StatusCode::NOT_FOUND, _) => SyncError::NotFound,
        (StatusCode::PAYLOAD_TOO_LARGE, _) => SyncError::TooLarge,
        (StatusCode::INSUFFICIENT_STORAGE, _) => SyncError::QuotaExceeded,
        (s, _) if s.is_server_error() => SyncError::Unavailable,
        (s, _) => SyncError::Server {
            status: s.as_u16(),
            code,
        },
    })
}

pub(crate) async fn json<T: DeserializeOwned>(builder: RequestBuilder) -> Result<T> {
    let response = check(builder.send().await?).await?;
    response.json().await.map_err(|_| SyncError::BadResponse)
}

pub(crate) async fn unit(builder: RequestBuilder) -> Result<()> {
    check(builder.send().await?).await.map(drop)
}
