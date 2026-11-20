//! Several `AppCore` instances as separate devices against a local server.
//! Skipped unless `NE_SYNC_TEST_URL` is set (see `server/README.md`).

mod support;

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use ne_core::page::{Block, PageDoc};
use note_everywhere_desktop_lib::app::types::{
    CreateNodeArgs, NodeKind, SessionState, SignInArgs, SignUpArgs, SyncState,
};
use note_everywhere_desktop_lib::app::{AppCore, CoreEvent};
use support::proxy::Proxy;
use support::{Device, PASSWORD, USERNAME};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, MutexGuard};

const SETUP_CODE: &str = "local-test-setup-code";
const VAULT_PASSWORD: &str = "vault secret passphrase";

static SERIAL: Mutex<()> = Mutex::const_new(());

/// Holds the server for one test (it keeps one account for good) and wipes it.
struct Server {
    url: String,
    addr: SocketAddr,
    _lock: MutexGuard<'static, ()>,
}

async fn http_post(addr: SocketAddr, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: NoteEverywhere/test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.expect("read");
    reply.lines().next().unwrap_or_default().to_owned()
}

async fn acquire() -> Option<Server> {
    let Ok(url) = std::env::var(support::SERVER_ENV) else {
        eprintln!("skipped: set NE_SYNC_TEST_URL to a local server (see server/README.md)");
        return None;
    };
    let lock = SERIAL.lock().await;
    let addr: SocketAddr = url
        .trim_start_matches("http://")
        .parse()
        .expect("host:port");
    let status = http_post(addr, "/v1/_test/reset").await;
    assert!(
        status.contains("204"),
        "the server must run with TEST_INSPECT=1: {status}"
    );
    Some(Server {
        url,
        addr,
        _lock: lock,
    })
}

macro_rules! server {
    () => {
        match acquire().await {
            Some(server) => server,
            None => return,
        }
    };
}

async fn wait_for<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

fn sign_up_args(url: &str, label: &str) -> SignUpArgs {
    SignUpArgs {
        server_url: url.into(),
        setup_code: SETUP_CODE.into(),
        username: USERNAME.into(),
        password: PASSWORD.into(),
        device_label: label.into(),
    }
}

fn sign_in_args(url: &str, label: &str, password: &str) -> SignInArgs {
    SignInArgs {
        server_url: url.into(),
        username: USERNAME.into(),
        password: password.into(),
        device_label: label.into(),
    }
}

async fn signed_up(server: &Server, label: &str) -> Device {
    let device = Device::new().await;
    let recovery = device
        .core
        .sign_up(sign_up_args(&server.url, label))
        .await
        .expect("sign up");
    assert_eq!(recovery.replace('-', "").len(), 44);
    device
}

async fn signed_in(url: &str, label: &str) -> Device {
    let device = Device::new().await;
    device
        .core
        .sign_in(sign_in_args(url, label, PASSWORD))
        .await
        .expect("sign in");
    device
}

fn page_args(parent: Option<&str>, title: &str) -> CreateNodeArgs {
    CreateNodeArgs {
        kind: NodeKind::Page,
        parent_id: parent.map(str::to_owned),
        index: None,
        title: Some(title.into()),
    }
}

async fn titles(core: &AppCore) -> Vec<String> {
    core.tree_snapshot()
        .await
        .expect("tree")
        .into_iter()
        .map(|n| n.title)
        .collect()
}

async fn append(core: &AppCore, page: &str, text: &str) {
    let snapshot = core.page_open(page).await.expect("open").snapshot;
    let doc = PageDoc::from_snapshot(&snapshot).expect("doc");
    let update = doc
        .edit(|d| d.append_blocks(&[], &[Block::paragraph(text)]).map(drop))
        .expect("edit")
        .1;
    core.page_apply_update(page, &update).await.expect("apply");
}

async fn text_of(core: &AppCore, page: &str) -> String {
    let snapshot = core.page_open(page).await.expect("open").snapshot;
    PageDoc::from_snapshot(&snapshot)
        .expect("doc")
        .plain_text()
        .expect("text")
}

async fn synced(core: &AppCore) {
    core.sync_now().await.expect("sync now");
}

async fn same_tree(a: &AppCore, b: &AppCore) -> bool {
    titles(a).await == titles(b).await
}

#[tokio::test]
async fn two_devices_converge_both_ways() {
    let server = server!();
    let a = signed_up(&server, "device a").await;
    assert_eq!(
        a.core.session_status().await.expect("status").state,
        SessionState::SignedIn
    );
    let folder = a
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Folder,
            parent_id: None,
            index: None,
            title: Some("Folder".into()),
        })
        .await
        .expect("folder");
    let page = a
        .core
        .tree_create(page_args(Some(&folder), "Shared page"))
        .await
        .expect("page");
    append(&a.core, &page, "written on a").await;

    // An attachment shown by a page, and a vault with one item.
    let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[9u8; 300]].concat();
    let added = a
        .core
        .attachment_add_bytes(png.clone(), "image/png", Some("pic.png"))
        .await
        .expect("attach");
    let snapshot = a.core.page_open(&page).await.expect("open").snapshot;
    let doc = PageDoc::from_snapshot(&snapshot).expect("doc");
    let image = Block::new(ne_core::page::BlockKind::Image {
        src: format!("attachment:{}", added.hash),
        alt: "pic".into(),
    });
    let update = doc
        .edit(|d| d.append_blocks(&[], &[image]).map(drop))
        .expect("edit")
        .1;
    a.core
        .page_apply_update(&page, &update)
        .await
        .expect("apply");
    a.core
        .vault_create(VAULT_PASSWORD.into())
        .await
        .expect("vault");
    a.core
        .vault_save(note_login("Mail", "hunter2"))
        .await
        .expect("item");
    synced(&a.core).await;
    wait_for("the attachment upload", || async {
        a.core.sync_status().await.expect("s").pending_updates == 0
    })
    .await;

    // B signs in on a fresh install and bootstraps everything.
    let b = signed_in(&server.url, "device b").await;
    wait_for("b to receive the tree", || async {
        same_tree(&a.core, &b.core).await
    })
    .await;
    assert_eq!(titles(&b.core).await, ["Folder", "Shared page"]);
    wait_for("b to receive the page", || async {
        text_of(&b.core, &page).await.contains("written on a")
    })
    .await;
    let remote_vault_ready = || async {
        b.core.vault_unlock(VAULT_PASSWORD.into()).await.is_ok()
            && !b.core.vault_list(None).await.expect("l").is_empty()
    };
    wait_for("b to receive the vault", remote_vault_ready).await;
    assert_eq!(
        b.core
            .vault_reveal(
                &b.core.vault_list(None).await.expect("l")[0].id,
                "password",
                None
            )
            .await
            .expect("reveal"),
        "hunter2"
    );

    // The attachment is fetched from the server on first use.
    wait_for("the attachment index to arrive", || async {
        b.core
            .serve_attachment(&format!("/{}", added.hash), None)
            .await
            .status
            == 200
    })
    .await;
    assert_eq!(
        b.core
            .serve_attachment(&format!("/{}", added.hash), None)
            .await
            .body,
        png
    );

    // And back: B edits, renames and creates; A converges, and an open page gets the update bytes.
    a.core.page_open(&page).await.expect("keep open on a");
    a.events.clear();
    append(&b.core, &page, "written on b").await;
    b.core
        .tree_rename(&page, "Renamed on b")
        .await
        .expect("rename");
    let extra = b
        .core
        .tree_create(page_args(None, "Created on b"))
        .await
        .expect("create");
    append(&b.core, &extra, "only on b").await;
    wait_for("a to see b's changes", || async {
        same_tree(&a.core, &b.core).await
            && text_of(&a.core, &page).await.contains("written on b")
            && text_of(&a.core, &extra).await.contains("only on b")
    })
    .await;
    assert!(titles(&a.core).await.contains(&"Renamed on b".to_owned()));
    let remote: Vec<_> = a.events.named("page:remote-update");
    assert!(remote.iter().any(|e| matches!(e, CoreEvent::PageRemoteUpdate { page_id, update } if *page_id == page && !update.is_empty())));
    assert!(!a.events.named("tree:changed").is_empty());
    let (ta, tb) = (text_of(&a.core, &page).await, text_of(&b.core, &page).await);
    assert_eq!(ta, tb);
    assert!(ta.contains("written on a") && ta.contains("written on b"));
    println!(
        "two_devices: {} nodes, page text {ta:?}",
        titles(&a.core).await.len()
    );
}

fn note_login(
    title: &str,
    password: &str,
) -> note_everywhere_desktop_lib::app::types::VaultItemInput {
    note_everywhere_desktop_lib::app::types::VaultItemInput {
        id: None,
        kind: "login".into(),
        title: title.into(),
        username: Some("alice".into()),
        password: Some(password.into()),
        value: None,
        urls: None,
        notes: None,
        tags: None,
    }
}

#[tokio::test]
async fn offline_edits_on_both_devices_converge_after_reconnecting() {
    let server = server!();
    let proxy_a = Proxy::start(server.addr).await;
    let proxy_b = Proxy::start(server.addr).await;
    let a = Device::new().await;
    a.core
        .sign_up(sign_up_args(&proxy_a.url(), "a"))
        .await
        .expect("sign up");
    let page = a
        .core
        .tree_create(page_args(None, "Doc"))
        .await
        .expect("page");
    append(&a.core, &page, "base").await;
    synced(&a.core).await;
    let b = Device::new().await;
    b.core
        .sign_in(sign_in_args(&proxy_b.url(), "b", PASSWORD))
        .await
        .expect("sign in");
    wait_for("b to bootstrap", || async {
        titles(&b.core).await == ["Doc"] && text_of(&b.core, &page).await.contains("base")
    })
    .await;

    proxy_a.cut();
    proxy_b.cut();
    wait_for("both to notice they are offline", || async {
        a.core.sync_status().await.expect("a").state == SyncState::Offline
            && b.core.sync_status().await.expect("b").state == SyncState::Offline
    })
    .await;
    append(&a.core, &page, "offline on a").await;
    append(&b.core, &page, "offline on b").await;
    a.core
        .tree_create(page_args(None, "Only a"))
        .await
        .expect("a page");
    b.core
        .tree_create(page_args(None, "Only b"))
        .await
        .expect("b page");
    b.core
        .tree_rename(&page, "Doc renamed")
        .await
        .expect("rename");
    assert!(a.core.sync_status().await.expect("a").pending_updates >= 3);
    assert!(b.core.sync_status().await.expect("b").pending_updates >= 4);
    assert!(
        a.core.sync_now().await.is_err(),
        "sync now reports the outage"
    );

    proxy_a.restore();
    proxy_b.restore();
    synced(&a.core).await;
    synced(&b.core).await;
    wait_for("both to converge", || async {
        let (ta, tb) = (text_of(&a.core, &page).await, text_of(&b.core, &page).await);
        same_tree(&a.core, &b.core).await
            && ta == tb
            && ta.contains("offline on a")
            && ta.contains("offline on b")
    })
    .await;
    let mut names = titles(&a.core).await;
    names.sort();
    assert_eq!(names, ["Doc renamed", "Only a", "Only b"]);
    assert_eq!(a.core.sync_status().await.expect("a").pending_updates, 0);
    println!("offline: converged on {names:?}");
}

#[tokio::test]
async fn a_third_device_bootstraps_from_a_compacted_log() {
    let server = server!();
    let a = signed_up(&server, "a").await;
    let page = a
        .core
        .tree_create(page_args(None, "Busy page"))
        .await
        .expect("page");
    for n in 0..45 {
        append(&a.core, &page, &format!("line {n}")).await;
    }
    synced(&a.core).await;
    let taken = a.core.compact_server_log(40).await.expect("compact");
    assert!(taken >= 1, "the busy page should have been snapshotted");
    let c = signed_in(&server.url, "c").await;
    wait_for("c to bootstrap", || async {
        titles(&c.core).await == ["Busy page"] && text_of(&c.core, &page).await.contains("line 44")
    })
    .await;
    assert_eq!(text_of(&c.core, &page).await, text_of(&a.core, &page).await);
    println!("compaction: {taken} snapshot(s) accepted");
}

#[tokio::test]
async fn a_password_change_signs_the_other_device_out() {
    let server = server!();
    let a = signed_up(&server, "a").await;
    let page = a
        .core
        .tree_create(page_args(None, "Doc"))
        .await
        .expect("page");
    synced(&a.core).await;
    let b = signed_in(&server.url, "b").await;
    wait_for("b to bootstrap", || async {
        titles(&b.core).await == ["Doc"]
    })
    .await;

    assert_eq!(
        a.core
            .change_password("wrong", "new password")
            .await
            .expect_err("wrong")
            .code,
        "wrong_password"
    );
    a.core
        .change_password(PASSWORD, "new password")
        .await
        .expect("change");
    wait_for("b to need a new sign-in", || async {
        b.core.session_status().await.expect("s").state == SessionState::NeedsSignIn
    })
    .await;
    let status = b.core.sync_status().await.expect("status");
    assert_eq!(status.state, SyncState::Error);
    assert!(
        status
            .last_error
            .expect("message")
            .contains("Sign in again")
    );
    assert!(b.events.named("session:changed").iter().any(
        |e| matches!(e, CoreEvent::SessionChanged(s) if s.state == SessionState::NeedsSignIn)
    ));
    assert_eq!(
        b.secrets.get_len(),
        0,
        "the rejected credentials were removed from the keychain"
    );

    // Local data is intact and editable meanwhile; the old password no longer works.
    append(&b.core, &page, "written while signed out").await;
    assert_eq!(
        b.core
            .sign_in(sign_in_args(&server.url, "b", PASSWORD))
            .await
            .expect_err("old")
            .code,
        "wrong_password"
    );
    let other = b
        .core
        .sign_in(SignInArgs {
            username: "bob@example.test".into(),
            ..sign_in_args(&server.url, "b", "new password")
        })
        .await;
    assert_eq!(other.expect_err("other account").code, "account_mismatch");
    b.core
        .sign_in(sign_in_args(&server.url, "b", "new password"))
        .await
        .expect("new password");
    assert_eq!(
        b.core.session_status().await.expect("s").state,
        SessionState::SignedIn
    );
    synced(&b.core).await;
    wait_for("a to receive b's offline edit", || async {
        text_of(&a.core, &page)
            .await
            .contains("written while signed out")
    })
    .await;
    // A is still signed in; the same device can sign out and in again.
    assert_eq!(
        a.core.session_status().await.expect("s").state,
        SessionState::SignedIn
    );
}

trait SecretCount {
    fn get_len(&self) -> usize;
}

impl SecretCount for note_everywhere_desktop_lib::app::MemoryBackend {
    fn get_len(&self) -> usize {
        use ne_platform::secure_store::SecretBackend;
        ["default/session-token", "default/root-key"]
            .iter()
            .filter(|user| {
                self.get("dev.noteeverywhere.app", user)
                    .expect("get")
                    .is_some()
            })
            .count()
    }
}

#[tokio::test]
async fn a_local_workspace_upgraded_to_synced_uploads_its_data() {
    let server = server!();
    let local = Device::local().await;
    let folder = local
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Folder,
            parent_id: None,
            index: None,
            title: Some("Local folder".into()),
        })
        .await
        .expect("folder");
    let page = local
        .core
        .tree_create(page_args(Some(&folder), "Written before sign-up"))
        .await
        .expect("page");
    append(&local.core, &page, "offline first").await;
    local
        .core
        .vault_create(VAULT_PASSWORD.into())
        .await
        .expect("vault");
    local
        .core
        .vault_save(note_login("Bank", "pw"))
        .await
        .expect("item");
    assert_eq!(
        local.core.sync_status().await.expect("status").state,
        SyncState::Disabled
    );

    local
        .core
        .sign_up(sign_up_args(&server.url, "local device"))
        .await
        .expect("sign up");
    synced(&local.core).await;
    let other = signed_in(&server.url, "second").await;
    wait_for("the second device to receive the old data", || async {
        titles(&other.core).await == ["Local folder", "Written before sign-up"]
            && text_of(&other.core, &page).await.contains("offline first")
    })
    .await;
    wait_for("the vault", || async {
        other.core.vault_unlock(VAULT_PASSWORD.into()).await.is_ok()
            && other.core.vault_list(None).await.expect("l").len() == 1
    })
    .await;

    // Signing out keeps the data and returns to a local workspace.
    local.core.sign_out().await.expect("sign out");
    let status = local.core.session_status().await.expect("status");
    assert_eq!((status.state, status.username), (SessionState::Local, None));
    assert_eq!(
        local.core.sync_status().await.expect("s").state,
        SyncState::Disabled
    );
    assert_eq!(
        titles(&local.core).await,
        ["Local folder", "Written before sign-up"]
    );
    assert_eq!(local.secrets.get_len(), 0);
    assert_eq!(
        local.core.sign_out().await.expect_err("twice").code,
        "not_signed_in"
    );
}

#[tokio::test]
async fn restarting_resumes_the_session_without_a_password() {
    let server = server!();
    let a = signed_up(&server, "a").await;
    let page = a
        .core
        .tree_create(page_args(None, "Doc"))
        .await
        .expect("page");
    synced(&a.core).await;
    let b = signed_in(&server.url, "b").await;
    wait_for("b to bootstrap", || async {
        titles(&b.core).await == ["Doc"]
    })
    .await;
    assert_eq!(
        a.secrets.get_len(),
        2,
        "only the session token and the root key are kept"
    );

    let a = a.restart().await;
    let status = a.core.session_status().await.expect("status");
    assert_eq!(
        (status.state, status.username.as_deref()),
        (SessionState::SignedIn, Some(USERNAME))
    );
    assert_eq!(titles(&a.core).await, ["Doc"]);
    append(&a.core, &page, "after restart").await;
    wait_for("b to receive the edit made after a restart", || async {
        text_of(&b.core, &page).await.contains("after restart")
    })
    .await;
    println!("restart: resumed as {:?}", status.server_url);

    // Sessions: both devices are listed and the current one is marked.
    let sessions = a.core.sessions_list().await.expect("sessions");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions.iter().filter(|s| s.current).count(), 1);
    let other = sessions
        .iter()
        .find(|s| s.device_label == "b")
        .expect("b's session");
    a.core.sessions_revoke(&other.id).await.expect("revoke");
    wait_for("b to lose its session", || async {
        b.core.session_status().await.expect("s").state == SessionState::NeedsSignIn
    })
    .await;
}

#[tokio::test]
async fn account_errors_have_stable_codes() {
    let server = server!();
    let device = Device::new().await;
    let mut wrong = sign_up_args(&server.url, "a");
    wrong.setup_code = "not the code".into();
    assert_eq!(
        device
            .core
            .sign_up(wrong)
            .await
            .expect_err("setup code")
            .code,
        "forbidden"
    );
    assert_eq!(
        device.core.session_status().await.expect("s").state,
        SessionState::Empty
    );
    device
        .core
        .sign_up(sign_up_args(&server.url, "a"))
        .await
        .expect("sign up");
    let second = Device::new().await;
    assert_eq!(
        second
            .core
            .sign_up(sign_up_args(&server.url, "b"))
            .await
            .expect_err("closed")
            .code,
        "registration_closed"
    );
    assert_eq!(
        second
            .core
            .sign_in(sign_in_args(&server.url, "b", "nope"))
            .await
            .expect_err("password")
            .code,
        "wrong_password"
    );
    assert_eq!(
        second.core.session_status().await.expect("s").state,
        SessionState::Empty,
        "a failed sign-in creates nothing"
    );
    let mut codes = Vec::new();
    for _ in 0..8 {
        codes.push(
            second
                .core
                .sign_in(sign_in_args(&server.url, "b", "still wrong"))
                .await
                .expect_err("fails")
                .code,
        );
    }
    assert!(
        codes.contains(&"locked_out".to_owned()),
        "repeated failures lock the account out: {codes:?}"
    );
    let unreachable = Device::new().await;
    assert_eq!(
        unreachable
            .core
            .sign_in(sign_in_args("http://127.0.0.1:1", "x", PASSWORD))
            .await
            .expect_err("offline")
            .code,
        "offline"
    );
    assert_eq!(
        device
            .core
            .sign_up(sign_up_args(&server.url, "again"))
            .await
            .expect_err("signed in")
            .code,
        "already_signed_in"
    );
}

#[tokio::test]
async fn a_server_that_went_back_in_time_stops_sync_and_keeps_local_data() {
    let server = server!();
    let a = signed_up(&server, "a").await;
    let page = a
        .core
        .tree_create(page_args(None, "Precious"))
        .await
        .expect("page");
    for n in 0..12 {
        append(&a.core, &page, &format!("note {n}")).await;
    }
    synced(&a.core).await;

    // The server is wiped and the same account is created again with far less history.
    assert!(
        http_post(server.addr, "/v1/_test/reset")
            .await
            .contains("204")
    );
    let fresh = signed_up(&server, "fresh").await;
    fresh
        .core
        .tree_create(page_args(None, "New start"))
        .await
        .expect("page");
    synced(&fresh.core).await;

    wait_for("a to lose its session", || async {
        a.core.session_status().await.expect("s").state == SessionState::NeedsSignIn
    })
    .await;
    a.core
        .sign_in(sign_in_args(&server.url, "a", PASSWORD))
        .await
        .expect("sign in again");
    wait_for("a to stop with a reset error", || async {
        let status = a.core.sync_status().await.expect("status");
        status.state == SyncState::Error && status.last_error.is_some_and(|m| m.contains("reset"))
    })
    .await;
    assert!(a.core.sync_now().await.is_err());
    assert!(
        text_of(&a.core, &page).await.contains("note 11"),
        "nothing local was lost"
    );
    assert_eq!(titles(&a.core).await, ["Precious"]);
}

#[tokio::test]
async fn a_missing_or_rejected_session_at_start_means_needs_sign_in() {
    let server = server!();
    let a = signed_up(&server, "a").await;
    a.core
        .tree_create(page_args(None, "Doc"))
        .await
        .expect("page");
    synced(&a.core).await;
    let b = signed_in(&server.url, "b").await;
    wait_for("b to bootstrap", || async {
        titles(&b.core).await == ["Doc"]
    })
    .await;

    // The keychain entries are gone: the data opens, the account asks for the password.
    {
        use ne_platform::secure_store::SecretBackend;
        for user in ["default/session-token", "default/root-key"] {
            b.secrets
                .delete("dev.noteeverywhere.app", user)
                .expect("delete");
        }
    }
    let b = b.restart().await;
    let status = b.core.session_status().await.expect("status");
    assert_eq!(
        (status.state, status.username.as_deref()),
        (SessionState::NeedsSignIn, Some(USERNAME))
    );
    assert_eq!(titles(&b.core).await, ["Doc"]);
    b.core
        .sign_in(sign_in_args(&server.url, "b", PASSWORD))
        .await
        .expect("sign in");
    assert_eq!(
        b.core.session_status().await.expect("status").state,
        SessionState::SignedIn
    );

    // A session revoked while the app was closed is noticed when it reconnects.
    let sessions = a.core.sessions_list().await.expect("sessions");
    // B signed in twice, so it has an older orphaned session; the newest one is live.
    let b_session = sessions
        .iter()
        .filter(|s| !s.current)
        .max_by_key(|s| s.created_at)
        .expect("b's session");
    let b = {
        b.core.shutdown().await;
        let (dir, secrets) = (b.dir.clone(), b.secrets.clone());
        drop(b);
        a.core.sessions_revoke(&b_session.id).await.expect("revoke");
        Device::open_in(dir, secrets).await
    };
    wait_for("the rejected session to be noticed", || async {
        b.core.session_status().await.expect("s").state == SessionState::NeedsSignIn
    })
    .await;
    assert_eq!(titles(&b.core).await, ["Doc"]);
}

#[tokio::test]
async fn two_different_vaults_are_not_merged() {
    let server = server!();
    let a = signed_up(&server, "a").await;
    a.core
        .vault_create(VAULT_PASSWORD.into())
        .await
        .expect("vault");
    a.core
        .vault_save(note_login("Mail", "hunter2"))
        .await
        .expect("item");
    synced(&a.core).await;

    let local = Device::local().await;
    local
        .core
        .vault_create("another vault password".into())
        .await
        .expect("local vault");
    let refused = local
        .core
        .sign_in(sign_in_args(&server.url, "local", PASSWORD))
        .await
        .expect_err("conflict");
    assert_eq!(refused.code, "vault_conflict");
    let status = local.core.session_status().await.expect("status");
    assert_eq!(
        status.state,
        SessionState::Local,
        "nothing changed on the device"
    );
    assert_eq!(local.secrets.get_len(), 0);
    assert_eq!(
        a.core.sessions_list().await.expect("sessions").len(),
        1,
        "the refused session was closed again"
    );
}
