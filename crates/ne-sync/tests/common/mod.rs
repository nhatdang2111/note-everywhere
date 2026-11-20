//! Shared helpers for the integration tests that run against a local server
//! (`wrangler dev`). They are skipped unless `NE_SYNC_TEST_URL` is set.
//!
//! The server keeps one account for good, so the tests share it one at a time:
//! every test takes a process-wide lock and wipes the server through the
//! test-only reset route first.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ne_crypto::account::AccountSecrets;
use ne_crypto::password::{KdfParams, KdfPolicy};
use ne_sync::{
    Backoff, ClientConfig, IncomingKind, MemoryStore, SignUp, SyncClient, SyncEngine, SyncStatus,
    Unlocked,
};
use tokio::sync::{Mutex, MutexGuard};
use url::Url;

mod proxy;
mod raw;
#[allow(unused_imports)]
pub use proxy::Proxy;
#[allow(unused_imports)]
pub use raw::RawSocket;

pub const USERNAME: &str = "alice@example.test";
pub const PASSWORD: &str = "correct horse battery staple";
pub const USER_AGENT: &str = "NoteEverywhere/0.0 (integration test)";

static SERIAL: Mutex<()> = Mutex::const_new(());

/// Cheap Argon2 settings so tests stay fast; real accounts use the defaults.
pub const TEST_KDF: KdfParams = KdfParams {
    memory_kib: 8,
    iterations: 1,
    parallelism: 1,
};

pub fn server_url() -> Option<Url> {
    std::env::var("NE_SYNC_TEST_URL")
        .ok()
        .and_then(|v| Url::parse(&v).ok())
}

pub fn setup_code() -> String {
    std::env::var("NE_SYNC_TEST_SETUP_CODE").unwrap_or_else(|_| "local-test-setup-code".into())
}

/// Returns the server URL, or prints why the test is skipped and returns early.
#[macro_export]
macro_rules! require_server {
    () => {
        match $crate::common::server_url() {
            Some(url) => url,
            None => {
                eprintln!("skipped: set NE_SYNC_TEST_URL to a local server (see server/README.md)");
                return;
            }
        }
    };
}

pub struct Harness {
    pub url: Url,
    _lock: MutexGuard<'static, ()>,
}

pub fn raw_http() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("http client")
}

/// Takes the lock and wipes the server.
pub async fn fresh(url: Url) -> Harness {
    let lock = SERIAL.lock().await;
    let response = raw_http()
        .post(url.join("/v1/_test/reset").expect("url"))
        .send()
        .await
        .expect("reset request");
    assert_eq!(
        response.status(),
        204,
        "server must run with TEST_INSPECT=1"
    );
    Harness { url, _lock: lock }
}

impl Harness {
    pub fn config(&self, label: &str) -> ClientConfig {
        let mut cfg = ClientConfig::new(self.url.clone(), label);
        cfg.kdf_params = TEST_KDF;
        cfg.kdf_policy = KdfPolicy::LENIENT;
        cfg
    }

    pub fn client(&self, label: &str) -> SyncClient {
        SyncClient::new(self.config(label)).expect("client")
    }

    /// Creates the account and returns the signed-in first device.
    pub async fn sign_up(&self) -> (SyncClient, SignUp) {
        let client = self.client("first device");
        let signup = client
            .sign_up(&setup_code(), USERNAME, PASSWORD)
            .await
            .expect("sign up");
        (client, signup)
    }

    /// Signs in another device with the account's password.
    pub async fn login(&self, label: &str) -> (SyncClient, Unlocked) {
        let client = self.client(label);
        let unlocked = client.login(USERNAME, PASSWORD).await.expect("login");
        (client, unlocked)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        raw_http()
            .get(self.url.join(path).expect("url"))
            .send()
            .await
            .expect("request")
    }

    /// Every byte the server stores (test-only route).
    pub async fn dump(&self) -> Vec<u8> {
        self.get("/v1/_test/dump")
            .await
            .bytes()
            .await
            .expect("dump")
            .to_vec()
    }

    pub async fn stats(&self) -> serde_json::Value {
        self.get("/v1/_test/stats")
            .await
            .json()
            .await
            .expect("stats")
    }
}

/// Engine settings tuned for tests: short batching window, fast backoff.
pub fn start_engine(
    client: &SyncClient,
    secrets: &AccountSecrets,
    device: &str,
    store: &Arc<MemoryStore>,
    via: Option<&Url>,
) -> SyncEngine {
    let mut cfg = client.engine_config(device).expect("engine config");
    cfg.batch_delay = Duration::from_millis(40);
    cfg.backoff = Backoff {
        base: Duration::from_millis(50),
        max: Duration::from_millis(400),
    };
    if let Some(url) = via {
        cfg.server = url.clone();
    }
    SyncEngine::start(cfg, &secrets.workspace_key, store.clone()).expect("engine")
}

/// Polls `condition` until it holds or `timeout` passes.
pub async fn wait_for<F, Fut>(what: &str, timeout: Duration, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub async fn wait_synced(engine: &SyncEngine) {
    wait_for("synced status", Duration::from_secs(20), || async {
        engine.status() == SyncStatus::Synced
    })
    .await;
}

/// The document model used by the tests: a document is a set of numbers, an
/// update adds one number, a snapshot is the whole set. Merging is a union, so
/// like a CRDT it is order independent and idempotent.
pub fn update_of(n: u32) -> Vec<u8> {
    n.to_le_bytes().to_vec()
}

pub fn snapshot_of(set: &BTreeSet<u32>) -> Vec<u8> {
    set.iter().flat_map(|n| n.to_le_bytes()).collect()
}

fn numbers(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|word| u32::from_le_bytes(*word)).collect()
}

/// The state of one document on a device: its own numbers plus everything applied.
pub fn state_of(store: &MemoryStore, doc: &str, own: &BTreeSet<u32>) -> BTreeSet<u32> {
    let mut state = own.clone();
    for update in store.applied().iter().filter(|u| u.doc_id == doc) {
        state.extend(numbers(&update.plaintext));
    }
    state
}

/// How many log rows of a kind a store has applied.
pub fn applied_count(store: &MemoryStore, snapshots: bool) -> usize {
    store
        .applied()
        .iter()
        .filter(|u| matches!(u.kind, IncomingKind::Snapshot { .. }) == snapshots)
        .count()
}

/// Socket address of the test server, for pointing a proxy at it.
pub async fn server_addr(url: &Url) -> std::net::SocketAddr {
    let host = url.host_str().expect("host");
    let port = url.port_or_known_default().expect("port");
    tokio::net::lookup_host((host, port))
        .await
        .expect("resolve")
        .next()
        .expect("address")
}
