//! Simulates one day of typical use against a local server and reports the
//! requests and rows it costs next to the Cloudflare free-plan limits.
//!
//! Three devices make 300 edits each in typing bursts of 10 (so each burst is
//! one batched push), reconnect eight times (about once per working hour),
//! share ten attachments and compact one document each. Run with
//! `--nocapture` to see the table. Skipped without `NE_SYNC_TEST_URL`.
//!
//! CPU time per request cannot be measured locally; see the server README.

mod common;

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use ne_sync::{MemoryStore, SyncEngine};

use common::{Proxy, fresh, server_addr, snapshot_of, state_of, update_of, wait_for, wait_synced};

const DEVICES: usize = 3;
const EDITS_PER_DEVICE: u32 = 300;
const BURST: u32 = 10;
const RECONNECTS_PER_DEVICE: u32 = 8;
const ATTACHMENTS: usize = 10;

const WORKER_REQUEST_LIMIT: u64 = 100_000;
const DO_REQUEST_LIMIT: u64 = 100_000;
const ROWS_WRITTEN_LIMIT: u64 = 100_000;
const ROWS_READ_LIMIT: u64 = 5_000_000;

fn field(stats: &serde_json::Value, name: &str) -> u64 {
    stats[name]
        .as_u64()
        .unwrap_or_else(|| panic!("missing stat {name}"))
}

fn row(label: &str, used: u64, limit: u64) {
    println!(
        "{label:<46}{used:>10}{limit:>12}{:>9.2}%",
        used as f64 * 100.0 / limit as f64
    );
}

#[tokio::test]
async fn one_day_of_typical_use_stays_far_below_the_free_limits() {
    let h = fresh(require_server!()).await;
    let addr = server_addr(&h.url).await;
    let (first, signup) = h.sign_up().await;
    let mut clients = vec![(first, signup.unlocked)];
    for n in 1..DEVICES {
        let (client, unlocked) = h.login(&format!("device {n}")).await;
        clients.push((client, unlocked));
    }

    let mut proxies = Vec::new();
    let mut stores = Vec::new();
    let mut engines: Vec<SyncEngine> = Vec::new();
    for (n, (client, unlocked)) in clients.iter().enumerate() {
        let proxy = Proxy::start(addr).await;
        let store = Arc::new(MemoryStore::new());
        let mut cfg = client
            .engine_config(&format!("device-{n}"))
            .expect("config");
        cfg.server = proxy.url();
        cfg.batch_delay = Duration::from_millis(60);
        cfg.backoff = ne_sync::Backoff {
            base: Duration::from_millis(30),
            max: Duration::from_millis(200),
        };
        engines.push(
            SyncEngine::start(cfg, &unlocked.secrets.workspace_key, store.clone()).expect("engine"),
        );
        proxies.push(proxy);
        stores.push(store);
    }
    for engine in &engines {
        wait_synced(engine).await;
    }

    // The working day: bursts of edits, with a reconnect now and then.
    let bursts = EDITS_PER_DEVICE / BURST;
    let reconnect_every = bursts / RECONNECTS_PER_DEVICE;
    for burst in 0..bursts {
        for (n, engine) in engines.iter().enumerate() {
            for edit in 0..BURST {
                let value = (n as u32) * 10_000 + burst * BURST + edit;
                engine
                    .submit("doc-daily", &update_of(value))
                    .expect("submit");
            }
        }
        tokio::time::sleep(Duration::from_millis(140)).await;
        if burst % reconnect_every == reconnect_every - 1 {
            proxies.iter().for_each(Proxy::kill_connections);
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    }

    // Shared attachments: one upload, two downloads each.
    let key = &clients[0].1.secrets.workspace_key;
    let dir = tempfile::tempdir().expect("temp dir");
    for n in 0..ATTACHMENTS {
        let id = format!("att-{n}");
        let blob = clients[0]
            .0
            .upload_attachment(key, &id, Cursor::new(vec![n as u8; 40_000]))
            .await
            .expect("upload");
        for (client, unlocked) in &clients[1..] {
            let out = dir
                .path()
                .join(format!("{id}-{}", unlocked.session.session_id));
            client
                .download_attachment(&unlocked.secrets.workspace_key, &id, &blob, &out)
                .await
                .expect("download");
        }
    }

    // Everything converges, then each device compacts once.
    let want: BTreeSet<u32> = (0..DEVICES as u32)
        .flat_map(|n| (0..EDITS_PER_DEVICE).map(move |i| n * 10_000 + i))
        .collect();
    wait_for(
        "all devices to converge",
        Duration::from_secs(60),
        || async {
            (0..DEVICES).all(|n| {
                let own: BTreeSet<u32> = (0..EDITS_PER_DEVICE)
                    .map(|i| n as u32 * 10_000 + i)
                    .collect();
                state_of(&stores[n], "doc-daily", &own) == want
            })
        },
    )
    .await;
    for engine in &engines {
        wait_synced(engine).await;
    }
    for (n, engine) in engines.iter().enumerate() {
        let own: BTreeSet<u32> = (0..EDITS_PER_DEVICE)
            .map(|i| n as u32 * 10_000 + i)
            .collect();
        let upto = engine.snapshot_point().expect("point");
        let state = state_of(&stores[n], "doc-daily", &own);
        // Later devices find the document already compacted; that is fine.
        let _ = engine
            .upload_snapshot("doc-daily", upto, &snapshot_of(&state))
            .await;
    }

    let stats = h.stats().await;
    let (fetches, messages) = (field(&stats, "fetches"), field(&stats, "messages"));
    let (written, read) = (field(&stats, "rows_written"), field(&stats, "rows_read"));
    let worker_requests = fetches;
    let do_requests_worst = fetches + messages;
    let do_requests_billed = fetches + messages.div_ceil(20);
    println!();
    println!(
        "One simulated day: {DEVICES} devices x {EDITS_PER_DEVICE} edits in bursts of {BURST}, {RECONNECTS_PER_DEVICE} reconnects each, {ATTACHMENTS} attachments"
    );
    println!(
        "{:<46}{:>10}{:>12}{:>10}",
        "", "used", "free limit", "share"
    );
    row(
        "Worker requests (HTTP + WebSocket upgrades)",
        worker_requests,
        WORKER_REQUEST_LIMIT,
    );
    row(
        "Durable Object requests, every message counted",
        do_requests_worst,
        DO_REQUEST_LIMIT,
    );
    row(
        "Durable Object requests, 20:1 message ratio",
        do_requests_billed,
        DO_REQUEST_LIMIT,
    );
    row("Durable Object rows written", written, ROWS_WRITTEN_LIMIT);
    row("Durable Object rows read", read, ROWS_READ_LIMIT);
    println!(
        "(WebSocket frames received: {messages}; keepalive pings are answered by the runtime and not counted)"
    );

    assert!(worker_requests < WORKER_REQUEST_LIMIT / 10);
    assert!(do_requests_worst < DO_REQUEST_LIMIT / 10);
    assert!(written < ROWS_WRITTEN_LIMIT / 10);
    assert!(read < ROWS_READ_LIMIT / 10);
}
