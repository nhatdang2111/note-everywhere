//! Staging area for objects too big for one log row.
//!
//! A client that must sync an update or snapshot above the row limit seals it
//! in independent parts of at most 1 MiB, uploads them here one request each,
//! and then publishes the object with a single `Commit` frame. Only that frame
//! creates a log row, and the server refuses it unless every part exists, so a
//! receiver either sees the whole object or nothing. Parts are keyed by the
//! group (the log row's `client_id`) and index; storing is `INSERT OR IGNORE`,
//! so retries and resumed uploads are harmless. Parts of a compacted row are
//! deleted with it; parts that never got committed are swept after a day.
//!
//! The table is `WITHOUT ROWID`, so each part costs one row write.

use ne_sync_protocol::frame::CLIENT_ID_LEN;
use ne_sync_protocol::ids::hex_decode;
use ne_sync_protocol::limits::{MAX_PART_PAYLOAD, MAX_PARTS};
use worker::{Method, Request, Response};

use crate::db::{b, int, n};
use crate::http::{ApiError, ApiResult, content_length, json};
use crate::sessions;
use crate::workspace_do::Workspace;

/// Staged parts without a live log row are dropped after this long.
const ORPHAN_TTL_MS: u64 = 24 * 3600 * 1000;

impl Workspace {
    /// `rest` is `<group hex>` or `<group hex>/<index>`.
    pub(crate) async fn part_request(
        &self,
        method: Method,
        mut req: Request,
        rest: &str,
        now: u64,
    ) -> ApiResult<Response> {
        let session = self.authenticate(&req, now)?;
        let (group_hex, index) = match rest.split_once('/') {
            Some((group, index)) => (group, Some(index)),
            None => (rest, None),
        };
        let group: [u8; CLIENT_ID_LEN] = hex_decode(group_hex).ok_or(ApiError::NotFound)?;
        let index = index
            .map(|text| {
                text.parse::<usize>()
                    .ok()
                    .filter(|i| *i < MAX_PARTS)
                    .ok_or(ApiError::NotFound)
            })
            .transpose()?;
        match (method, index) {
            (Method::Get, None) => {
                // Which parts exist, so an interrupted upload resumes with the
                // missing ones only.
                let have = self
                    .db
                    .query(
                        "SELECT idx FROM parts WHERE grp = ? ORDER BY idx",
                        vec![b(&group)],
                    )?
                    .iter()
                    .map(|row| int(row, 0))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(json(200, &serde_json::json!({ "have": have }))?)
            }
            (Method::Get, Some(index)) => self.get_part(&group, index),
            (Method::Put, Some(index)) => {
                sessions::touch(&self.db, &session.id, now, self.cfg.touch_ms)?;
                self.put_part(&mut req, &group, index, now).await
            }
            _ => Err(ApiError::NotFound),
        }
    }

    fn get_part(&self, group: &[u8; CLIENT_ID_LEN], index: usize) -> ApiResult<Response> {
        let row = self
            .db
            .one(
                "SELECT payload FROM parts WHERE grp = ? AND idx = ?",
                vec![b(group), n(index as u64)],
            )?
            .ok_or(ApiError::NotFound)?;
        let payload = crate::db::blob(&row, 0)?;
        let mut response = Response::from_bytes(payload)?;
        let headers = response.headers_mut();
        headers.set("Content-Type", "application/octet-stream")?;
        headers.set("Cache-Control", "no-store")?;
        Ok(response)
    }

    async fn put_part(
        &self,
        req: &mut Request,
        group: &[u8; CLIENT_ID_LEN],
        index: usize,
        now: u64,
    ) -> ApiResult<Response> {
        match content_length(req) {
            None => return Err(ApiError::LengthRequired),
            Some(0) => return Err(ApiError::BadRequest),
            Some(len) if len as usize > MAX_PART_PAYLOAD => return Err(ApiError::TooLarge),
            Some(_) => {}
        }
        // The size limit applies to the bytes actually received, not only to
        // the declared length.
        let bytes = req.bytes().await?;
        if bytes.is_empty() {
            return Err(ApiError::BadRequest);
        }
        if bytes.len() > MAX_PART_PAYLOAD {
            return Err(ApiError::TooLarge);
        }
        if index == 0 {
            self.sweep_orphan_parts(now)?;
        }
        let stored = self.db.query(
            "INSERT OR IGNORE INTO parts (grp, idx, payload, at) VALUES (?, ?, ?, ?) RETURNING idx",
            vec![b(group), n(index as u64), b(&bytes), n(now)],
        )?;
        let status = if stored.is_empty() { 200 } else { 201 };
        Ok(json(
            status,
            &serde_json::json!({ "stored": status == 201 }),
        )?)
    }

    /// Drops staged parts that no live log row refers to and that are old
    /// enough that their upload cannot still be running.
    fn sweep_orphan_parts(&self, now: u64) -> ApiResult<()> {
        self.db.execute(
            "DELETE FROM parts WHERE at < ? AND NOT EXISTS (
                SELECT 1 FROM updates u WHERE u.client_id = parts.grp AND u.payload IS NOT NULL)",
            vec![n(now.saturating_sub(ORPHAN_TTL_MS))],
        )?;
        Ok(())
    }

    /// True when parts `0..count` all exist for the group.
    pub(crate) fn parts_complete(
        &self,
        group: &[u8; CLIENT_ID_LEN],
        count: u16,
    ) -> worker::Result<bool> {
        let have = self
            .db
            .one(
                "SELECT COUNT(*) FROM parts WHERE grp = ? AND idx < ?",
                vec![b(group), n(u64::from(count))],
            )?
            .map_or(Ok(0), |row| int(&row, 0))?;
        Ok(have == i64::from(count))
    }
}
