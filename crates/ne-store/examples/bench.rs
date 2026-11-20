//! Benchmark of the local store on a synthetic workspace.
//!
//! ```text
//! cargo run --release -p ne-store --example bench -- [--pages 10000]
//! ```
//!
//! It builds a workspace of synthetic Vietnamese and English pages in a
//! temporary directory, then reports the figures the product targets:
//! opening a 1,000-block page (target under 30 ms), search latency over varied
//! queries (target under 50 ms at p95 on 10,000 pages), the sidebar snapshot,
//! reopening the workspace, the latency of a durable local update, and the size
//! of the database. All text is generated; nothing is read from the machine.

use std::path::Path;
use std::time::{Duration, Instant};

use ne_core::NodeId;
use ne_core::page::{Block, BlockKind, PageDoc};
use ne_core::tree::Position;
use ne_store::{Durability, StoreConfig, Workspace, fold};

const VIETNAMESE: &[&str] = &[
    "xin",
    "chào",
    "tôi",
    "là",
    "người",
    "Việt",
    "Nam",
    "học",
    "ghi",
    "chú",
    "kế",
    "hoạch",
    "tuần",
    "này",
    "cuộc",
    "họp",
    "dự",
    "án",
    "báo",
    "cáo",
    "số",
    "liệu",
    "quý",
    "thành",
    "phố",
    "đường",
    "phố",
    "nhà",
    "hàng",
    "cà",
    "phê",
    "sáng",
    "chiều",
    "tối",
    "ăn",
    "uống",
    "bánh",
    "mì",
    "phở",
    "bún",
    "chả",
    "nghiên",
    "cứu",
    "tài",
    "liệu",
    "tham",
    "khảo",
    "ngân",
    "sách",
    "chuẩn",
    "bị",
    "hoàn",
    "thành",
    "bản",
    "đầu",
    "tiên",
    "mục",
    "tiêu",
    "công",
    "việc",
    "thương",
    "mại",
    "điện",
    "tử",
    "khách",
    "hàng",
    "đặt",
    "chỗ",
    "chuyến",
    "đi",
    "Đà",
    "Lạt",
    "Hà",
    "Nội",
    "Huế",
    "Sài",
    "Gòn",
    "nước",
    "mắm",
    "ngày",
    "mai",
    "hôm",
    "qua",
    "nhớ",
    "mua",
    "sắm",
    "danh",
    "sách",
    "việc",
    "cần",
    "làm",
    "ý",
    "tưởng",
    "lớn",
    "nhỏ",
    "quan",
    "trọng",
    "khẩn",
    "cấp",
    "đã",
    "xong",
    "chưa",
    "bắt",
    "đầu",
    "tiếp",
    "theo",
    "kết",
    "quả",
    "thử",
    "nghiệm",
    "phân",
    "tích",
    "dữ",
    "liệu",
    "máy",
    "tính",
    "phần",
    "mềm",
    "ứng",
    "dụng",
];

const ENGLISH: &[&str] = &[
    "plan",
    "meeting",
    "review",
    "design",
    "notes",
    "project",
    "budget",
    "release",
    "roadmap",
    "feature",
    "priority",
    "deadline",
    "summary",
    "draft",
    "todo",
    "idea",
    "report",
    "sync",
    "offline",
    "search",
    "editor",
    "document",
    "workspace",
    "backup",
    "deploy",
    "testing",
    "network",
    "storage",
    "latency",
    "cache",
    "index",
    "query",
    "result",
    "update",
    "version",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn word(&mut self) -> &'static str {
        if self.below(5) == 0 {
            ENGLISH[self.below(ENGLISH.len())]
        } else {
            VIETNAMESE[self.below(VIETNAMESE.len())]
        }
    }

    fn sentence(&mut self, min: usize, max: usize) -> String {
        let n = min + self.below(max - min + 1);
        let mut words: Vec<&str> = (0..n).map(|_| self.word()).collect();
        words.push(".");
        let mut text = words.join(" ").replace(" .", ".");
        if let Some(first) = text.chars().next() {
            text = first.to_uppercase().collect::<String>() + &text[first.len_utf8()..];
        }
        text
    }
}

fn list_item(text: &str) -> Block {
    Block::new(BlockKind::ListItem).with_children(vec![Block::paragraph(text)])
}

fn random_block(rng: &mut Rng) -> Block {
    match rng.below(20) {
        0 => Block::heading(
            1 + u8::try_from(rng.below(3)).unwrap_or(0),
            &rng.sentence(2, 6),
        ),
        1 | 2 => Block::new(BlockKind::BulletList).with_children(vec![
            list_item(&rng.sentence(4, 12)),
            list_item(&rng.sentence(4, 12)),
        ]),
        3 => Block::new(BlockKind::TodoList).with_children(vec![
            Block::new(BlockKind::TodoItem {
                checked: rng.below(2) == 0,
            })
            .with_children(vec![Block::paragraph(&rng.sentence(3, 9))]),
        ]),
        4 => {
            Block::new(BlockKind::Quote).with_children(vec![Block::paragraph(&rng.sentence(8, 20))])
        }
        _ => Block::paragraph(&rng.sentence(12, 40)),
    }
}

fn page_blocks(rng: &mut Rng, count: usize, links: &[NodeId]) -> Vec<Block> {
    let mut blocks: Vec<Block> = (0..count).map(|_| random_block(rng)).collect();
    for _ in 0..2.min(links.len()) {
        let target = links[rng.below(links.len())];
        blocks.push(Block::new(BlockKind::PageLink {
            page_id: target.to_string(),
            title: String::new(),
        }));
    }
    blocks
}

fn write_page(ws: &mut Workspace, id: NodeId, blocks: &[Block]) {
    ws.edit_page(id, |doc| {
        doc.append_blocks(&[], blocks)?;
        doc.delete_block(&[0])
    })
    .expect("write page");
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[rank]
}

fn summarize(label: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    println!(
        "  {label:<34} n={:<4} min {:>7.2}  p50 {:>7.2}  p95 {:>7.2}  max {:>7.2} ms",
        samples.len(),
        samples[0],
        percentile(&samples, 0.5),
        percentile(&samples, 0.95),
        samples[samples.len() - 1]
    );
}

fn time<R>(f: impl FnOnce() -> R) -> (R, f64) {
    let start = Instant::now();
    let value = f();
    (value, ms(start.elapsed()))
}

fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.metadata().map_or(0, |m| m.len()))
                .sum()
        })
        .unwrap_or(0)
}

fn main() {
    let pages: usize = std::env::args()
        .skip_while(|a| a != "--pages")
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(10_000);
    let dir = tempfile::tempdir().expect("temporary directory");
    let mut rng = Rng(0x00c0_ffee_1234_5678);

    // ---- build the workspace ----
    let bulk = StoreConfig {
        durability: Durability::Normal,
        ..StoreConfig::default()
    };
    let mut ws = Workspace::create(dir.path(), bulk.clone()).expect("create workspace");
    let (top, sub) = (10usize, 10usize);
    let per_leaf = pages.div_ceil(top * sub);
    let mut created: Vec<NodeId> = Vec::new();
    let mut body_chars = 0usize;
    let (_, build_ms) = time(|| {
        'build: for t in 0..top {
            let folder = ws
                .create_folder(None, Position::End, &format!("Khu {t} {}", rng.word()))
                .expect("folder");
            for s in 0..sub {
                let leaf = ws
                    .create_folder(
                        Some(folder),
                        Position::End,
                        &format!("Nhóm {s} {}", rng.word()),
                    )
                    .expect("folder");
                for _ in 0..per_leaf {
                    if created.len() >= pages {
                        break 'build;
                    }
                    let title = format!(
                        "{} {} {}",
                        rng.sentence(1, 1).trim_end_matches('.'),
                        rng.word(),
                        rng.word()
                    );
                    let id = ws
                        .create_page(Some(leaf), Position::End, &title)
                        .expect("page");
                    let count = 4 + rng.below(9);
                    let blocks = page_blocks(&mut rng, count, &created);
                    body_chars += blocks.iter().map(|b| b.plain_text().len()).sum::<usize>();
                    write_page(&mut ws, id, &blocks);
                    created.push(id);
                }
            }
        }
    });
    println!(
        "workspace: {} pages, {} folders, {:.1} MiB of body text, built in {:.1} s ({:.2} ms per page, durability Normal)",
        created.len(),
        top + top * sub,
        body_chars as f64 / 1_048_576.0,
        build_ms / 1000.0,
        build_ms / created.len() as f64
    );

    // ---- a 1,000-block page ----
    let big_parent = ws
        .create_folder(None, Position::End, "Trang lớn")
        .expect("folder");
    let big = ws
        .create_page(Some(big_parent), Position::End, "Trang 1.000 block")
        .expect("page");
    let big_blocks = page_blocks(&mut rng, 998, &created);
    write_page(&mut ws, big, &big_blocks);
    ws.compact(&ne_store::DocRef::Page(big)).expect("compact");
    let block_count = ws
        .open_page(big)
        .expect("open")
        .blocks()
        .expect("blocks")
        .len();
    let snapshot_len = ws.page_snapshot(big).expect("snapshot").len();
    println!(
        "\nopening a page of {block_count} blocks ({:.0} KiB snapshot)",
        snapshot_len as f64 / 1024.0
    );
    let mut cold = Vec::new();
    let mut read = Vec::new();
    let mut export = Vec::new();
    let mut total = Vec::new();
    for _ in 0..40 {
        ws.evict_pages();
        let started = Instant::now();
        let (_, open) = time(|| ws.open_page(big).map(|_| ()).expect("open"));
        let (blocks, read_ms) =
            time(|| ws.open_page(big).expect("cached").blocks().expect("blocks"));
        let (_, export_ms) = time(|| ws.page_snapshot(big).expect("snapshot"));
        assert_eq!(blocks.len(), block_count);
        cold.push(open);
        read.push(read_ms);
        export.push(export_ms);
        total.push(ms(started.elapsed()));
    }
    summarize("open (load from SQLite + import)", cold);
    summarize("read model (blocks)", read);
    summarize("export snapshot for the editor", export);
    summarize("open + read + export", total);

    // The same page with 99 un-compacted updates, the worst case before the
    // update log folds into a snapshot.
    let paragraphs: Vec<usize> = big_blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.kind == BlockKind::Paragraph)
        .map(|(i, _)| i)
        .collect();
    for i in 0..99 {
        let index = paragraphs[(i * 7) % paragraphs.len()];
        ws.edit_page(big, |d| d.insert_text(&[index], 0, "gõ "))
            .expect("edit");
    }
    let (log, _) = ws
        .update_log_size(&ne_store::DocRef::Page(big))
        .map(|s| (s.0, s.1))
        .expect("log");
    let mut with_log = Vec::new();
    for _ in 0..40 {
        ws.evict_pages();
        let (_, open) = time(|| ws.open_page(big).map(|_| ()).expect("open"));
        with_log.push(open);
    }
    summarize(&format!("open with {log} pending updates"), with_log);

    // ---- search ----
    let vocabulary: Vec<&str> = VIETNAMESE.iter().chain(ENGLISH).copied().collect();
    let mut queries: Vec<(&str, String)> = Vec::new();
    for _ in 0..40 {
        let w = vocabulary[rng.below(vocabulary.len())];
        queries.push(("one word", w.to_owned()));
        queries.push(("one word, no accents", fold(w)));
        let prefix: String = fold(w).chars().take(4).collect();
        queries.push(("prefix of 4", prefix));
        let middle: String = fold(w).chars().skip(1).take(4).collect();
        queries.push(("inside a word", middle));
        queries.push((
            "two words",
            format!(
                "{} {}",
                fold(w),
                fold(vocabulary[rng.below(vocabulary.len())])
            ),
        ));
        queries.push((
            "three words",
            format!(
                "{} {} {}",
                fold(w),
                fold(vocabulary[rng.below(vocabulary.len())]),
                fold(vocabulary[rng.below(vocabulary.len())])
            ),
        ));
        queries.push(("short + long", format!("{} {}", fold(w), "an")));
    }
    for _ in 0..20 {
        queries.push((
            "short term (titles)",
            fold(vocabulary[rng.below(vocabulary.len())])
                .chars()
                .take(2)
                .collect(),
        ));
    }
    queries.push(("rare term", "khong-ton-tai-xyz".to_owned()));
    for (_, q) in queries.iter().take(30) {
        let _ = ws.search(q, 20);
    }
    println!(
        "\nsearch over {} pages, limit 20, {} queries",
        created.len(),
        queries.len()
    );
    let mut all = Vec::new();
    let mut by_kind: std::collections::BTreeMap<&str, Vec<f64>> = Default::default();
    let mut hits_total = 0usize;
    for (kind, q) in &queries {
        let (hits, took) = time(|| ws.search(q, 20).expect("search"));
        hits_total += hits.len();
        all.push(took);
        by_kind.entry(kind).or_default().push(took);
    }
    summarize("all queries", all);
    for (kind, samples) in by_kind {
        summarize(kind, samples);
    }
    println!(
        "  average results per query: {:.1}",
        hits_total as f64 / queries.len() as f64
    );

    // ---- sidebar ----
    println!("\nsidebar snapshot");
    let mut sidebar = Vec::new();
    let mut tree_walk = Vec::new();
    let mut rows = 0;
    for _ in 0..20 {
        let (list, took) = time(|| ws.sidebar().expect("sidebar"));
        rows = list.len();
        sidebar.push(took);
        let (_, took) = time(|| ws.tree().snapshot().expect("tree snapshot"));
        tree_walk.push(took);
    }
    summarize(&format!("SQLite index, {rows} rows"), sidebar);
    summarize("CRDT tree walk (reference)", tree_walk);

    // ---- durable local updates ----
    drop(ws);
    let durable = StoreConfig::default();
    let (mut ws, reopen_ms) = {
        let (ws, took) = time(|| Workspace::open(dir.path(), durable.clone()).expect("reopen"));
        (ws, took)
    };
    println!(
        "\nreopening the workspace: {reopen_ms:.1} ms ({} nodes)",
        ws.tree().snapshot().expect("snapshot").len()
    );
    let page = created[created.len() / 2];
    let editor =
        PageDoc::from_snapshot(&ws.page_snapshot(page).expect("snapshot")).expect("editor doc");
    editor.set_peer_id(1234).expect("peer");
    let mut local = Vec::new();
    for i in 0..200 {
        let before = editor.version();
        editor
            .insert_text(&[0], 0, if i % 2 == 0 { "ă" } else { "x" })
            .expect("type");
        let update = editor.export_updates_since(&before).expect("update");
        let (_, took) = time(|| ws.apply_local_update(page, &update).expect("apply"));
        local.push(took);
    }
    println!("applying an editor update, durability Full (fsync per commit)");
    summarize("apply_local_update", local);

    // ---- size ----
    drop(ws);
    let db = dir.path().join(ne_store::DB_FILE);
    let conn = rusqlite::Connection::open(&db).expect("open database");
    // Fold the write-ahead log into the main file so its size is the whole story.
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .expect("checkpoint");
    let size = std::fs::metadata(&db).map_or(0, |m| m.len());
    let sum = |sql: &str| -> f64 {
        conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
            .expect("size query")
            .unwrap_or(0) as f64
            / 1_048_576.0
    };
    println!(
        "\ndatabase: {:.1} MiB for {:.1} MiB of body text ({} KiB per page)",
        size as f64 / 1_048_576.0,
        body_chars as f64 / 1_048_576.0,
        size / 1024 / created.len().max(1) as u64
    );
    println!(
        "  documents {:.1} MiB (snapshots {:.1}, update log {:.1}), page text {:.1} MiB, rest is indexes (search, nodes, links)",
        sum("SELECT SUM(length(snapshot)) + SUM(length(version)) FROM docs")
            + sum("SELECT SUM(length(bytes)) FROM updates"),
        sum("SELECT SUM(length(snapshot)) FROM docs"),
        sum("SELECT SUM(length(bytes)) FROM updates"),
        sum("SELECT SUM(length(body)) FROM page_text"),
    );
    let _ = dir_size(dir.path());
    println!("\ntargets: open a 1,000-block page < 30 ms, search < 50 ms");
}
