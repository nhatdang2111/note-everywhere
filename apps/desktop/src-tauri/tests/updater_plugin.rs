//! The real Tauri updater plugin behind `TauriUpdater`, against a local HTTP
//! server and a throwaway key pair made for each test run. The private half
//! only ever exists in memory here; nothing is written to disk and nothing is
//! installed (downloading verifies the package, installing is a separate step
//! that these tests never take).

// Not run on Windows for the same reason as `ipc.rs`: a test executable that
// links the Tauri runtime has no application manifest there and fails to load.
#![cfg(all(desktop, not(windows)))]

use std::collections::HashMap;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign::KeyPair;
use note_everywhere_desktop_lib::app::{ProgressFn, UpdateBackend, UpdateFailure};
use note_everywhere_desktop_lib::updater::{TauriUpdater, updater_configured};
use serde_json::{Value, json};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const PACKAGE: &[u8] = b"synthetic update package, not an application";
/// Newer than any version this crate can have.
const OFFERED: &str = "999.0.0";

type Routes = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// Serves fixed bodies on 127.0.0.1, in a few writes so a download reports
/// progress more than once.
async fn serve(routes: Routes) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&buffer[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let path = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let body = routes.lock().expect("lock").get(&path).cloned();
                let Some(body) = body else {
                    let _ = stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                for part in body.chunks(body.len().div_ceil(3).max(1)) {
                    let _ = stream.write_all(part).await;
                    let _ = stream.flush().await;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            });
        }
    });
    addr
}

fn sign(pair: &KeyPair, data: &[u8], version: &str) -> String {
    let comment = format!("timestamp:1\tfile:package\tversion:{version}");
    let signature = minisign::sign(
        Some(&pair.pk),
        &pair.sk,
        Cursor::new(data),
        Some(&comment),
        Some("throwaway test signature"),
    )
    .expect("sign");
    STANDARD.encode(signature.to_string())
}

fn public_key(pair: &KeyPair) -> String {
    STANDARD.encode(pair.pk.to_box().expect("box").to_string())
}

fn manifest(addr: SocketAddr, version: &str, signature: &str) -> Vec<u8> {
    let entry = json!({ "signature": signature, "url": format!("http://{addr}/package") });
    let platforms: serde_json::Map<String, Value> = [
        "darwin-aarch64",
        "darwin-x86_64",
        "windows-x86_64",
        "windows-aarch64",
        "linux-x86_64",
        "linux-aarch64",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), entry.clone()))
    .collect();
    serde_json::to_vec(&json!({
        "version": version,
        "notes": "Synthetic release.",
        "pub_date": "2030-01-01T00:00:00.123Z",
        "platforms": platforms,
    }))
    .expect("json")
}

/// A mock app whose updater plugin trusts `pubkey`, wired the way the real
/// app wires it: the key sits in `plugins.updater.pubkey` of the configuration.
fn updater(pubkey: &str, endpoint: &str) -> (tauri::App<MockRuntime>, TauriUpdater<MockRuntime>) {
    let mut context = mock_context(noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), json!({ "pubkey": pubkey }));
    assert!(updater_configured(context.config()));
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .expect("app");
    let backend = TauriUpdater::new(app.handle().clone(), true)
        .with_endpoint(Url::parse(endpoint).expect("url"));
    (app, backend)
}

struct Setup {
    pair: KeyPair,
    routes: Routes,
    addr: SocketAddr,
}

async fn setup() -> Setup {
    let routes: Routes = Arc::default();
    let addr = serve(routes.clone()).await;
    Setup {
        pair: KeyPair::generate_unencrypted_keypair().expect("keys"),
        routes,
        addr,
    }
}

impl Setup {
    fn publish(&self, package: &[u8], signature: &str, version: &str) {
        let mut routes = self.routes.lock().expect("lock");
        routes.insert("/package".into(), package.to_vec());
        routes.insert(
            "/latest.json".into(),
            manifest(self.addr, version, signature),
        );
    }

    fn endpoint(&self) -> String {
        format!("http://{}/latest.json", self.addr)
    }
}

type Reports = Arc<Mutex<Vec<(u64, Option<u64>)>>>;

fn recorder() -> (ProgressFn, Reports) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let progress: ProgressFn = Arc::new(move |received, total| {
        sink.lock().expect("lock").push((received, total));
    });
    (progress, seen)
}

#[tokio::test]
async fn a_signed_package_is_found_downloaded_with_progress_and_verified() {
    let s = setup().await;
    s.publish(PACKAGE, &sign(&s.pair, PACKAGE, OFFERED), OFFERED);
    let (_app, backend) = updater(&public_key(&s.pair), &s.endpoint());

    let offer = backend.check().await.expect("check").expect("an update");
    assert_eq!(offer.version, OFFERED);
    assert_eq!(offer.notes.as_deref(), Some("Synthetic release."));

    let (progress, seen) = recorder();
    backend.download(progress).await.expect("download");
    let seen = seen.lock().expect("lock").clone();
    let total = u64::try_from(PACKAGE.len()).expect("fits");
    assert!(seen.len() >= 2, "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0].0 < w[1].0), "{seen:?}");
    assert_eq!(seen.last(), Some(&(total, Some(total))));
}

#[tokio::test]
async fn a_manifest_that_is_not_newer_is_no_update() {
    let s = setup().await;
    s.publish(PACKAGE, &sign(&s.pair, PACKAGE, "0.0.1"), "0.0.1");
    let (_app, backend) = updater(&public_key(&s.pair), &s.endpoint());
    assert_eq!(backend.check().await.expect("check"), None);
    // With nothing found there is nothing to download or install.
    let (progress, _) = recorder();
    assert_eq!(backend.download(progress).await, Err(UpdateFailure::Other));
    assert_eq!(backend.install(), Err(UpdateFailure::Other));
}

#[tokio::test]
async fn a_package_changed_after_signing_is_refused() {
    let s = setup().await;
    let signature = sign(&s.pair, PACKAGE, OFFERED);
    s.publish(b"a different package", &signature, OFFERED);
    let (_app, backend) = updater(&public_key(&s.pair), &s.endpoint());
    backend.check().await.expect("check").expect("an update");
    let (progress, _) = recorder();
    assert_eq!(
        backend.download(progress).await,
        Err(UpdateFailure::BadSignature)
    );
    // Nothing verified was kept, so installing is refused too.
    assert_eq!(backend.install(), Err(UpdateFailure::Other));
}

#[tokio::test]
async fn a_package_signed_with_another_key_is_refused() {
    let s = setup().await;
    let attacker = KeyPair::generate_unencrypted_keypair().expect("keys");
    s.publish(PACKAGE, &sign(&attacker, PACKAGE, OFFERED), OFFERED);
    let (_app, backend) = updater(&public_key(&s.pair), &s.endpoint());
    backend.check().await.expect("check").expect("an update");
    let (progress, _) = recorder();
    assert_eq!(
        backend.download(progress).await,
        Err(UpdateFailure::BadSignature)
    );
}

#[tokio::test]
async fn a_signature_made_for_another_version_is_refused() {
    let s = setup().await;
    // An old, genuine package paired with a manifest that announces a newer version.
    s.publish(PACKAGE, &sign(&s.pair, PACKAGE, "0.0.1"), OFFERED);
    let (_app, backend) = updater(&public_key(&s.pair), &s.endpoint());
    backend.check().await.expect("check").expect("an update");
    let (progress, _) = recorder();
    assert_eq!(
        backend.download(progress).await,
        Err(UpdateFailure::BadSignature)
    );
}

#[tokio::test]
async fn an_endpoint_that_cannot_be_reached_is_reported_as_unreachable() {
    let s = setup().await;
    // Nothing listens on this port: bind and drop to learn a free one.
    let free = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dead = free.local_addr().expect("addr");
    drop(free);
    let (_app, backend) = updater(&public_key(&s.pair), &format!("http://{dead}/latest.json"));
    assert_eq!(backend.check().await, Err(UpdateFailure::Unreachable));

    // A server that has no manifest answers 404.
    let (_app, backend) = updater(&public_key(&s.pair), &s.endpoint());
    assert_eq!(backend.check().await, Err(UpdateFailure::Unreachable));
}

#[test]
fn the_shipped_configuration_has_no_key_so_a_plain_build_cannot_update() {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
    )
    .expect("config");
    let config: Value = serde_json::from_str(&raw).expect("json");
    assert!(
        config.pointer("/plugins/updater").is_none(),
        "the key is supplied at release time, never committed"
    );
    let mut parsed = tauri::Config::default();
    assert!(!updater_configured(&parsed));
    parsed
        .plugins
        .0
        .insert("updater".into(), json!({ "pubkey": "  " }));
    assert!(!updater_configured(&parsed), "a blank key is no key");
    parsed
        .plugins
        .0
        .insert("updater".into(), json!({ "pubkey": "abc" }));
    assert!(updater_configured(&parsed));
}

#[test]
fn the_endpoint_is_the_latest_release_manifest_of_this_repository() {
    assert_eq!(
        note_everywhere_desktop_lib::updater::UPDATE_ENDPOINT,
        "https://github.com/nhatdang2111/note-everywhere/releases/latest/download/latest.json"
    );
}
