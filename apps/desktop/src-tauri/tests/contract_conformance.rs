//! The Rust side must cover the UI contract exactly: every `CoreApi` method
//! is a registered command with the argument fields the contract names, and
//! every `CoreEventMap` event is emitted from somewhere.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use note_everywhere_desktop_lib::app::CoreEvent;
use note_everywhere_desktop_lib::app::types::{
    AddedAttachment, SessionState, SessionStatus, SyncState, SyncStatus, VoiceText,
};
use note_everywhere_desktop_lib::app::update::{UpdateState, UpdateStatus};

fn manifest(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read(path: &str) -> String {
    fs::read_to_string(manifest(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn snake_case(name: &str) -> String {
    name.chars().fold(String::new(), |mut out, c| {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
        out
    })
}

fn camel_case(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The body of `export interface <name> { ... }`, up to the closing brace at column 0.
fn interface_body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source
        .find(&format!("export interface {name} {{"))
        .unwrap_or_else(|| panic!("interface {name} not found"));
    let body = &source[start..];
    let end = body.find("\n}").expect("interface end");
    &body[..end]
}

/// One contract method: its name and the argument fields written inline, with
/// whether each may be absent or null.
struct ContractMethod {
    snake: String,
    inline_args: Option<BTreeMap<String, bool>>,
}

fn contract_methods() -> Vec<ContractMethod> {
    let source = read("../src/api/contract.ts");
    let mut methods = Vec::new();
    for line in interface_body(&source, "CoreApi").lines() {
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        let Some((name, after)) = rest.split_once('(') else {
            continue;
        };
        if name.is_empty() || !name.chars().all(char::is_alphanumeric) || name.starts_with("//") {
            continue;
        }
        let inline_args = after.strip_prefix("args: {").map(|fields| {
            let fields = fields.split_once("})").expect("inline args end").0;
            fields
                .split(';')
                .filter_map(|field| {
                    let field = field.trim();
                    let (key, ty) = field.split_once(':')?;
                    let optional = key.ends_with('?') || ty.contains("null");
                    Some((key.trim_end_matches('?').trim().to_owned(), optional))
                })
                .collect()
        });
        methods.push(ContractMethod {
            snake: snake_case(name),
            inline_args,
        });
    }
    methods
}

fn registered_commands() -> BTreeSet<String> {
    let source = read("src/lib.rs");
    let list = source
        .split("generate_handler![")
        .nth(1)
        .expect("handler list");
    let list = list.split(']').next().expect("handler list end");
    list.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .strip_prefix("commands::")
                .expect("commands:: path")
                .to_owned()
        })
        .collect()
}

/// Field lists written in `command!(name { field: Type, ... } -> ...)`.
fn declared_fields() -> BTreeMap<String, BTreeMap<String, bool>> {
    let source = read("src/commands.rs");
    let mut out = BTreeMap::new();
    for statement in source.split("command!(").skip(1) {
        let Some((name, rest)) = statement.split_once([' ', '(']) else {
            continue;
        };
        let rest = rest.trim_start();
        if !rest.starts_with('{') {
            continue;
        }
        let fields = rest[1..].split_once('}').expect("field list end").0;
        let parsed = fields
            .split(',')
            .filter_map(|field| {
                let (key, ty) = field.split_once(':')?;
                Some((camel_case(key.trim()), ty.trim().starts_with("Option<")))
            })
            .collect();
        out.insert(name.to_owned(), parsed);
    }
    out
}

#[test]
fn every_contract_method_is_a_registered_command() {
    let contract: BTreeSet<String> = contract_methods().into_iter().map(|m| m.snake).collect();
    let registered = registered_commands();
    assert!(
        contract.len() > 50,
        "the contract parse found {} methods",
        contract.len()
    );
    let missing: Vec<_> = contract.difference(&registered).collect();
    let extra: Vec<_> = registered.difference(&contract).collect();
    assert!(
        missing.is_empty(),
        "contract methods without a command: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "commands the contract does not know: {extra:?}"
    );
    assert!(contract.contains("voice_select_model"));
}

#[test]
fn argument_fields_match_the_contract() {
    let declared = declared_fields();
    let mut checked = 0;
    for method in contract_methods() {
        let Some(expected) = method.inline_args else {
            continue;
        };
        let actual = declared.get(&method.snake).unwrap_or_else(|| {
            panic!(
                "{} takes inline arguments but declares no field list",
                method.snake
            )
        });
        assert_eq!(
            actual.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>(),
            "argument names of {}",
            method.snake
        );
        for (field, optional) in &expected {
            assert_eq!(
                actual[field], *optional,
                "{}: `{field}` optional or nullable in the contract only if Option in Rust",
                method.snake
            );
        }
        checked += 1;
    }
    assert!(checked > 30, "only {checked} commands were compared");
}

fn sample_events() -> Vec<CoreEvent> {
    vec![
        CoreEvent::TreeChanged { node_ids: vec![] },
        CoreEvent::PageRemoteUpdate {
            page_id: String::new(),
            update: vec![],
        },
        CoreEvent::SyncStatus(SyncStatus {
            state: SyncState::Disabled,
            pending_updates: 0,
            last_synced_at: None,
            last_error: None,
        }),
        CoreEvent::SessionChanged(SessionStatus {
            state: SessionState::Empty,
            username: None,
            server_url: None,
        }),
        CoreEvent::VaultState { state: "locked" },
        CoreEvent::VoiceDownloadProgress {
            name: String::new(),
            received_bytes: 0,
            total_bytes: 0,
        },
        CoreEvent::VoiceHotkey { pressed: true },
        CoreEvent::DatabaseChanged {
            database_id: String::new(),
            row_ids: vec![],
        },
        CoreEvent::RemindersChanged,
        CoreEvent::ReminderDue {
            reminders: vec![],
            native_shown: false,
        },
        CoreEvent::UpdateStatus(UpdateStatus {
            state: UpdateState::Idle,
            current_version: String::new(),
            version: None,
            notes: None,
            last_checked_at: None,
            message: None,
        }),
        CoreEvent::UpdateProgress {
            received_bytes: 0,
            total_bytes: None,
        },
    ]
}

#[test]
fn every_event_name_is_emitted_from_somewhere() {
    let source = read("../src/api/contract.ts");
    let names: BTreeSet<String> = interface_body(&source, "CoreEventMap")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let quoted = line.strip_prefix('"')?;
            Some(quoted.split_once('"')?.0.to_owned())
        })
        .collect();
    assert_eq!(names.len(), 12, "{names:?}");
    let samples = sample_events();
    let ours: BTreeSet<String> = samples.iter().map(|e| e.name().to_owned()).collect();
    assert_eq!(names, ours, "event names differ from the contract");

    // Each variant is built by code other than the event definitions themselves.
    let mut sources = Vec::new();
    for dir in ["src", "src/app"] {
        for entry in fs::read_dir(manifest(dir)).expect("dir") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "events.rs")
            {
                sources.push(fs::read_to_string(&path).expect("source"));
            }
        }
    }
    for event in &samples {
        let debug = format!("{event:?}");
        let variant = debug.split([' ', '(', '{']).next().expect("variant");
        let needle = format!("CoreEvent::{variant}");
        assert!(
            sources.iter().any(|s| s.contains(&needle)),
            "{} ({variant}) is never emitted",
            event.name()
        );
    }
}

#[test]
fn the_events_of_databases_and_reminders_have_the_documented_payloads() {
    assert_eq!(
        CoreEvent::DatabaseChanged {
            database_id: "1@2".into(),
            row_ids: vec!["3@4".into()],
        }
        .payload(),
        serde_json::json!({ "databaseId": "1@2", "rowIds": ["3@4"] })
    );
    assert_eq!(CoreEvent::RemindersChanged.payload(), serde_json::json!({}));
    assert_eq!(
        CoreEvent::ReminderDue {
            reminders: vec![],
            native_shown: true,
        }
        .payload(),
        serde_json::json!({ "reminders": [], "nativeShown": true })
    );
}

#[test]
fn event_payloads_are_camel_case_json() {
    let payload = CoreEvent::PageRemoteUpdate {
        page_id: "1@2".into(),
        update: vec![1, 2],
    }
    .payload();
    assert_eq!(
        payload,
        serde_json::json!({ "pageId": "1@2", "update": [1, 2] })
    );
    let sync = CoreEvent::SyncStatus(SyncStatus {
        state: SyncState::Pending,
        pending_updates: 3,
        last_synced_at: None,
        last_error: None,
    })
    .payload();
    assert_eq!(
        sync,
        serde_json::json!({ "state": "pending", "pendingUpdates": 3, "lastSyncedAt": null, "lastError": null })
    );
    let session = CoreEvent::SessionChanged(SessionStatus {
        state: SessionState::NeedsSignIn,
        username: Some("a".into()),
        server_url: None,
    })
    .payload();
    assert_eq!(
        session,
        serde_json::json!({ "state": "needs_sign_in", "username": "a" })
    );
}

#[test]
fn voice_stop_returns_the_kept_recording_as_the_contract_says() {
    let contract = read("../src/api/contract.ts");
    assert!(
        contract.contains("voiceStop(): Promise<{ text: string; audio?: AddedAttachment }>;"),
        "the contract's voiceStop result changed; update the shell to match"
    );
    let kept = VoiceText {
        text: "hello".into(),
        audio: Some(AddedAttachment {
            hash: "ab".repeat(32),
            mime: "audio/wav".into(),
            byte_length: 44,
            url: "ne-attachment://localhost/ab".into(),
        }),
    };
    assert_eq!(
        serde_json::to_value(&kept).expect("json"),
        serde_json::json!({
            "text": "hello",
            "audio": {
                "hash": "ab".repeat(32),
                "mime": "audio/wav",
                "byteLength": 44,
                "url": "ne-attachment://localhost/ab",
            }
        })
    );
    // Without a kept recording the key is absent, not null.
    let plain = VoiceText {
        text: "hello".into(),
        audio: None,
    };
    assert_eq!(
        serde_json::to_value(&plain).expect("json"),
        serde_json::json!({ "text": "hello" })
    );
}

#[test]
fn the_update_events_have_the_documented_payloads() {
    let status = UpdateStatus {
        state: UpdateState::Available,
        current_version: "0.1.0".into(),
        version: Some("0.2.0".into()),
        notes: None,
        last_checked_at: Some(5),
        message: None,
    };
    assert_eq!(
        CoreEvent::UpdateStatus(status).payload(),
        serde_json::json!({
            "state": "available",
            "currentVersion": "0.1.0",
            "version": "0.2.0",
            "lastCheckedAt": 5,
            "message": null,
        })
    );
    assert_eq!(
        CoreEvent::UpdateProgress {
            received_bytes: 10,
            total_bytes: None,
        }
        .payload(),
        serde_json::json!({ "receivedBytes": 10, "totalBytes": null })
    );
    // Every state the contract lists serialises under that name.
    let source = read("../src/api/contract.ts");
    let states = interface_body(&source, "UpdateStatus");
    assert!(states.contains("state: UpdateState;"));
    for (state, name) in [
        (UpdateState::Unavailable, "unavailable"),
        (UpdateState::Idle, "idle"),
        (UpdateState::Checking, "checking"),
        (UpdateState::UpToDate, "up_to_date"),
        (UpdateState::Available, "available"),
        (UpdateState::Downloading, "downloading"),
        (UpdateState::Ready, "ready"),
        (UpdateState::Error, "error"),
    ] {
        assert_eq!(serde_json::to_value(state).expect("json"), name);
        assert!(
            source.contains(&format!("| \"{name}\"")),
            "the contract does not list the state {name}"
        );
    }
}

/// The webview may only listen to events: everything else (dialogs, clipboard,
/// the shortcut, notifications, the updater) is driven from Rust, so no plugin
/// permission may be granted, and the UI must not import a plugin API.
#[test]
fn the_capability_grants_only_what_the_ui_uses() {
    let capability: serde_json::Value =
        serde_json::from_str(&read("capabilities/default.json")).expect("capability json");
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["core:event:default"])
    );
    let mut imports = Vec::new();
    let mut stack = vec![manifest("../src")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).expect("dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "ts" || e == "tsx")
                && fs::read_to_string(&path)
                    .expect("source")
                    .contains("@tauri-apps/plugin-")
            {
                imports.push(path);
            }
        }
    }
    assert!(
        imports.is_empty(),
        "the UI imports a plugin API, which needs a capability: {imports:?}"
    );
}
