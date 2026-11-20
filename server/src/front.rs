//! The front Worker: cheap checks, then at most one call to the Durable Object.

use ne_sync_protocol::limits::MAX_JSON_BODY;
use worker::{Env, Method, Request, Response, Result};

use crate::config::Config;
use crate::http::{ApiError, content_length, json};

const OBJECT_BINDING: &str = "WORKSPACE";
const OBJECT_NAME: &str = "main";
const USER_AGENT_PREFIX: &str = "NoteEverywhere/";

fn header(req: &Request, name: &str) -> Option<String> {
    req.headers().get(name).ok().flatten()
}

/// Checks that need no state, so rejected requests never reach the object.
fn precheck(req: &Request, cfg: &Config) -> std::result::Result<(), ApiError> {
    let path = req.path();
    if !path.starts_with("/v1/") {
        return Err(ApiError::NotFound);
    }
    // CORS stays closed: browsers always send Origin, the native client never does.
    if header(req, "Origin").is_some() {
        return Err(ApiError::Forbidden);
    }
    let app_agent = header(req, "User-Agent")
        .is_some_and(|agent| agent.starts_with(USER_AGENT_PREFIX) && agent.len() <= 200);
    if !app_agent {
        return Err(ApiError::Forbidden);
    }
    if matches!(req.method(), Method::Post | Method::Put) {
        let limit = if path.starts_with("/v1/blobs/") {
            cfg.blob_max
        } else {
            MAX_JSON_BODY as u64
        };
        match content_length(req) {
            None => return Err(ApiError::LengthRequired),
            Some(len) if len > limit => return Err(ApiError::TooLarge),
            Some(_) => {}
        }
    }
    Ok(())
}

pub async fn handle(req: Request, env: Env) -> Result<Response> {
    let cfg = Config::from_env(&env);
    if let Err(err) = precheck(&req, &cfg) {
        return err.into_response();
    }
    if req.method() == Method::Get && req.path() == "/v1/health" {
        return json(200, &serde_json::json!({ "status": "ok" }));
    }
    env.durable_object(OBJECT_BINDING)?
        .get_by_name(OBJECT_NAME)?
        .fetch_with_request(req)
        .await
}
