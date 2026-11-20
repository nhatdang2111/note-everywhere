//! After any sequence of local edits, remote updates, restarts and
//! compactions, every derived index equals a full rebuild from the documents.

mod store_support;

use std::sync::Arc;

use ne_core::NodeId;
use ne_core::page::{Block, BlockKind, Inline, Marks, Run, attachment_src, page_href};
use ne_core::tree::{NodeKind, Position};
use ne_store::{StoreConfig, Workspace};
use proptest::prelude::*;
use store_support::*;

const WORDS: [&str; 10] = [
    "ghi chú",
    "đường",
    "Hà Nội",
    "kế hoạch",
    "tuần",
    "mì",
    "quả",
    "nghiên cứu",
    "Đà Lạt",
    "plan",
];

#[derive(Debug, Clone)]
enum Action {
    CreateFolder {
        parent: usize,
        pos: usize,
    },
    CreatePage {
        parent: usize,
        pos: usize,
        word: usize,
    },
    Rename {
        node: usize,
        word: usize,
    },
    Icon {
        node: usize,
        on: bool,
    },
    Favorite {
        node: usize,
        on: bool,
    },
    Move {
        node: usize,
        parent: usize,
        pos: usize,
    },
    Reorder {
        node: usize,
        pos: usize,
    },
    Trash {
        node: usize,
    },
    Restore {
        node: usize,
    },
    DeletePermanently {
        node: usize,
    },
    EmptyTrash,
    Paragraph {
        node: usize,
        word: usize,
    },
    PageLink {
        node: usize,
        target: usize,
    },
    InlineLink {
        node: usize,
        target: usize,
    },
    Image {
        node: usize,
        which: usize,
    },
    RemoveBlock {
        node: usize,
    },
    Replace {
        node: usize,
        word: usize,
    },
    Compact {
        node: usize,
    },
}

#[derive(Debug, Clone)]
enum Step {
    Local(Action),
    OtherDevice(Action),
    Restart,
    Sync,
}

fn action() -> impl Strategy<Value = Action> {
    let n = 0..40usize;
    prop_oneof![
        2 => (n.clone(), n.clone()).prop_map(|(parent, pos)| Action::CreateFolder { parent, pos }),
        4 => (n.clone(), n.clone(), 0..10usize).prop_map(|(parent, pos, word)| Action::CreatePage { parent, pos, word }),
        2 => (n.clone(), 0..10usize).prop_map(|(node, word)| Action::Rename { node, word }),
        1 => (n.clone(), any::<bool>()).prop_map(|(node, on)| Action::Icon { node, on }),
        1 => (n.clone(), any::<bool>()).prop_map(|(node, on)| Action::Favorite { node, on }),
        3 => (n.clone(), n.clone(), n.clone()).prop_map(|(node, parent, pos)| Action::Move { node, parent, pos }),
        2 => (n.clone(), n.clone()).prop_map(|(node, pos)| Action::Reorder { node, pos }),
        3 => n.clone().prop_map(|node| Action::Trash { node }),
        2 => n.clone().prop_map(|node| Action::Restore { node }),
        1 => n.clone().prop_map(|node| Action::DeletePermanently { node }),
        1 => Just(Action::EmptyTrash),
        4 => (n.clone(), 0..10usize).prop_map(|(node, word)| Action::Paragraph { node, word }),
        2 => (n.clone(), n.clone()).prop_map(|(node, target)| Action::PageLink { node, target }),
        2 => (n.clone(), n.clone()).prop_map(|(node, target)| Action::InlineLink { node, target }),
        2 => (n.clone(), 0..3usize).prop_map(|(node, which)| Action::Image { node, which }),
        2 => n.clone().prop_map(|node| Action::RemoveBlock { node }),
        2 => (n.clone(), 0..10usize).prop_map(|(node, word)| Action::Replace { node, word }),
        1 => n.prop_map(|node| Action::Compact { node }),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        8 => action().prop_map(Step::Local),
        3 => action().prop_map(Step::OtherDevice),
        1 => Just(Step::Restart),
        2 => Just(Step::Sync),
    ]
}

/// Every node the tree knows, visible or trashed.
fn all_nodes(ws: &Workspace) -> Vec<(NodeId, NodeKind)> {
    let mut nodes: Vec<_> = ws
        .tree()
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|n| (n.id, n.kind))
        .collect();
    for top in ws.trash().expect("trash") {
        nodes.push((top.id, top.kind));
        for id in ws.tree().descendants(top.id).expect("descendants") {
            let info = ws.tree().get(id).expect("node");
            nodes.push((id, info.kind));
        }
    }
    nodes
}

fn pick(nodes: &[(NodeId, NodeKind)], n: usize) -> Option<(NodeId, NodeKind)> {
    (!nodes.is_empty()).then(|| nodes[n % nodes.len()])
}

fn pick_page(nodes: &[(NodeId, NodeKind)], n: usize) -> Option<NodeId> {
    let pages: Vec<_> = nodes.iter().filter(|(_, k)| *k == NodeKind::Page).collect();
    (!pages.is_empty()).then(|| pages[n % pages.len()].0)
}

fn position(n: usize) -> Position {
    if n.is_multiple_of(5) {
        Position::End
    } else {
        Position::Index(n % 4)
    }
}

fn perform(ws: &mut Workspace, hashes: &[String], action: &Action) {
    let nodes = all_nodes(ws);
    // Operations that the model rejects (moving into a descendant, editing a
    // trashed node, ...) are skipped: the property is about accepted ones.
    match action {
        Action::CreateFolder { parent, pos } => {
            let parent = pick(&nodes, *parent)
                .map(|(id, _)| id)
                .filter(|_| parent % 3 != 0);
            let _ = ws.create_folder(parent, position(*pos), "Thư mục");
        }
        Action::CreatePage { parent, pos, word } => {
            let parent = pick(&nodes, *parent)
                .map(|(id, _)| id)
                .filter(|_| parent % 3 != 0);
            let _ = ws.create_page(parent, position(*pos), WORDS[*word]);
        }
        Action::Rename { node, word } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.rename(id, WORDS[*word]);
            }
        }
        Action::Icon { node, on } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.set_icon(id, on.then_some("📝"));
            }
        }
        Action::Favorite { node, on } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.set_favorite(id, *on);
            }
        }
        Action::Move { node, parent, pos } => {
            if let (Some((id, _)), parent) = (pick(&nodes, *node), pick(&nodes, *parent)) {
                let _ = ws.move_node(
                    id,
                    parent.map(|p| p.0).filter(|_| parent_is_some(*pos)),
                    position(*pos),
                );
            }
        }
        Action::Reorder { node, pos } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.reorder(id, position(*pos));
            }
        }
        Action::Trash { node } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.trash_node(id);
            }
        }
        Action::Restore { node } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.restore(id);
            }
        }
        Action::DeletePermanently { node } => {
            if let Some((id, _)) = pick(&nodes, *node) {
                let _ = ws.delete_permanently(id);
            }
        }
        Action::EmptyTrash => {
            let _ = ws.empty_trash();
        }
        Action::Paragraph { node, word } => edit(ws, &nodes, *node, |d| {
            d.append_block(
                &[],
                &Block::paragraph(&format!("{} {}", WORDS[*word], WORDS[(*word + 3) % 10])),
            )
            .map(|_| ())
        }),
        Action::PageLink { node, target } => {
            if let Some(target) = pick_page(&nodes, *target) {
                edit(ws, &nodes, *node, |d| {
                    d.append_block(
                        &[],
                        &Block::new(BlockKind::PageLink {
                            page_id: target.to_string(),
                            title: String::new(),
                        }),
                    )
                    .map(|_| ())
                });
            }
        }
        Action::InlineLink { node, target } => {
            if let Some(target) = pick_page(&nodes, *target) {
                let block = Block::new(BlockKind::Paragraph).with_inline(vec![
                    Inline::text("xem "),
                    Inline::Text(Run {
                        text: "trang".into(),
                        marks: Marks {
                            link: Some(page_href(target)),
                            ..Marks::default()
                        },
                    }),
                ]);
                edit(ws, &nodes, *node, |d| {
                    d.append_block(&[], &block).map(|_| ())
                });
            }
        }
        Action::Image { node, which } => {
            let block = Block::new(BlockKind::Image {
                src: attachment_src(&hashes[*which]),
                alt: String::new(),
            });
            edit(ws, &nodes, *node, |d| {
                d.append_block(&[], &block).map(|_| ())
            });
        }
        Action::RemoveBlock { node } => edit(ws, &nodes, *node, |d| {
            let len = d.blocks()?.len();
            if len > 1 {
                d.delete_block(&[len - 1])
            } else {
                Ok(())
            }
        }),
        Action::Replace { node, word } => edit(ws, &nodes, *node, |d| {
            let first_is_text = d.blocks()?.first().is_some_and(|b| b.kind.is_text_block());
            if first_is_text {
                d.set_inline(&[0], &[Inline::text(WORDS[*word])])
            } else {
                Ok(())
            }
        }),
        Action::Compact { node } => {
            if let Some(id) = pick_page(&nodes, *node) {
                let _ = ws.compact(&ne_store::DocRef::Page(id));
            }
        }
    }
}

fn parent_is_some(pos: usize) -> bool {
    !pos.is_multiple_of(4)
}

fn edit(
    ws: &mut Workspace,
    nodes: &[(NodeId, NodeKind)],
    node: usize,
    f: impl FnOnce(&ne_core::page::PageDoc) -> ne_core::Result<()>,
) {
    if let Some(id) = pick_page(nodes, node) {
        let _ = ws.edit_page(id, f);
    }
}

fn section<'a>(dump: &'a str, name: &str) -> &'a str {
    let start = dump.find(&format!("== {name}\n")).expect("section exists");
    let rest = &dump[start..];
    match rest[3..].find("== ") {
        Some(end) => &rest[..end + 3],
        None => rest,
    }
}

fn config(compact_every: usize) -> StoreConfig {
    let mut config = store_support::config();
    config.compact_after_updates = compact_every;
    config.clock = Arc::new(TestClock::new(5_000_000));
    config
}

fn run(steps: &[Step]) -> String {
    let dir_a = tempfile::tempdir().expect("tempdir");
    let dir_b = tempfile::tempdir().expect("tempdir");
    let mut a = Workspace::create(dir_a.path(), config(6)).expect("create a");
    let mut b = Workspace::create(dir_b.path(), config(6)).expect("create b");
    let hashes: Vec<String> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|name| {
            a.add_attachment(name, name.as_bytes())
                .expect("attachment")
                .hash
        })
        .collect();
    deliver(&mut a, &mut b);

    for step in steps {
        match step {
            Step::Local(action) => perform(&mut a, &hashes, action),
            Step::OtherDevice(action) => {
                perform(&mut b, &hashes, action);
                deliver(&mut b, &mut a);
                deliver(&mut a, &mut b);
            }
            Step::Restart => {
                drop(a);
                a = Workspace::open(dir_a.path(), config(6)).expect("reopen a");
            }
            Step::Sync => {
                deliver(&mut a, &mut b);
                deliver(&mut b, &mut a);
            }
        }
    }

    // The incremental indexes equal a recomputation, for the data and for search.
    let queries = [
        "ghi chu", "duong", "ha noi", "ke hoach", "tuan", "mi", "qua", "nghien", "lat", "plan",
        "trang", "thu muc",
    ];
    let search_all = |ws: &Workspace| -> Vec<Vec<NodeId>> {
        queries
            .iter()
            .map(|q| {
                let mut ids: Vec<_> = ws
                    .search(q, 1000)
                    .expect("search")
                    .into_iter()
                    .map(|h| h.id)
                    .collect();
                ids.sort();
                ids
            })
            .collect()
    };
    let before = a.dump_derived_indexes().expect("dump");
    let found_before = search_all(&a);
    let sidebar_before = a.sidebar().expect("sidebar");
    a.rebuild_indexes().expect("rebuild");
    assert_eq!(
        before,
        a.dump_derived_indexes().expect("dump"),
        "indexes drifted from a rebuild"
    );
    assert_eq!(found_before, search_all(&a), "search results drifted");
    let mut sidebar_after = a.sidebar().expect("sidebar");
    let mut sidebar_before = sidebar_before;
    sidebar_before.sort_by_key(|r| r.id);
    sidebar_after.sort_by_key(|r| r.id);
    assert_eq!(sidebar_before, sidebar_after);

    // The sidebar rows describe exactly the visible tree.
    let tree = a.tree().snapshot().expect("snapshot");
    assert_eq!(sidebar_after.len(), tree.len());

    // Both devices end with the same derived data once synced.
    deliver(&mut a, &mut b);
    deliver(&mut b, &mut a);
    deliver(&mut a, &mut b);
    let (da, db) = (
        a.dump_derived_indexes().expect("a"),
        b.dump_derived_indexes().expect("b"),
    );
    for name in ["nodes", "page_text", "links", "indexed"] {
        assert_eq!(
            section(&da, name),
            section(&db, name),
            "devices disagree on {name}"
        );
    }
    a.check_integrity().expect("sound");
    da
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 200, ..ProptestConfig::default() })]

    #[test]
    fn derived_indexes_equal_a_full_rebuild(steps in proptest::collection::vec(step(), 1..45)) {
        run(&steps);
    }
}

#[test]
fn a_long_fixed_sequence_keeps_the_indexes_exact() {
    use Action::*;
    use Step::*;
    let steps = vec![
        Local(CreateFolder { parent: 0, pos: 0 }),
        Local(CreatePage {
            parent: 1,
            pos: 1,
            word: 0,
        }),
        Local(CreatePage {
            parent: 1,
            pos: 2,
            word: 1,
        }),
        Local(CreatePage {
            parent: 0,
            pos: 3,
            word: 2,
        }),
        Local(Paragraph { node: 0, word: 3 }),
        Local(PageLink { node: 1, target: 0 }),
        Local(InlineLink { node: 2, target: 1 }),
        Local(Image { node: 0, which: 1 }),
        Sync,
        OtherDevice(Rename { node: 1, word: 4 }),
        OtherDevice(Trash { node: 2 }),
        Local(Move {
            node: 3,
            parent: 1,
            pos: 1,
        }),
        Restart,
        Local(Restore { node: 2 }),
        Local(Trash { node: 0 }),
        Local(DeletePermanently { node: 0 }),
        Local(EmptyTrash),
        Local(CreatePage {
            parent: 0,
            pos: 0,
            word: 7,
        }),
        Local(CreatePage {
            parent: 3,
            pos: 0,
            word: 8,
        }),
        Local(CreateFolder { parent: 0, pos: 1 }),
        Local(Paragraph { node: 0, word: 9 }),
        Local(Paragraph { node: 1, word: 2 }),
        Local(PageLink { node: 0, target: 1 }),
        Local(InlineLink { node: 1, target: 0 }),
        Local(Image { node: 1, which: 2 }),
        OtherDevice(CreatePage {
            parent: 0,
            pos: 1,
            word: 5,
        }),
        Local(Replace { node: 0, word: 6 }),
        Restart,
        Sync,
    ];
    let dump = run(&steps);
    // The sequence really built something to index.
    let rows = |name: &str| section(&dump, name).lines().count() - 1;
    assert!(rows("nodes") >= 3, "{dump}");
    assert!(rows("links") >= 1, "{dump}");
    assert!(rows("attachment_refs") >= 1, "{dump}");
    assert!(rows("page_text") >= 2, "{dump}");
}

/// A tiny deterministic generator, so a fixed set of long random sequences
/// can also assert that the runs were not trivial.
fn steps_from_seed(seed: u64, len: usize) -> Vec<Step> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % bound as u64).expect("small")
    };
    (0..len)
        .map(|_| {
            let (n1, n2, n3, w) = (next(40), next(40), next(40), next(10));
            let on = next(2) == 0;
            let action = match next(20) {
                0 => Action::CreateFolder {
                    parent: n1,
                    pos: n2,
                },
                1..=4 => Action::CreatePage {
                    parent: n1,
                    pos: n2,
                    word: w,
                },
                5 => Action::Rename { node: n1, word: w },
                6 => Action::Icon { node: n1, on },
                7 => Action::Favorite { node: n1, on },
                8 | 9 => Action::Move {
                    node: n1,
                    parent: n2,
                    pos: n3,
                },
                10 => Action::Reorder { node: n1, pos: n2 },
                11 => Action::Trash { node: n1 },
                12 => Action::Restore { node: n1 },
                13 if next(3) == 0 => Action::DeletePermanently { node: n1 },
                14..=15 => Action::Paragraph { node: n1, word: w },
                16 => Action::PageLink {
                    node: n1,
                    target: n2,
                },
                17 => Action::InlineLink {
                    node: n1,
                    target: n2,
                },
                18 => Action::Image {
                    node: n1,
                    which: w % 3,
                },
                _ => Action::Replace { node: n1, word: w },
            };
            match next(12) {
                0 => Step::Restart,
                1 | 2 => Step::Sync,
                3 | 4 => Step::OtherDevice(action),
                _ => Step::Local(action),
            }
        })
        .collect()
}

#[test]
fn seeded_random_sequences_exercise_every_index_and_stay_exact() {
    let mut totals = std::collections::BTreeMap::<&str, usize>::new();
    for seed in 1..=24u64 {
        let dump = run(&steps_from_seed(seed, 60));
        for name in ["nodes", "page_text", "links", "attachment_refs", "indexed"] {
            *totals.entry(name).or_default() += section(&dump, name).lines().count() - 1;
        }
    }
    for (name, count) in &totals {
        assert!(
            *count >= 20,
            "the random runs barely touched {name}: {totals:?}"
        );
    }
}
