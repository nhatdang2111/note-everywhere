//! Note Everywhere sync server for Cloudflare Workers.
//!
//! A front Worker validates and routes; one SQLite-backed Durable Object keeps
//! all state. Everything stored is ciphertext or opaque to the server: it never
//! receives a password, a note, or an unwrapped key.

mod account;
mod blobs;
mod config;
mod db;
mod front;
mod http;
mod random;
mod sessions;
mod sync;
mod testing;
mod throttle;
mod workspace_do;

use worker::*;

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    match front::handle(req, env).await {
        Ok(response) => Ok(response),
        Err(err) => {
            // Never echo internals to the caller.
            console_error!("request failed: {err}");
            http::ApiError::Unavailable.into_response()
        }
    }
}
