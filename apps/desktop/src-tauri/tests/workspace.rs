//! Tree, pages, search, backlinks, attachments, Markdown and settings,
//! against a local-only workspace.

mod support;

use ne_core::page::{Block, BlockKind, PageDoc};
use note_everywhere_desktop_lib::app::CoreEvent;
use note_everywhere_desktop_lib::app::types::{
    CreateNodeArgs, MoveNodeArgs, NodeKind, SessionState, SettingsPatch,
};
use support::Device;

fn create(
    kind: NodeKind,
    parent: Option<&str>,
    index: Option<usize>,
    title: &str,
) -> CreateNodeArgs {
    CreateNodeArgs {
        kind,
        parent_id: parent.map(str::to_owned),
        index,
        title: Some(title.to_owned()),
    }
}

/// An update that appends a paragraph, produced the way the editor would.
fn paragraph_update(snapshot: &[u8], text: &str) -> Vec<u8> {
    let doc = PageDoc::from_snapshot(snapshot).expect("snapshot");
    doc.edit(|d| d.append_blocks(&[], &[Block::paragraph(text)]).map(drop))
        .expect("edit")
        .1
}

#[tokio::test]
async fn a_workspace_is_created_once() {
    let device = Device::new().await;
    assert_eq!(
        device.core.session_status().await.expect("status").state,
        SessionState::Empty
    );
    assert_eq!(
        device
            .core
            .tree_snapshot()
            .await
            .expect_err("no workspace")
            .code,
        "no_workspace"
    );
    device.core.workspace_create_local().await.expect("create");
    assert_eq!(
        device.core.session_status().await.expect("status").state,
        SessionState::Local
    );
    assert_eq!(
        device
            .core
            .workspace_create_local()
            .await
            .expect_err("twice")
            .code,
        "workspace_exists"
    );
    let events = device.events.named("session:changed");
    assert!(!events.is_empty());
}

#[tokio::test]
async fn the_tree_is_depth_first_and_follows_moves() {
    let device = Device::local().await;
    let core = &device.core;
    let a = core
        .tree_create(create(NodeKind::Folder, None, None, "A"))
        .await
        .expect("a");
    let b = core
        .tree_create(create(NodeKind::Page, None, None, "B"))
        .await
        .expect("b");
    let a1 = core
        .tree_create(create(NodeKind::Page, Some(&a), None, "A1"))
        .await
        .expect("a1");
    let a2 = core
        .tree_create(create(NodeKind::Page, Some(&a), Some(0), "A2"))
        .await
        .expect("a2");
    let titles = |nodes: Vec<note_everywhere_desktop_lib::app::types::TreeNode>| {
        nodes.into_iter().map(|n| n.title).collect::<Vec<_>>()
    };
    assert_eq!(
        titles(core.tree_snapshot().await.expect("tree")),
        ["A", "A2", "A1", "B"]
    );

    // The index counts the new parent's children after the node left its old slot.
    core.tree_move(MoveNodeArgs {
        id: b.clone(),
        parent_id: Some(a.clone()),
        index: Some(1),
    })
    .await
    .expect("move");
    assert_eq!(
        titles(core.tree_snapshot().await.expect("tree")),
        ["A", "A2", "B", "A1"]
    );
    core.tree_move(MoveNodeArgs {
        id: a2,
        parent_id: Some(a.clone()),
        index: Some(99),
    })
    .await
    .expect("stale index goes last");
    assert_eq!(
        titles(core.tree_snapshot().await.expect("tree")),
        ["A", "B", "A1", "A2"]
    );

    let cycle = core
        .tree_move(MoveNodeArgs {
            id: a.clone(),
            parent_id: Some(a1.clone()),
            index: None,
        })
        .await;
    assert_eq!(cycle.expect_err("cycle").code, "cycle");
    assert_eq!(
        core.tree_rename("nonsense", "x")
            .await
            .expect_err("bad id")
            .code,
        "not_found"
    );

    core.tree_rename(&a, "Alpha").await.expect("rename");
    core.tree_set_icon(&a, Some("📁")).await.expect("icon");
    core.tree_set_favorite(&a, true).await.expect("favorite");
    let tree = core.tree_snapshot().await.expect("tree");
    let alpha = tree.iter().find(|n| n.id == a).expect("alpha");
    assert_eq!(
        (alpha.title.as_str(), alpha.icon.as_deref(), alpha.favorite),
        ("Alpha", Some("📁"), true)
    );
    assert!(!device.events.named("tree:changed").is_empty());
}

#[tokio::test]
async fn trash_restore_and_delete() {
    let device = Device::local().await;
    let core = &device.core;
    let folder = core
        .tree_create(create(NodeKind::Folder, None, None, "F"))
        .await
        .expect("f");
    let page = core
        .tree_create(create(NodeKind::Page, Some(&folder), None, "P"))
        .await
        .expect("p");
    core.tree_trash(&folder).await.expect("trash");
    assert!(core.tree_snapshot().await.expect("tree").is_empty());
    let trash = core.tree_list_trash().await.expect("trash list");
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].node.id, folder);
    assert!(trash[0].trashed_at > 0);
    core.tree_restore(&folder).await.expect("restore");
    assert_eq!(core.tree_snapshot().await.expect("tree").len(), 2);
    core.tree_trash(&folder).await.expect("trash again");
    core.tree_delete_permanently(&folder).await.expect("delete");
    assert!(core.tree_list_trash().await.expect("trash").is_empty());
    assert_eq!(
        core.page_open(&page).await.expect_err("gone").code,
        "not_found"
    );
    core.tree_trash(
        &core
            .tree_create(create(NodeKind::Page, None, None, "Q"))
            .await
            .expect("q"),
    )
    .await
    .expect("trash q");
    core.tree_empty_trash().await.expect("empty");
    assert!(core.tree_list_trash().await.expect("trash").is_empty());
}

#[tokio::test]
async fn a_new_page_queues_its_skeleton_and_edits_are_indexed() {
    let device = Device::local().await;
    let core = &device.core;
    let before = core.pending_updates();
    let page = core
        .tree_create(create(NodeKind::Page, None, None, "Notes"))
        .await
        .expect("page");
    assert!(
        core.pending_updates() > before,
        "the tree node and the page skeleton are queued"
    );

    let opened = core.page_open(&page).await.expect("open");
    assert!(!opened.snapshot.is_empty());
    let queued = core.pending_updates();
    core.page_apply_update(
        &page,
        &paragraph_update(&opened.snapshot, "ghi chú về Hà Nội"),
    )
    .await
    .expect("apply");
    assert_eq!(core.pending_updates(), queued + 1);

    let hits = core.search_query("ghi chu", None).await.expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page_id, page);
    assert_eq!(hits[0].title, "Notes");
    assert!(!hits[0].highlights.is_empty());
    // Ranges count UTF-16 units, which is how the webview indexes the snippet.
    let units: Vec<u16> = hits[0].snippet.encode_utf16().collect();
    let marked: Vec<String> = hits[0]
        .highlights
        .iter()
        .map(|&(start, end)| String::from_utf16_lossy(&units[start..end]).to_lowercase())
        .collect();
    assert_eq!(marked, ["ghi", "chú"]);

    assert_eq!(
        core.page_apply_update(&page, &[])
            .await
            .expect_err("empty")
            .code,
        "invalid_input"
    );
    assert_eq!(
        core.page_apply_update(&page, b"garbage")
            .await
            .expect_err("garbage")
            .code,
        "storage_error"
    );
    core.page_close(&page).await.expect("close");
}

#[tokio::test]
async fn backlinks_and_breadcrumbs() {
    let device = Device::local().await;
    let core = &device.core;
    let folder = core
        .tree_create(create(NodeKind::Folder, None, None, "Projects"))
        .await
        .expect("folder");
    let target = core
        .tree_create(create(NodeKind::Page, Some(&folder), None, "Target"))
        .await
        .expect("target");
    let source = core
        .tree_create(create(NodeKind::Page, None, None, "Source"))
        .await
        .expect("source");
    let snapshot = core.page_open(&source).await.expect("open").snapshot;
    let doc = PageDoc::from_snapshot(&snapshot).expect("doc");
    let link = Block::new(BlockKind::PageLink {
        page_id: target.clone(),
        title: "Target".into(),
    });
    let update = doc
        .edit(|d| {
            d.append_blocks(&[], &[Block::paragraph("see the findings"), link])
                .map(drop)
        })
        .expect("edit")
        .1;
    core.page_apply_update(&source, &update)
        .await
        .expect("apply");
    let backlinks = core.page_backlinks(&target).await.expect("backlinks");
    assert_eq!(backlinks.len(), 1);
    assert_eq!(
        (backlinks[0].page_id.as_str(), backlinks[0].title.as_str()),
        (source.as_str(), "Source")
    );
    assert!(backlinks[0].snippet.contains("see the findings"));
    let hits = core.search_query("target", None).await.expect("search");
    assert_eq!(hits[0].breadcrumb, ["Projects"]);
}

#[tokio::test]
async fn attachments_are_served_by_hash_only() {
    let device = Device::local().await;
    let core = &device.core;
    let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[7u8; 40]].concat();
    let added = core
        .attachment_add_bytes(png.clone(), "image/png", Some("../../shot.png"))
        .await
        .expect("add");
    assert_eq!(added.byte_length, png.len() as u64);
    assert_eq!(added.mime, "image/png");
    assert_eq!(
        added.url,
        format!("ne-attachment://localhost/{}", added.hash)
    );
    assert_eq!(
        core.attachment_url(&added.hash).await.expect("url"),
        added.url
    );
    assert_eq!(
        core.attachment_url("../etc/passwd")
            .await
            .expect_err("bad")
            .code,
        "invalid_input"
    );

    let whole = core
        .serve_attachment(&format!("/{}", added.hash), None)
        .await;
    assert_eq!(whole.status, 200);
    assert_eq!(whole.body, png);
    assert!(
        whole
            .headers
            .contains(&("Content-Type", "image/png".to_owned()))
    );
    let part = core
        .serve_attachment(&format!("/{}", added.hash), Some("bytes=8-11"))
        .await;
    assert_eq!((part.status, part.body.as_slice()), (206, &png[8..12]));
    assert!(
        part.headers
            .contains(&("Content-Range", format!("bytes 8-11/{}", png.len())))
    );
    assert_eq!(
        core.serve_attachment(&format!("/{}", added.hash), Some("bytes=500-"))
            .await
            .status,
        416
    );

    for bad in [
        "/..%2f..%2fetc%2fpasswd",
        "/../workspace.db",
        "/",
        "/not-a-hash",
        &format!("/{}", "0".repeat(64)),
        &format!("/{}", added.hash.to_uppercase()),
    ] {
        assert_eq!(core.serve_attachment(bad, None).await.status, 404, "{bad}");
    }

    // The same bytes are stored once, and a nameless paste gets an extension from its type.
    let again = core
        .attachment_add_bytes(png, "image/png", None)
        .await
        .expect("add again");
    assert_eq!(again.hash, added.hash);
    let note = core
        .attachment_add_bytes(b"plain".to_vec(), "text/plain", None)
        .await
        .expect("text");
    assert_eq!(note.mime, "text/plain");
    assert_eq!(
        core.attachment_add_bytes(Vec::new(), "image/png", None)
            .await
            .expect_err("empty")
            .code,
        "invalid_input"
    );
}

#[tokio::test]
async fn picking_an_attachment_uses_the_dialog() {
    let device = Device::local().await;
    assert!(
        device
            .core
            .attachment_pick(true)
            .await
            .expect("cancelled")
            .is_none()
    );
    let file = device.dir.path().join("photo.jpg");
    std::fs::write(&file, [&[0xFF, 0xD8, 0xFF][..], &[1u8; 20]].concat()).expect("write");
    device.dialogs.queue_file(Some(file));
    let added = device
        .core
        .attachment_pick(true)
        .await
        .expect("pick")
        .expect("some");
    assert_eq!(added.mime, "image/jpeg");
    assert_eq!(added.byte_length, 23);
}

#[tokio::test]
async fn markdown_round_trips_through_the_folder_pickers() {
    let device = Device::local().await;
    let core = &device.core;
    let page = core
        .tree_create(create(NodeKind::Page, None, None, "Exported"))
        .await
        .expect("page");
    let snapshot = core.page_open(&page).await.expect("open").snapshot;
    core.page_apply_update(&page, &paragraph_update(&snapshot, "hello markdown"))
        .await
        .expect("apply");

    assert!(
        core.export_markdown(None)
            .await
            .expect("cancelled")
            .is_none()
    );
    let out = device.dir.path().join("out");
    std::fs::create_dir_all(&out).expect("mkdir");
    device.dialogs.queue_folder(Some(out.clone()));
    assert_eq!(core.export_markdown(None).await.expect("export"), Some(1));
    let exported = std::fs::read_dir(&out)
        .expect("dir")
        .next()
        .expect("one folder")
        .expect("entry")
        .path();
    let text = std::fs::read_to_string(exported.join("Exported.md")).expect("file");
    assert!(text.contains("hello markdown"));

    device.dialogs.queue_folder(Some(exported));
    assert_eq!(core.import_markdown(None).await.expect("import"), Some(1));
    let titles: Vec<String> = core
        .tree_snapshot()
        .await
        .expect("tree")
        .into_iter()
        .map(|n| n.title)
        .collect();
    assert_eq!(
        titles.iter().filter(|t| t.as_str() == "Exported").count(),
        2
    );
    assert!(
        core.import_markdown(None)
            .await
            .expect("cancelled")
            .is_none()
    );
}

#[tokio::test]
async fn settings_persist_validate_and_register_the_hotkey() {
    let device = Device::local().await;
    let defaults = device.core.settings_get().await.expect("get");
    assert_eq!(
        (defaults.theme.as_str(), defaults.vault_idle_lock_seconds),
        ("system", 300)
    );
    let updated = device
        .core
        .settings_set(SettingsPatch {
            theme: Some("dark".into()),
            voice_keep_audio: Some(true),
            ..SettingsPatch::default()
        })
        .await
        .expect("set");
    assert_eq!(
        (updated.theme.as_str(), updated.voice_keep_audio),
        ("dark", true)
    );
    for bad in [
        SettingsPatch {
            theme: Some("neon".into()),
            ..SettingsPatch::default()
        },
        SettingsPatch {
            voice_language: Some("de".into()),
            ..SettingsPatch::default()
        },
        SettingsPatch {
            vault_idle_lock_seconds: Some(10 * 24 * 3600),
            ..SettingsPatch::default()
        },
        SettingsPatch {
            voice_hotkey: Some(String::new()),
            ..SettingsPatch::default()
        },
    ] {
        assert_eq!(
            device
                .core
                .settings_set(bad)
                .await
                .expect_err("invalid")
                .code,
            "invalid_input"
        );
    }
    // The theme survives a restart together with the data.
    let page = device
        .core
        .tree_create(create(NodeKind::Page, None, None, "Kept"))
        .await
        .expect("page");
    let device = device.restart().await;
    let settings = device.core.settings_get().await.expect("get");
    assert_eq!(
        (settings.theme.as_str(), settings.voice_keep_audio),
        ("dark", true)
    );
    assert_eq!(
        device.core.session_status().await.expect("status").state,
        SessionState::Local
    );
    assert!(
        device
            .core
            .tree_snapshot()
            .await
            .expect("tree")
            .iter()
            .any(|n| n.id == page)
    );
    assert!(
        device
            .events
            .all()
            .iter()
            .all(|e| !matches!(e, CoreEvent::VoiceHotkey { .. }))
    );
}

#[tokio::test]
async fn settings_work_before_a_workspace_exists() {
    let device = Device::new().await;
    device
        .core
        .settings_set(SettingsPatch {
            theme: Some("light".into()),
            ..SettingsPatch::default()
        })
        .await
        .expect("set");
    device.core.workspace_create_local().await.expect("create");
    let device = device.restart().await;
    assert_eq!(
        device.core.settings_get().await.expect("get").theme,
        "light"
    );
}

#[cfg(feature = "transcribe")]
#[tokio::test]
async fn changing_the_hotkey_registers_it_or_rejects_it() {
    let device = Device::local().await;
    device
        .core
        .settings_set(SettingsPatch {
            voice_hotkey: Some("Alt+Space".into()),
            ..SettingsPatch::default()
        })
        .await
        .expect("set");
    assert_eq!(
        device.hotkeys.0.lock().expect("lock").as_slice(),
        ["Alt+Space"]
    );
    let bad = device
        .core
        .settings_set(SettingsPatch {
            voice_hotkey: Some("bogus".into()),
            ..SettingsPatch::default()
        })
        .await;
    assert_eq!(bad.expect_err("rejected").code, "invalid_hotkey");
    assert_eq!(
        device.core.settings_get().await.expect("get").voice_hotkey,
        "Alt+Space"
    );
}

#[cfg(not(feature = "transcribe"))]
#[tokio::test]
async fn voice_is_unavailable_without_the_feature() {
    let device = Device::local().await;
    let status = device.core.voice_status().await.expect("status");
    assert!(!status.available && !status.recording && status.models.is_empty());
    assert_eq!(
        device
            .core
            .voice_start()
            .await
            .expect_err("unavailable")
            .code,
        "voice_unavailable"
    );
    assert_eq!(
        device
            .core
            .voice_select_model("small-q5_1")
            .await
            .expect_err("unavailable")
            .code,
        "voice_unavailable"
    );
}

#[cfg(feature = "transcribe")]
#[tokio::test]
async fn voice_reports_models_and_needs_one_to_record() {
    let device = Device::local().await;
    let status = device.core.voice_status().await.expect("status");
    assert!(status.available && !status.recording);
    assert_eq!(status.models.len(), 2);
    assert!(status.models.iter().all(|m| !m.installed) && status.active_model.is_none());
    assert_eq!(
        device.core.voice_start().await.expect_err("no model").code,
        "voice_model_missing"
    );
    assert_eq!(
        device
            .core
            .voice_select_model("small-q5_1")
            .await
            .expect_err("not installed")
            .code,
        "voice_model_missing"
    );
    assert_eq!(
        device
            .core
            .voice_select_model("nope")
            .await
            .expect_err("unknown")
            .code,
        "unknown_model"
    );
}
