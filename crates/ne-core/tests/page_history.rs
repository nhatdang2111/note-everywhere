//! Page history: versions, previews, restore as a forward change, trimming.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use ne_core::page::{Block, BlockKind, HISTORY_SESSION_GAP_SECS, PageDoc};

struct Clock(Arc<AtomicI64>);

impl Clock {
    fn new() -> (Self, Arc<AtomicI64>) {
        let now = Arc::new(AtomicI64::new(1_700_000_000_000));
        (Self(now.clone()), now)
    }
}

impl ne_core::tree::Clock for Clock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn page(peer: u64) -> (PageDoc, Arc<AtomicI64>) {
    let (clock, now) = Clock::new();
    let doc = PageDoc::with_peer_id(peer)
        .expect("page")
        .with_clock(Arc::new(clock));
    (doc, now)
}

fn advance(now: &AtomicI64, secs: i64) {
    now.fetch_add(secs * 1000, Ordering::SeqCst);
}

fn texts(doc: &PageDoc) -> Vec<String> {
    doc.blocks()
        .expect("blocks")
        .iter()
        .map(Block::plain_text)
        .filter(|t| !t.is_empty())
        .collect()
}

fn texts_of(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(Block::plain_text)
        .filter(|t| !t.is_empty())
        .collect()
}

fn add(doc: &PageDoc, text: &str) {
    doc.append_block(&[], &Block::paragraph(text))
        .expect("append");
}

#[test]
fn changes_group_into_versions_by_time_gap() {
    let (doc, now) = page(10);
    add(&doc, "one");
    advance(&now, 60);
    add(&doc, "two");
    advance(&now, HISTORY_SESSION_GAP_SECS + 1);
    add(&doc, "three");
    advance(&now, 60);
    add(&doc, "four");
    let history = doc.history().expect("history");
    assert!(!history.trimmed);
    // The skeleton, then {one, two}, then {three, four}; newest first.
    assert_eq!(history.versions.len(), 3, "{:?}", history.versions);
    let newest = &history.versions[0];
    assert_eq!(newest.changes, 2);
    let (start, end) = (
        newest.started_at.expect("start"),
        newest.ended_at.expect("end"),
    );
    assert_eq!(end - start, 60_000);
    assert_eq!(
        history.versions[1].ended_at.expect("end") - history.versions[1].started_at.expect("start"),
        60_000
    );
    assert_eq!(
        history.versions[2].started_at, None,
        "the skeleton carries no time"
    );
    assert_eq!(history.oldest_at, history.versions[1].started_at);
}

#[test]
fn a_version_previews_as_the_page_was_then() {
    let (doc, now) = page(10);
    add(&doc, "one");
    advance(&now, 400);
    add(&doc, "two");
    advance(&now, 400);
    doc.delete_block(&[1]).expect("delete");
    let history = doc.history().expect("history");
    assert_eq!(history.versions.len(), 4);
    // Newest first: after the delete, after "two", after "one", skeleton.
    let at = |i: usize| texts_of(&doc.blocks_at(&history.versions[i].id).expect("preview"));
    assert_eq!(at(0), ["two"]);
    assert_eq!(at(1), ["one", "two"]);
    assert_eq!(at(2), ["one"]);
    assert!(at(3).is_empty());
    assert_eq!(texts(&doc), ["two"], "previewing changes nothing");
    assert!(doc.blocks_at("zz").is_err());
    assert!(doc.blocks_at("00").is_err());
}

#[test]
fn restoring_is_a_new_change_other_devices_receive() {
    let (a, now) = page(10);
    add(&a, "keep");
    advance(&now, 400);
    add(&a, "later");
    let history = a.history().expect("history");
    let before = history.versions[1].clone();
    let b = PageDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("b");
    b.set_peer_id(20).expect("peer");
    let version = a.version();

    advance(&now, 400);
    a.restore_to(&before.id).expect("restore");
    assert_eq!(texts(&a), ["keep"]);

    let after = a.history().expect("history");
    assert!(
        after.versions.len() > history.versions.len(),
        "history only grows"
    );
    assert_eq!(
        texts_of(&a.blocks_at(&history.versions[0].id).expect("old head")),
        ["keep", "later"],
        "the version that was left is still there"
    );
    let update = a.export_updates_since(&b.version()).expect("update");
    assert!(!update.is_empty());
    b.import(&update).expect("import");
    assert_eq!(texts(&b), ["keep"], "the restore reaches the other device");
    assert_ne!(version, a.version());
    // Restoring the restore brings the later text back.
    let newest_before_restore = after.versions[1].clone();
    a.restore_to(&newest_before_restore.id)
        .expect("undo the restore");
    assert_eq!(texts(&a), ["keep", "later"]);
}

#[test]
fn authors_make_separate_versions() {
    let (a, now_a) = page(10);
    add(&a, "from a");
    let b = PageDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("b");
    b.set_peer_id(20).expect("peer");
    advance(&now_a, 100);
    add(&a, "a again");
    add(&b, "from b");
    let to_a = b.export_updates_since(&a.version()).expect("b to a");
    a.import(&to_a).expect("import");
    let history = a.history().expect("history");
    let authors: std::collections::BTreeSet<_> =
        history.versions.iter().map(|v| v.author.clone()).collect();
    assert!(
        authors.contains("10") && authors.contains("20"),
        "{authors:?}"
    );
    assert!(history.versions.len() >= 4, "{:?}", history.versions);
}

#[test]
fn restore_refuses_a_version_with_unknown_blocks_it_cannot_write() {
    let (doc, _) = page(10);
    let err = doc
        .replace_blocks(&[Block::new(BlockKind::Unknown {
            node_name: "weird".into(),
        })])
        .expect_err("unknown");
    assert!(err.to_string().contains("unknown"));
    doc.replace_blocks(&[]).expect("empty keeps a paragraph");
    assert_eq!(doc.blocks().expect("blocks").len(), 1);
}

#[test]
fn trimming_bounds_history_and_is_reported() {
    let (doc, now) = page(10);
    add(&doc, "old");
    advance(&now, 400);
    add(&doc, "middle");
    advance(&now, 400);
    add(&doc, "new");
    let history = doc.history().expect("history");
    assert!(!history.trimmed);
    let keep_from = &history.versions[1]; // the state after "middle"
    let shallow = doc
        .export_shallow_snapshot_at(&keep_from.id)
        .expect("shallow");
    let trimmed = PageDoc::from_snapshot(&shallow).expect("load");
    trimmed.set_peer_id(30).expect("peer");
    assert_eq!(
        texts(&trimmed),
        ["old", "middle", "new"],
        "the current state is whole"
    );
    let report = trimmed.history().expect("history");
    assert!(report.trimmed);
    assert!(
        !report.previewable,
        "a trimmed copy cannot rebuild past versions"
    );
    assert!(report.versions.len() < history.versions.len());
    assert!(report.versions.last().expect("oldest").started_at.is_some());
    assert!(trimmed.blocks_at(&report.versions[0].id).is_err());
    assert!(trimmed.restore_to(&report.versions[0].id).is_err());
    assert!(history.previewable);
}

#[test]
fn a_trimmed_replica_merges_only_with_replicas_at_or_past_the_trim_point() {
    let (a, now) = page(10);
    add(&a, "old");
    advance(&now, 400);
    add(&a, "middle");
    let behind = PageDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("behind");
    behind.set_peer_id(20).expect("peer");
    advance(&now, 400);
    add(&a, "new");
    let current = PageDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("current");
    current.set_peer_id(21).expect("peer");
    let history = a.history().expect("history");
    let shallow = a
        .export_shallow_snapshot_at(&history.versions[0].id)
        .expect("shallow");
    let trimmed = PageDoc::from_snapshot(&shallow).expect("load");
    trimmed.set_peer_id(30).expect("peer");

    // A replica that has seen the trim point exchanges edits both ways.
    add(&current, "from current");
    let update = current
        .export_updates_since(&trimmed.version())
        .expect("update");
    trimmed.import(&update).expect("import into trimmed");
    assert!(texts(&trimmed).contains(&"from current".to_owned()));
    let known = current.version();
    add(&trimmed, "after trim");
    let back = trimmed
        .export_updates_since(&known)
        .expect("trimmed update");
    current.import(&back).expect("import into full");
    assert!(texts(&current).contains(&"after trim".to_owned()));

    // A replica behind the trim point edits on top of history the trimmed copy
    // no longer has: Loro refuses the import. This is why nothing trims
    // history automatically.
    add(&behind, "from behind");
    let update = behind
        .export_updates_since(&trimmed.version())
        .expect("update");
    assert!(trimmed.import(&update).is_err());
}
