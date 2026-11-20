//! Encrypted attachments in R2, addressed by the SHA-256 of their ciphertext.
//!
//! The Worker binds the bucket directly, so deployments need no S3
//! credentials. R2 itself verifies the checksum while it receives the stream,
//! which costs no Worker CPU; the object is therefore guaranteed to match the
//! address it was stored under.

use ne_sync_protocol::ids::hex_encode;
use worker::{Bucket, Data, Method, Request, Response};

use crate::db::{int, n, s, uint};
use crate::http::{ApiError, ApiResult, content_length, json, no_content};
use crate::workspace_do::Workspace;

const BUCKET_BINDING: &str = "BLOBS";
/// A reservation that never completed (the upload died) stops counting
/// against the quota after this long.
const PENDING_TTL_MS: u64 = 3600 * 1000;
const STATE_PENDING: u64 = 0;
const STATE_STORED: u64 = 1;

fn object_key(hex: &str) -> String {
    format!("blobs/{hex}")
}

impl Workspace {
    pub(crate) fn bucket(&self) -> worker::Result<Bucket> {
        self.env.bucket(BUCKET_BINDING)
    }

    fn stored_size(&self, hex: &str) -> ApiResult<Option<u64>> {
        let row = self.db.one(
            "SELECT size FROM blobs WHERE hash = ? AND state = ?",
            vec![s(hex), n(STATE_STORED)],
        )?;
        Ok(row.map(|row| uint(&row, 0)).transpose()?)
    }

    pub(crate) async fn blob_request(
        &self,
        method: Method,
        req: Request,
        hash: [u8; 32],
        now: u64,
    ) -> ApiResult<Response> {
        self.authenticate(&req, now)?;
        let hex = hex_encode(&hash);
        match method {
            Method::Put => self.put_blob(req, &hex, &hash, now).await,
            Method::Get => self.get_blob(&hex).await,
            Method::Head => {
                let size = self.stored_size(&hex)?.ok_or(ApiError::NotFound)?;
                let mut response = Response::empty()?;
                response
                    .headers_mut()
                    .set("X-Blob-Size", &size.to_string())?;
                Ok(response)
            }
            Method::Delete => {
                self.bucket()?.delete(object_key(&hex)).await?;
                self.db
                    .execute("DELETE FROM blobs WHERE hash = ?", vec![s(&hex)])?;
                Ok(no_content()?)
            }
            _ => Err(ApiError::NotFound),
        }
    }

    async fn put_blob(
        &self,
        req: Request,
        hex: &str,
        hash: &[u8; 32],
        now: u64,
    ) -> ApiResult<Response> {
        let length = content_length(&req).ok_or(ApiError::LengthRequired)?;
        if length == 0 {
            return Err(ApiError::BadRequest);
        }
        if length > self.cfg.blob_max {
            return Err(ApiError::TooLarge);
        }
        if self.stored_size(hex)?.is_some() {
            // Content-addressed, so the object is already exactly this.
            return Ok(json(200, &serde_json::json!({ "stored": false }))?);
        }
        self.db.execute(
            "DELETE FROM blobs WHERE state = ? AND created_at < ?",
            vec![n(STATE_PENDING), n(now.saturating_sub(PENDING_TTL_MS))],
        )?;
        let used = self
            .db
            .one("SELECT COALESCE(SUM(size), 0) FROM blobs", vec![])?
            .map_or(Ok(0), |row| int(&row, 0))?;
        if u64::try_from(used).unwrap_or(0).saturating_add(length) > self.cfg.blob_quota {
            return Err(ApiError::Quota);
        }
        // Reserve before streaming so concurrent uploads cannot overshoot the quota.
        self.db.execute(
            "INSERT OR REPLACE INTO blobs (hash, size, state, created_at) VALUES (?, ?, ?, ?)",
            vec![s(hex), n(length), n(STATE_PENDING), n(now)],
        )?;
        let Some(body) = req.inner().body() else {
            self.db
                .execute("DELETE FROM blobs WHERE hash = ?", vec![s(hex)])?;
            return Err(ApiError::BadRequest);
        };
        let stored = self
            .bucket()?
            .put(object_key(hex), Data::ReadableStream(body))
            .sha256(hash.to_vec())
            .execute()
            .await;
        if let Err(err) = stored {
            self.db
                .execute("DELETE FROM blobs WHERE hash = ?", vec![s(hex)])?;
            let text = err.to_string().to_ascii_lowercase();
            return Err(if text.contains("checksum") || text.contains("digest") {
                ApiError::BadRequest
            } else {
                worker::console_error!("blob upload failed: {err}");
                ApiError::Unavailable
            });
        }
        self.db.execute(
            "UPDATE blobs SET state = ? WHERE hash = ?",
            vec![n(STATE_STORED), s(hex)],
        )?;
        Ok(json(
            201,
            &serde_json::json!({ "stored": true, "size": length }),
        )?)
    }

    async fn get_blob(&self, hex: &str) -> ApiResult<Response> {
        let size = self.stored_size(hex)?.ok_or(ApiError::NotFound)?;
        let Some(object) = self.bucket()?.get(object_key(hex)).execute().await? else {
            // The index claims an object the bucket does not have.
            self.db
                .execute("DELETE FROM blobs WHERE hash = ?", vec![s(hex)])?;
            return Err(ApiError::NotFound);
        };
        let body = object
            .body()
            .ok_or(ApiError::Unavailable)?
            .response_body()?;
        let mut response = Response::from_body(body)?;
        let headers = response.headers_mut();
        headers.set("Content-Type", "application/octet-stream")?;
        headers.set("X-Blob-Size", &size.to_string())?;
        headers.set("Cache-Control", "no-store")?;
        Ok(response)
    }
}
