//! Routes and counters for local test runs.
//!
//! They exist only when the `TEST_INSPECT` variable is set to `1`, which the
//! committed configuration never does; any other deployment answers 404 for
//! `/v1/_test/*`. They let the integration tests wipe state between cases,
//! look at every byte the server stores, and read request and row counters.

use serde::{Deserialize, Serialize};
use worker::{Method, Response, SqlStorageValue};

use crate::db::{META_STATS, TABLES};
use crate::http::{ApiError, ApiResult, json, no_content};
use crate::workspace_do::Workspace;

#[derive(Clone, Copy)]
pub enum EventKind {
    Fetch,
    Message,
}

#[derive(Default, Serialize, Deserialize)]
struct Stats {
    /// HTTP requests and WebSocket upgrades that reached the object.
    fetches: u64,
    /// Binary frames received over WebSockets.
    messages: u64,
    rows_written: u64,
    rows_read: u64,
    /// Times the object was constructed, which is once per wake-up after
    /// hibernation or eviction.
    wakes: u64,
}

impl Workspace {
    /// Accumulates counters; a no-op unless test inspection is enabled.
    pub(crate) fn record_event(&self, kind: EventKind) {
        if !self.cfg.test_inspect {
            return;
        }
        let (written, read) = self.db.take_counters();
        let mut stats = self.stats().unwrap_or_default();
        match kind {
            EventKind::Fetch => stats.fetches += 1,
            EventKind::Message => stats.messages += 1,
        }
        stats.rows_written += written;
        stats.rows_read += read;
        if let Ok(text) = serde_json::to_string(&stats) {
            // Bookkeeping must not count toward what it measures.
            let _ = self.db.set_meta(META_STATS, &text);
            self.db.take_counters();
        }
    }

    /// Counts a construction of the object; a no-op unless test inspection is enabled.
    pub(crate) fn record_wake(&self) {
        if !self.cfg.test_inspect {
            return;
        }
        let mut stats = self.stats().unwrap_or_default();
        stats.wakes += 1;
        if let Ok(text) = serde_json::to_string(&stats) {
            let _ = self.db.set_meta(META_STATS, &text);
        }
        self.db.take_counters();
    }

    fn stats(&self) -> Option<Stats> {
        serde_json::from_str(&self.db.meta(META_STATS).ok()??).ok()
    }

    pub(crate) async fn test_route(&self, method: Method, name: &str) -> ApiResult<Response> {
        match (method, name) {
            (Method::Get, "stats") => Ok(json(200, &self.stats().unwrap_or_default())?),
            (Method::Get, "dump") => self.dump().await,
            (Method::Post, "reset") => self.reset().await,
            (Method::Post, "age-tombstones") => {
                // Makes every tombstone look older than its retention.
                self.db
                    .execute("UPDATE updates SET at = 0 WHERE payload IS NULL", vec![])?;
                Ok(no_content()?)
            }
            _ => Err(ApiError::NotFound),
        }
    }

    /// Every stored byte: all table rows and all attachment objects.
    async fn dump(&self) -> ApiResult<Response> {
        let mut out: Vec<u8> = Vec::new();
        for table in TABLES {
            for row in self.db.query(&format!("SELECT * FROM {table}"), vec![])? {
                for value in row {
                    match value {
                        SqlStorageValue::Blob(bytes) => out.extend_from_slice(&bytes),
                        SqlStorageValue::String(text) => out.extend_from_slice(text.as_bytes()),
                        other => out.extend_from_slice(format!("{other:?}").as_bytes()),
                    }
                    out.push(b'|');
                }
                out.push(b'\n');
            }
        }
        let bucket = self.bucket()?;
        for object in bucket.list().execute().await?.objects() {
            if let Some(found) = bucket.get(object.key()).execute().await?
                && let Some(body) = found.body()
            {
                out.extend_from_slice(&body.bytes().await?);
            }
        }
        Ok(Response::from_bytes(out)?)
    }

    /// Wipes the object and the bucket and drops every connection.
    async fn reset(&self) -> ApiResult<Response> {
        for ws in self.state.get_websockets() {
            let _ = ws.close(Some(4002), Some("reset"));
        }
        for table in TABLES {
            self.db.execute(&format!("DELETE FROM {table}"), vec![])?;
        }
        let bucket = self.bucket()?;
        let keys: Vec<String> = bucket
            .list()
            .execute()
            .await?
            .objects()
            .iter()
            .map(|o| o.key())
            .collect();
        if !keys.is_empty() {
            bucket.delete_multiple(keys).await?;
        }
        self.db.take_counters();
        Ok(no_content()?)
    }
}
