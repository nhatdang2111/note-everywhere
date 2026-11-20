//! Replicas of one page converge: concurrent edits to one paragraph merge by
//! character, edits to different blocks both survive, and random edit and sync
//! interleavings always end in the same document.

mod page_support;

use ne_core::page::{Block, BlockKind, Inline, PageDoc};
use page_support::{bold, every_kind, fill, pull, replica, run, sync_all};
use proptest::prelude::*;

fn texts(doc: &PageDoc) -> Vec<String> {
    doc.blocks()
        .expect("blocks")
        .iter()
        .map(Block::plain_text)
        .collect()
}

fn seeded() -> (PageDoc, PageDoc) {
    let a = replica(1);
    fill(
        &a,
        &[Block::paragraph("Xin chào"), Block::paragraph("thế giới")],
    );
    let b = replica(2);
    pull(&b, &a);
    (a, b)
}

#[test]
fn same_paragraph_edits_merge_by_character() {
    let (a, b) = seeded();
    a.insert_text(&[0], 0, "[A] ").expect("a edits");
    b.insert_text(&[0], "Xin chào".chars().count(), " [B]")
        .expect("b edits");
    sync_all(&[&a, &b]);
    assert_eq!(texts(&a), texts(&b));
    assert_eq!(texts(&a)[0], "[A] Xin chào [B]");
}

#[test]
fn concurrent_inserts_at_the_same_spot_keep_both_texts() {
    let (a, b) = seeded();
    a.insert_text(&[1], 0, "aaa").expect("a edits");
    b.insert_text(&[1], 0, "bbb").expect("b edits");
    sync_all(&[&a, &b]);
    let merged = texts(&a)[1].clone();
    assert_eq!(merged.len(), "aaabbbthế giới".len());
    assert!(merged.ends_with("thế giới"));
    assert!(
        merged.contains("aaa") && merged.contains("bbb"),
        "no interleaving: {merged}"
    );
    assert_eq!(texts(&a), texts(&b));
}

#[test]
fn different_blocks_edited_concurrently_both_survive() {
    let (a, b) = seeded();
    a.insert_text(&[0], 0, ">> ").expect("a edits block 0");
    b.insert_text(&[1], 0, "<< ").expect("b edits block 1");
    b.append_block(&[], &Block::paragraph("từ b"))
        .expect("b appends");
    a.append_block(&[], &Block::paragraph("từ a"))
        .expect("a appends");
    sync_all(&[&a, &b]);
    assert_eq!(texts(&a), texts(&b));
    let merged = texts(&a);
    assert_eq!(&merged[..2], [">> Xin chào", "<< thế giới"]);
    assert_eq!(merged.len(), 4);
    assert!(merged.contains(&"từ a".to_string()) && merged.contains(&"từ b".to_string()));
}

#[test]
fn a_mark_and_a_text_edit_on_the_same_block_both_apply() {
    let (a, b) = seeded();
    a.set_inline(&[0], &[run("Xin chào", bold())])
        .expect("a bolds");
    b.insert_text(&[1], 0, "- ").expect("b edits next block");
    sync_all(&[&a, &b]);
    assert_eq!(a.blocks().expect("a"), b.blocks().expect("b"));
    let first = &a.blocks().expect("a")[0];
    assert!(matches!(&first.inline[..], [Inline::Text(r)] if r.marks.bold && r.text == "Xin chào"));
}

#[test]
fn todo_toggles_and_deletions_converge() {
    let a = replica(1);
    fill(&a, &every_kind());
    let b = replica(2);
    pull(&b, &a);
    a.set_todo_checked(&[6, 1], true).expect("a ticks");
    b.set_todo_checked(&[6, 0], false).expect("b unticks");
    b.delete_block(&[9]).expect("b deletes the callout");
    a.insert_text(&[9, 0], 0, "!")
        .expect("a edits the callout meanwhile");
    sync_all(&[&a, &b]);
    assert_eq!(a.blocks().expect("a"), b.blocks().expect("b"));
    assert_eq!(
        a.block(&[6, 1]).expect("read").kind,
        BlockKind::TodoItem { checked: true }
    );
}

#[test]
fn snapshots_exchanged_after_divergence_converge() {
    let (a, b) = seeded();
    a.insert_text(&[0], 0, "1").expect("edit");
    b.insert_text(&[0], 0, "2").expect("edit");
    let snapshot_a = a.export_snapshot().expect("snapshot");
    let snapshot_b = b.export_snapshot().expect("snapshot");
    a.import(&snapshot_b).expect("import");
    b.import(&snapshot_a).expect("import");
    assert_eq!(a.blocks().expect("a"), b.blocks().expect("b"));
}

#[test]
fn import_reports_missing_dependencies() {
    let (a, _b) = seeded();
    a.insert_text(&[0], 0, "một").expect("edit one");
    let mid = a.version();
    a.insert_text(&[0], 0, "hai").expect("edit two");
    let later = a.export_updates_since(&mid).expect("export later update");

    let fresh = replica(3);
    let outcome = fresh.import(&later).expect("import");
    assert!(
        outcome.has_missing_dependencies,
        "the update needs earlier history"
    );
}

#[derive(Debug, Clone)]
enum Action {
    Insert {
        block: usize,
        offset: usize,
        text: u8,
    },
    Replace {
        block: usize,
        bold: bool,
        text: u8,
    },
    Append {
        text: u8,
    },
    Delete {
        block: usize,
    },
    Tick {
        block: usize,
    },
    Sync {
        from: usize,
    },
}

#[derive(Debug, Clone)]
struct Step {
    replica: usize,
    action: Action,
}

const SNIPPETS: [&str; 6] = ["a", "é", "ê\u{0301}", "đ", "😀", " từ "];

fn snippet(n: u8) -> &'static str {
    SNIPPETS[usize::from(n) % SNIPPETS.len()]
}

fn step() -> impl Strategy<Value = Step> {
    let action = prop_oneof![
        4 => (0..8usize, 0..40usize, any::<u8>())
            .prop_map(|(block, offset, text)| Action::Insert { block, offset, text }),
        2 => (0..8usize, any::<bool>(), any::<u8>())
            .prop_map(|(block, bold, text)| Action::Replace { block, bold, text }),
        2 => any::<u8>().prop_map(|text| Action::Append { text }),
        1 => (0..8usize).prop_map(|block| Action::Delete { block }),
        1 => (0..8usize).prop_map(|block| Action::Tick { block }),
        3 => (0..3usize).prop_map(|from| Action::Sync { from }),
    ];
    (0..3usize, action).prop_map(|(replica, action)| Step { replica, action })
}

/// Applies one step. Edits that are illegal in the replica's current state (a
/// missing block, a schema rule) are skipped, which is what a UI would do.
fn apply(replicas: &[PageDoc], step: &Step) {
    let doc = &replicas[step.replica];
    let blocks = doc.blocks().expect("blocks");
    let pick = |n: usize| n % blocks.len().max(1);
    match &step.action {
        Action::Insert {
            block,
            offset,
            text,
        } => {
            if blocks.is_empty() {
                return;
            }
            let index = pick(*block);
            if !blocks[index].kind.is_text_block() {
                return;
            }
            let len = blocks[index].plain_text().chars().count();
            doc.insert_text(&[index], offset % (len + 1), snippet(*text))
                .expect("insert at a valid offset");
        }
        Action::Replace { block, bold, text } => {
            if blocks.is_empty() || !blocks[pick(*block)].kind.is_text_block() {
                return;
            }
            let marks = ne_core::page::Marks {
                bold: *bold,
                ..Default::default()
            };
            doc.set_inline(
                &[pick(*block)],
                &[
                    Inline::Text(ne_core::page::Run {
                        text: snippet(*text).into(),
                        marks,
                    }),
                    Inline::Break,
                    Inline::text("x"),
                ],
            )
            .expect("replace");
        }
        Action::Append { text } => {
            doc.append_block(&[], &Block::paragraph(snippet(*text)))
                .expect("append");
        }
        Action::Delete { block } => {
            if !blocks.is_empty() {
                let _ = doc.delete_block(&[pick(*block)]);
            }
        }
        Action::Tick { block } => {
            if blocks.is_empty() {
                return;
            }
            let index = pick(*block);
            if matches!(blocks[index].kind, BlockKind::TodoList) {
                let checked = !matches!(
                    blocks[index].children[0].kind,
                    BlockKind::TodoItem { checked: true }
                );
                doc.set_todo_checked(&[index, 0], checked).expect("tick");
            }
        }
        Action::Sync { from } => {
            if *from != step.replica {
                pull(doc, &replicas[*from]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_edits_and_syncs_converge(steps in proptest::collection::vec(step(), 1..60)) {
        let a = replica(1);
        fill(&a, &[
            Block::paragraph("Xin chào"),
            Block::paragraph("thế giới"),
            Block::new(BlockKind::TodoList).with_children(vec![Block::new(BlockKind::TodoItem { checked: false })
                .with_children(vec![Block::paragraph("việc")])]),
        ]);
        let b = replica(2);
        let c = replica(3);
        pull(&b, &a);
        pull(&c, &a);
        let replicas = [a, b, c];
        for step in &steps {
            apply(&replicas, step);
        }
        sync_all(&[&replicas[0], &replicas[1], &replicas[2]]);
        let first = replicas[0].blocks().expect("blocks");
        prop_assert_eq!(&first, &replicas[1].blocks().expect("blocks"));
        prop_assert_eq!(&first, &replicas[2].blocks().expect("blocks"));
    }
}
