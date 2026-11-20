//! Attachments: content addressing, deduplication, reference counts and
//! garbage collection.

mod store_support;

use std::fs;
use std::time::Duration;

use ne_core::page::{Block, BlockKind, attachment_src};
use ne_store::{Error, Workspace, is_valid_hash};
use sha2::{Digest, Sha256};
use store_support::*;

fn image(hash: &str) -> Block {
    Block::new(BlockKind::Image {
        src: attachment_src(hash),
        alt: "ảnh".into(),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn blob_files(ws: &Workspace) -> Vec<String> {
    let root = ws.dir().join(ne_store::ATTACHMENTS_DIR);
    let mut found = Vec::new();
    for shard in fs::read_dir(&root).expect("read attachments dir").flatten() {
        if shard.file_name() == "tmp" {
            continue;
        }
        for file in fs::read_dir(shard.path()).expect("read shard").flatten() {
            found.push(file.file_name().to_string_lossy().into_owned());
        }
    }
    found.sort();
    found
}

#[test]
fn content_is_addressed_by_sha256_and_stored_beside_the_database() {
    let (dir, mut ws) = new_workspace();
    let bytes = b"\x89PNG not really a picture, but bytes".to_vec();
    let info = ws.add_attachment("Sơ đồ.png", &bytes).expect("add");
    assert_eq!(info.hash, sha256_hex(&bytes));
    assert!(is_valid_hash(&info.hash));
    assert_eq!((info.size, info.ref_count), (bytes.len() as u64, 0));
    assert_eq!(info.file_name, "Sơ đồ.png");

    let path = ws.attachment_path(&info.hash).expect("path");
    assert!(path.starts_with(dir.path().join("attachments")));
    assert_eq!(
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str()),
        Some(&info.hash[..2])
    );
    assert_eq!(fs::read(&path).expect("read file"), bytes);
    assert_eq!(ws.read_attachment(&info.hash).expect("read"), bytes);
    let db_size = fs::metadata(dir.path().join(ne_store::DB_FILE))
        .expect("db")
        .len();
    let big = vec![7u8; 400_000];
    ws.add_attachment("big.bin", &big).expect("add big");
    let after = fs::metadata(dir.path().join(ne_store::DB_FILE))
        .expect("db")
        .len();
    assert!(
        after < db_size + 50_000,
        "blob bytes are not stored inside the database"
    );
}

#[test]
fn identical_content_is_stored_once() {
    let (_dir, mut ws) = new_workspace();
    let first = ws.add_attachment("a.txt", b"same bytes").expect("add");
    let second = ws
        .add_attachment("copy of a.txt", b"same bytes")
        .expect("add again");
    assert_eq!(
        first, second,
        "the first name wins and nothing new is recorded"
    );
    assert_eq!(blob_files(&ws), std::slice::from_ref(&first.hash));
    let other = ws
        .add_attachment("b.txt", b"other bytes")
        .expect("add other");
    assert_ne!(other.hash, first.hash);
    assert_eq!(blob_files(&ws).len(), 2);
}

#[test]
fn files_are_added_from_a_path_without_loading_them_whole() {
    let (dir, mut ws) = new_workspace();
    let source = dir.path().join("source.bin");
    let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    fs::write(&source, &content).expect("write source");
    let info = ws.add_attachment_from_path(&source).expect("add from path");
    assert_eq!(info.hash, sha256_hex(&content));
    assert_eq!(info.file_name, "source.bin");
    assert_eq!(info.size, content.len() as u64);
    assert_eq!(ws.read_attachment(&info.hash).expect("read"), content);
    let again = ws.add_attachment_from_path(&source).expect("add again");
    assert_eq!(again, info);
    let via_bytes = ws.add_attachment("x", &content).expect("same content");
    assert_eq!(via_bytes.hash, info.hash);
    assert!(matches!(
        ws.add_attachment_from_path(&dir.path().join("missing.bin")),
        Err(Error::Io { .. })
    ));
}

#[test]
fn bad_hashes_never_touch_the_file_system() {
    let (_dir, mut ws) = new_workspace();
    for bad in [
        "",
        "abc",
        "../../etc/passwd",
        &"A".repeat(64),
        &"g".repeat(64),
        &format!("{}/", "a".repeat(63)),
    ] {
        assert!(!is_valid_hash(bad));
        assert!(
            matches!(ws.attachment(bad), Err(Error::InvalidInput(_))),
            "{bad:?}"
        );
        assert!(ws.read_attachment(bad).is_err());
    }
    let missing = "a".repeat(64);
    assert!(ws.attachment(&missing).expect("lookup").is_none());
    assert!(matches!(
        ws.read_attachment(&missing),
        Err(Error::NotFound(_))
    ));
    let info = ws.add_attachment("x", b"x").expect("add");
    fs::remove_file(ws.attachment_path(&info.hash).expect("path")).expect("remove file");
    assert!(matches!(
        ws.read_attachment(&info.hash),
        Err(Error::NotFound(_))
    ));
    // Adding again heals the missing file.
    ws.add_attachment("x", b"x").expect("add again");
    assert_eq!(ws.read_attachment(&info.hash).expect("read"), b"x");
}

#[test]
fn a_truncated_file_is_reported_as_corrupt() {
    let (_dir, mut ws) = new_workspace();
    let info = ws.add_attachment("x", b"twelve bytes").expect("add");
    fs::write(ws.attachment_path(&info.hash).expect("path"), b"short").expect("truncate");
    assert!(matches!(
        ws.read_attachment(&info.hash),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn reference_counts_follow_page_content() {
    let (dir, mut ws) = new_workspace();
    let logo = ws.add_attachment("logo.png", b"logo").expect("add");
    let photo = ws.add_attachment("photo.jpg", b"photo").expect("add");
    let count = |ws: &Workspace, hash: &str| {
        ws.attachment(hash)
            .expect("info")
            .expect("stored")
            .ref_count
    };
    assert_eq!((count(&ws, &logo.hash), count(&ws, &photo.hash)), (0, 0));

    let a = page_with(
        &mut ws,
        "A",
        &[image(&logo.hash), image(&logo.hash), image(&photo.hash)],
    );
    let b = page_with(&mut ws, "B", &[image(&logo.hash)]);
    assert_eq!(
        count(&ws, &logo.hash),
        2,
        "pages count once however often they show it"
    );
    assert_eq!(count(&ws, &photo.hash), 1);

    ws.edit_page(a, |d| d.delete_block(&[2]))
        .expect("remove the photo");
    assert_eq!(count(&ws, &photo.hash), 0);
    assert_eq!(count(&ws, &logo.hash), 2);

    // Counts survive a restart and the page being compacted.
    drop(ws);
    let mut ws = reopen(dir.path());
    assert_eq!(count(&ws, &logo.hash), 2);
    ws.trash_node(b).expect("trash");
    ws.delete_permanently(b).expect("delete");
    assert_eq!(
        count(&ws, &logo.hash),
        1,
        "deleting a page releases its references"
    );
    ws.rebuild_indexes().expect("rebuild");
    assert_eq!((count(&ws, &logo.hash), count(&ws, &photo.hash)), (1, 0));
}

#[test]
fn a_reference_added_before_the_blob_is_counted_when_the_blob_arrives() {
    let (_dir, mut ws) = new_workspace();
    let hash = sha256_hex(b"late blob");
    page_with(&mut ws, "Có ảnh", &[image(&hash)]);
    let info = ws.add_attachment("late.bin", b"late blob").expect("add");
    assert_eq!(info.hash, hash);
    assert_eq!(info.ref_count, 1);
}

#[test]
fn garbage_collection_keeps_referenced_and_recent_blobs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let clock = TestClock::new(1_000_000);
    let mut ws = Workspace::create(dir.path(), config_with(&clock)).expect("create");
    let used = ws.add_attachment("used.png", b"used").expect("add");
    let unused = ws.add_attachment("unused.png", b"unused").expect("add");
    page_with(&mut ws, "P", &[image(&used.hash)]);

    // Both are younger than the grace period, so nothing is collected.
    let report = ws.gc_attachments(Duration::from_secs(60)).expect("gc");
    assert_eq!((report.removed, report.orphan_files), (0, 0));
    assert!(ws.attachment(&unused.hash).expect("info").is_some());

    clock.advance(120_000);
    let report = ws.gc_attachments(Duration::from_secs(60)).expect("gc");
    assert_eq!(report.removed, 1);
    assert_eq!(report.bytes_freed, "unused".len() as u64);
    assert!(ws.attachment(&unused.hash).expect("info").is_none());
    assert!(ws.attachment(&used.hash).expect("info").is_some());
    assert_eq!(blob_files(&ws), std::slice::from_ref(&used.hash));
    assert_eq!(ws.read_attachment(&used.hash).expect("read"), b"used");
}

#[test]
fn garbage_collection_removes_blobs_whose_last_page_is_gone() {
    let (_dir, mut ws) = new_workspace();
    let info = ws.add_attachment("x.png", b"x-bytes").expect("add");
    let page = page_with(&mut ws, "P", &[image(&info.hash)]);
    assert_eq!(ws.gc_attachments(Duration::ZERO).expect("gc").removed, 0);
    ws.trash_node(page).expect("trash");
    // Trashed pages still show their images.
    assert_eq!(ws.gc_attachments(Duration::ZERO).expect("gc").removed, 0);
    ws.delete_permanently(page).expect("delete");
    let report = ws.gc_attachments(Duration::ZERO).expect("gc");
    assert_eq!(report.removed, 1);
    assert!(blob_files(&ws).is_empty());
}

#[test]
fn stray_files_left_by_an_interrupted_add_are_swept() {
    let (_dir, mut ws) = new_workspace();
    let info = ws.add_attachment("x", b"known").expect("add");
    page_with(&mut ws, "P", &[image(&info.hash)]);
    // A blob that reached the disk but never got its database row.
    let stray_hash = sha256_hex(b"stray");
    let shard = ws
        .dir()
        .join(ne_store::ATTACHMENTS_DIR)
        .join(&stray_hash[..2]);
    fs::create_dir_all(&shard).expect("shard");
    fs::write(shard.join(&stray_hash), b"stray").expect("stray file");
    // Files that are not attachments are left alone.
    fs::write(shard.join("notes.txt"), b"mine").expect("other file");

    let report = ws.gc_attachments(Duration::ZERO).expect("gc");
    assert_eq!((report.removed, report.orphan_files), (0, 1));
    assert!(!shard.join(&stray_hash).exists());
    assert!(shard.join("notes.txt").exists());
    assert_eq!(ws.read_attachment(&info.hash).expect("read"), b"known");
}

#[test]
fn leftover_temporary_files_are_cleared_on_open() {
    let (dir, ws) = new_workspace();
    let tmp = ws.dir().join(ne_store::ATTACHMENTS_DIR).join("tmp");
    fs::write(tmp.join("half-written"), b"partial").expect("tmp file");
    drop(ws);
    let _ws = reopen(dir.path());
    assert!(fs::read_dir(&tmp).expect("tmp dir").next().is_none());
}
