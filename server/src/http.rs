//! Request helpers and the error model of the HTTP API.
//!
//! Error bodies carry only a stable code, never internal detail.

use ne_sync_protocol::limits::MAX_JSON_BODY;
use ne_sync_protocol::wire::{ErrorBody, codes};
use serde::Serialize;
use serde::de::DeserializeOwned;
use worker::{Headers, Request, Response, Result, console_error};

#[derive(Debug)]
pub enum ApiError {
    BadRequest,
    Unauthorized,
    InvalidCredentials,
    Forbidden,
    RegistrationClosed,
    NotFound,
    /// Too many failed attempts; carries the seconds until the next try.
    Locked(u64),
    TooLarge,
    LengthRequired,
    Quota,
    Unavailable,
    /// The account predates recovery verification and cannot be recovered
    /// through the server.
    RecoveryUnavailable,
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

impl ApiError {
    fn status(&self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::Unauthorized | Self::InvalidCredentials => 401,
            Self::Forbidden | Self::RegistrationClosed => 403,
            Self::NotFound => 404,
            Self::LengthRequired => 411,
            Self::TooLarge => 413,
            Self::Locked(_) => 429,
            Self::Quota => 507,
            Self::Unavailable => 503,
            Self::RecoveryUnavailable => 409,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest => codes::BAD_REQUEST,
            Self::Unauthorized => codes::UNAUTHORIZED,
            Self::InvalidCredentials => codes::INVALID_CREDENTIALS,
            Self::Forbidden => codes::FORBIDDEN,
            Self::RegistrationClosed => codes::REGISTRATION_CLOSED,
            Self::NotFound => codes::NOT_FOUND,
            Self::Locked(_) => codes::LOCKED,
            Self::TooLarge => codes::TOO_LARGE,
            Self::LengthRequired => codes::LENGTH_REQUIRED,
            Self::Quota => codes::QUOTA,
            Self::Unavailable => codes::UNAVAILABLE,
            Self::RecoveryUnavailable => codes::RECOVERY_UNAVAILABLE,
        }
    }

    pub fn into_response(self) -> Result<Response> {
        let retry_after = match self {
            Self::Locked(secs) => Some(secs),
            _ => None,
        };
        let body = ErrorBody {
            error: self.code().to_owned(),
            retry_after,
        };
        let mut response = Response::from_json(&body)?.with_status(self.status());
        no_store(response.headers_mut())?;
        if let Some(secs) = retry_after {
            response
                .headers_mut()
                .set("Retry-After", &secs.to_string())?;
        }
        Ok(response)
    }
}

/// Internal failures (storage, runtime) are logged without request data and
/// reported as a retryable "unavailable".
impl From<worker::Error> for ApiError {
    fn from(err: worker::Error) -> Self {
        console_error!("internal error: {err}");
        Self::Unavailable
    }
}

pub fn no_store(headers: &mut Headers) -> Result<()> {
    headers.set("Cache-Control", "no-store")
}

pub fn json<T: Serialize>(status: u16, body: &T) -> Result<Response> {
    let mut response = Response::from_json(body)?.with_status(status);
    no_store(response.headers_mut())?;
    Ok(response)
}

pub fn no_content() -> Result<Response> {
    let mut response = Response::empty()?.with_status(204);
    no_store(response.headers_mut())?;
    Ok(response)
}

pub fn content_length(req: &Request) -> Option<u64> {
    req.headers()
        .get("Content-Length")
        .ok()??
        .trim()
        .parse()
        .ok()
}

/// Reads and parses a JSON body, enforcing the size limit on the bytes
/// actually received (not only on the declared length).
pub async fn read_json<T: DeserializeOwned>(req: &mut Request) -> ApiResult<T> {
    match content_length(req) {
        Some(len) if len as usize > MAX_JSON_BODY => return Err(ApiError::TooLarge),
        None => return Err(ApiError::LengthRequired),
        Some(_) => {}
    }
    let bytes = req.bytes().await?;
    if bytes.len() > MAX_JSON_BODY {
        return Err(ApiError::TooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| ApiError::BadRequest)
}

pub fn bearer_token(req: &Request) -> Option<String> {
    let header = req.headers().get("Authorization").ok()??;
    let token = header.strip_prefix("Bearer ")?.trim();
    // 32 random bytes as unpadded base64 is 43 characters.
    (token.len() == 43).then(|| token.to_owned())
}

/// The caller's address as set by Cloudflare's edge, which clients cannot forge.
pub fn client_ip(req: &Request) -> String {
    req.headers()
        .get("CF-Connecting-IP")
        .ok()
        .flatten()
        .filter(|ip| ip.len() <= 64)
        .unwrap_or_else(|| "unknown".to_owned())
}
