//! Drives the registered commands through Tauri's mock runtime, so argument
//! parsing, camelCase JSON and the `{code, message}` rejection are exercised
//! the way the webview would see them (no window is opened).
//!
//! Not run on Windows: a test executable that links the Tauri runtime lacks
//! the application manifest the real binary gets from the bundler, so it fails
//! to load there (STATUS_ENTRYPOINT_NOT_FOUND). What is checked here, argument
//! names and the error shape, does not depend on the operating system.
#![cfg(not(windows))]

mod support;

use serde_json::{Value, json};
use support::Device;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow};

use note_everywhere_desktop_lib::command_handler;

struct Ipc {
    window: WebviewWindow<tauri::test::MockRuntime>,
    // Keeps the app, and with it the managed core, alive.
    _app: tauri::App<tauri::test::MockRuntime>,
    _device: Device,
}

fn ipc() -> Ipc {
    let device = tauri::async_runtime::block_on(Device::new());
    let app = mock_builder()
        .invoke_handler(command_handler())
        .build(mock_context(noop_assets()))
        .expect("mock app");
    app.manage(device.core.clone());
    let window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("window");
    Ipc {
        window,
        _app: app,
        _device: device,
    }
}

impl Ipc {
    fn call(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        let request = InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("url"),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        get_ipc_response(&self.window, request)
            .map(|body| body.deserialize::<Value>().expect("json body"))
    }

    fn ok(&self, cmd: &str, args: Value) -> Value {
        self.call(cmd, args)
            .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
    }
}

#[test]
fn commands_speak_the_contracts_json() {
    let ipc = ipc();
    assert_eq!(
        ipc.ok("app_version", json!({})),
        json!(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        ipc.ok("session_status", json!({})),
        json!({ "state": "empty" })
    );
    ipc.ok("workspace_create_local", json!({}));
    assert_eq!(
        ipc.ok("session_status", json!({})),
        json!({ "state": "local" })
    );

    let folder = ipc.ok(
        "tree_create",
        json!({ "kind": "folder", "parentId": null, "title": "Inbox" }),
    );
    let page = ipc.ok(
        "tree_create",
        json!({ "kind": "page", "parentId": folder, "index": 0, "title": "Today" }),
    );
    let tree = ipc.ok("tree_snapshot", json!({}));
    assert_eq!(tree[0]["id"], folder);
    assert_eq!(tree[1]["parentId"], folder);
    assert_eq!(tree[1]["kind"], "page");
    assert_eq!(tree[1]["favorite"], false);
    assert!(tree[1]["createdAt"].is_i64());

    ipc.ok("tree_set_icon", json!({ "id": page, "icon": "📝" }));
    ipc.ok("tree_set_favorite", json!({ "id": page, "favorite": true }));
    ipc.ok("tree_move", json!({ "id": page, "parentId": null }));
    let opened = ipc.ok("page_open", json!({ "id": page }));
    assert!(
        opened["snapshot"]
            .as_array()
            .is_some_and(|bytes| !bytes.is_empty())
    );
    ipc.ok("tree_trash", json!({ "id": page }));
    let trash = ipc.ok("tree_list_trash", json!({}));
    assert_eq!(
        (trash[0]["id"].clone(), trash[0]["title"].clone()),
        (page, json!("Today"))
    );
    assert!(trash[0]["trashedAt"].is_i64());

    let attachment = ipc.ok("attachment_add_bytes", json!({ "bytes": [137, 80, 78, 71, 13, 10, 26, 10, 1], "mime": "image/png", "fileName": "x.png" }));
    assert_eq!(attachment["byteLength"], 9);
    assert_eq!(attachment["mime"], "image/png");
    assert!(
        attachment["url"]
            .as_str()
            .is_some_and(|u| u.ends_with(attachment["hash"].as_str().expect("hash")))
    );
    assert_eq!(ipc.ok("sync_status", json!({}))["state"], "disabled");
    assert_eq!(ipc.ok("vault_state", json!({})), json!("uninitialised"));
    let vault = ipc.ok("vault_create", json!({ "password": "vault password" }));
    assert_eq!(vault["sameAsAccountPassword"], false);
    assert!(vault["recoveryKey"].is_string());
    let item = ipc.ok("vault_save", json!({ "item": { "kind": "login", "title": "Mail", "password": "pw", "urls": ["https://m.test"] } }));
    assert_eq!(item["kind"], "login");
    assert!(item["updatedAt"].is_i64());
    assert_eq!(
        ipc.ok(
            "vault_reveal",
            json!({ "id": item["id"], "field": "password" })
        ),
        json!("pw")
    );
    assert_eq!(
        ipc.ok(
            "vault_copy",
            json!({ "id": item["id"], "field": "username" })
        )["clearAfterSeconds"],
        0
    );
    let generated = ipc.ok(
        "vault_generate_password",
        json!({ "mode": "characters", "length": 16 }),
    );
    assert_eq!(generated["value"].as_str().map(str::len), Some(16));
    assert!(generated["entropyBits"].is_f64());
    let settings = ipc.ok("settings_set", json!({ "settings": { "theme": "dark" } }));
    assert_eq!(settings["theme"], "dark");
    assert_eq!(settings["vaultIdleLockSeconds"], 300);
    assert_eq!(
        ipc.ok("voice_status", json!({}))["available"],
        cfg!(feature = "transcribe")
    );
    assert_eq!(
        ipc.ok("export_markdown", json!({ "id": null })),
        Value::Null,
        "the picker was cancelled"
    );
}

#[test]
fn failures_reject_with_code_and_message() {
    let ipc = ipc();
    let no_workspace = ipc
        .call("tree_snapshot", json!({}))
        .expect_err("no workspace");
    assert_eq!(no_workspace["code"], "no_workspace");
    assert!(
        no_workspace["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty())
    );
    ipc.ok("workspace_create_local", json!({}));
    let bad_id = ipc
        .call("tree_rename", json!({ "id": "nonsense", "title": "x" }))
        .expect_err("bad id");
    assert_eq!(bad_id["code"], "not_found");
    let malformed = ipc
        .call("tree_rename", json!({ "id": 5 }))
        .expect_err("malformed");
    assert_eq!(malformed["code"], "invalid_input");
    let locked = ipc.call("vault_list", json!({})).expect_err("no vault");
    assert_eq!(locked["code"], "vault_uninitialised");
    let wrong = ipc.call("sign_in", json!({ "serverUrl": "http://127.0.0.1:1", "username": "a@example.test", "password": "x", "deviceLabel": "t" })).expect_err("offline");
    assert_eq!(wrong["code"], "offline");
    let recovery = ipc.call("recover_account", json!({ "serverUrl": "http://127.0.0.1:1", "username": "a@example.test", "recoveryKey": "nope", "newPassword": "x", "deviceLabel": "t" })).expect_err("bad key");
    assert_eq!(recovery["code"], "invalid_recovery_key");
}
