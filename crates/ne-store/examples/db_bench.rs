//! Benchmark of databases: 10,000 rows with 8 properties.
//!
//! ```text
//! cargo run --release -p ne-store --example db_bench -- [--rows 10000]
//! ```
//!
//! The database is built through the structured import (a generated CSV), then
//! the example reports the latency of a filter + multi-key sort query, a
//! board (group by) query, a text search over cells, and of one cell edit
//! (document change, index update and a durable commit). The target for the
//! first is under 100 ms. All content is generated.

use std::fmt::Write as _;
use std::fs;
use std::time::{Duration, Instant};

use ne_core::NodeId;
use ne_core::database::{CellValue, FilterNode, GroupConfig, Logic, Operator, QuerySpec, SortKey};
use ne_store::{Durability, StoreConfig, Workspace};
use serde_json::json;

const WORDS: &[&str] = &[
    "kế",
    "hoạch",
    "tuần",
    "họp",
    "dự",
    "án",
    "báo",
    "cáo",
    "khách",
    "hàng",
    "đường",
    "phố",
    "Hà",
    "Nội",
    "Sài",
    "Gòn",
    "cà",
    "phê",
    "sách",
    "vở",
];

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[usize::try_from(self.next()).unwrap_or(0) % items.len()]
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn measure(label: &str, runs: usize, mut work: impl FnMut(usize)) {
    work(0); // warm up
    let mut times: Vec<Duration> = (0..runs)
        .map(|i| {
            let start = Instant::now();
            work(i);
            start.elapsed()
        })
        .collect();
    times.sort();
    println!(
        "{label:<46} p50 {:>8.2} ms   p95 {:>8.2} ms   max {:>8.2} ms   ({runs} runs)",
        percentile(&times, 0.5).as_secs_f64() * 1000.0,
        percentile(&times, 0.95).as_secs_f64() * 1000.0,
        times.last().map_or(0.0, |t| t.as_secs_f64() * 1000.0),
    );
}

fn main() {
    let rows: usize = std::env::args()
        .skip_while(|a| a != "--rows")
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(10_000);
    let mut rng = Lcg(42);
    let export = tempfile::tempdir().expect("tempdir");
    let mut csv = String::from("Name,Note,Amount,Status,Tags,Due,Done,Link,Score\n");
    let statuses = ["Cần làm", "Đang làm", "Chờ", "Xong", "Huỷ"];
    let tags = ["a", "b", "c", "d", "e", "f", "g", "h"];
    for i in 0..rows {
        let title = format!("{} {} {i}", rng.pick(WORDS), rng.pick(WORDS));
        let note = format!(
            "{} {} {}",
            rng.pick(WORDS),
            rng.pick(WORDS),
            rng.pick(WORDS)
        );
        let amount = rng.next() % 10_000;
        let status = rng.pick(&statuses);
        let tag = format!("{}, {}", rng.pick(&tags), rng.pick(&tags));
        let day = 1 + rng.next() % 28;
        let month = 1 + rng.next() % 12;
        let done = if rng.next().is_multiple_of(2) {
            "Yes"
        } else {
            "No"
        };
        let score = rng.next() % 100;
        let _ = writeln!(
            csv,
            "{title},{note},{amount},{status},\"{tag}\",\"2026-{month:02}-{day:02}\",{done},https://example.test/{i},{score}"
        );
    }
    fs::write(export.path().join("Bench.csv"), csv).expect("csv");

    let dir = tempfile::tempdir().expect("workspace dir");
    let config = StoreConfig {
        durability: Durability::Full,
        ..StoreConfig::default()
    };
    let mut ws = Workspace::create(dir.path(), config.clone()).expect("create");
    let start = Instant::now();
    let report = ws
        .import_structured_dir(None, export.path())
        .expect("import");
    println!(
        "built {} rows (title plus 8 properties) by import in {:.1} s",
        report.rows,
        start.elapsed().as_secs_f64()
    );
    let db: NodeId = ws.children(None).expect("top")[0].id;
    let schema = ws.database_schema(db).expect("schema");
    let find = |name: &str| {
        schema
            .properties
            .iter()
            .find(|p| p.name == name)
            .expect("property")
    };
    let (amount, status, tags_prop, due, done, note) = (
        find("Amount").id.clone(),
        find("Status").id.clone(),
        find("Tags").id.clone(),
        find("Due").id.clone(),
        find("Done").id.clone(),
        find("Note").id.clone(),
    );
    let status_option = find("Status").options[0].id.clone();
    let tag_option = find("Tags").options[0].id.clone();
    println!(
        "types: {:?}",
        schema
            .properties
            .iter()
            .map(|p| (p.name.as_str(), p.ty))
            .collect::<Vec<_>>()
    );

    // Filter + multi-key sort: three conditions, three sort keys, first page.
    let hard = QuerySpec {
        filter: Some(FilterNode::Group {
            logic: Logic::And,
            children: vec![
                FilterNode::Condition {
                    property: amount.clone(),
                    operator: Operator::Gt,
                    value: Some(json!(2500)),
                },
                FilterNode::Group {
                    logic: Logic::Or,
                    children: vec![
                        FilterNode::Condition {
                            property: status.clone(),
                            operator: Operator::Is,
                            value: Some(json!(status_option)),
                        },
                        FilterNode::Condition {
                            property: tags_prop.clone(),
                            operator: Operator::Contains,
                            value: Some(json!(tag_option)),
                        },
                    ],
                },
                FilterNode::Condition {
                    property: note.clone(),
                    operator: Operator::NotContains,
                    value: Some(json!("cà phê")),
                },
            ],
        }),
        sorts: vec![
            SortKey {
                property: status.clone(),
                descending: false,
            },
            SortKey {
                property: amount.clone(),
                descending: true,
            },
            SortKey {
                property: "title".into(),
                descending: false,
            },
        ],
        limit: 100,
        ..QuerySpec::default()
    };
    let matched = ws.database_query(db, &hard).expect("query").total;
    println!("filter matches {matched} of {rows} rows");
    measure(
        "filter (3 conditions) + sort (3 keys), 100 rows",
        100,
        |_| {
            ws.database_query(db, &hard).expect("query");
        },
    );
    let sort_only = QuerySpec {
        sorts: vec![SortKey {
            property: "title".into(),
            descending: false,
        }],
        limit: 100,
        ..QuerySpec::default()
    };
    measure("sort by title (Vietnamese), 100 rows", 100, |_| {
        ws.database_query(db, &sort_only).expect("query");
    });
    measure(
        "third page of 100 (offset 200) of the hard query",
        100,
        |_| {
            let mut spec = hard.clone();
            spec.offset = 200;
            ws.database_query(db, &spec).expect("query");
        },
    );
    let board = QuerySpec {
        group: Some(GroupConfig {
            property: status.clone(),
            hide_empty: false,
        }),
        sorts: vec![SortKey {
            property: amount.clone(),
            descending: true,
        }],
        limit: 50,
        ..QuerySpec::default()
    };
    measure("board: group by select, 50 cards per column", 100, |_| {
        ws.database_query(db, &board).expect("query");
    });
    let calendar = QuerySpec {
        date_property: Some(due.clone()),
        date_range: Some(ne_core::database::DateRange {
            from: "2026-06-01".into(),
            to: "2026-06-30".into(),
        }),
        ..QuerySpec::default()
    };
    measure("calendar: rows of one month", 100, |_| {
        ws.database_query(db, &calendar).expect("query");
    });
    measure("search a text property value (full-text)", 100, |i| {
        ws.search(["kế hoạch", "cà phê", "đường phố", "Hà Nội"][i % 4], 20)
            .expect("search");
    });

    // One cell edit: document change + index update + durable commit.
    let ids: Vec<NodeId> = ws
        .database_query(
            db,
            &QuerySpec {
                limit: 1000,
                ..QuerySpec::default()
            },
        )
        .expect("rows")
        .rows
        .iter()
        .map(|r| r.id.parse().expect("id"))
        .collect();
    measure("cell edit: number (document + index + commit)", 300, |i| {
        let row = ids[(i * 7) % ids.len()];
        ws.database_set_cell(db, row, &amount, Some(CellValue::Number(i as f64)))
            .expect("edit");
    });
    measure(
        "two checkbox edits (one of them always changes)",
        300,
        |i| {
            let row = ids[(i * 11) % ids.len()];
            for ticked in [true, false] {
                ws.database_set_cell(db, row, &done, Some(CellValue::Checkbox(ticked)))
                    .expect("edit");
            }
        },
    );
    measure("cell edit: text (also refreshes search)", 300, |i| {
        let row = ids[(i * 13) % ids.len()];
        ws.database_set_cell(
            db,
            row,
            &note,
            Some(CellValue::Text(format!("ghi chú mới {i}"))),
        )
        .expect("edit");
    });
    measure("rename a row (tree + node index + title key)", 100, |i| {
        let row = ids[(i * 17) % ids.len()];
        ws.rename(row, &format!("Đổi tên {i}")).expect("rename");
    });
    measure("create a row at the end", 20, |i| {
        ws.database_create_row(
            db,
            &format!("Hàng mới {i}"),
            ne_core::tree::Position::End,
            &Default::default(),
        )
        .expect("row");
    });
    measure("move a board card (cell + reorder)", 30, |i| {
        let row = ids[(i * 19) % ids.len()];
        let before = ids[(i * 23 + 5) % ids.len()];
        ws.database_move_card(db, row, &status, Some(&status_option), Some(before))
            .expect("move");
    });
    ws.check_integrity().expect("sound");
    drop(ws);

    // A cold start: opening the workspace and running the first query.
    let start = Instant::now();
    let mut ws = Workspace::open(dir.path(), config).expect("reopen");
    let open = start.elapsed();
    let start = Instant::now();
    ws.database_query(db, &hard).expect("query");
    println!(
        "cold start: open {:.0} ms, first query (loads the document) {:.0} ms",
        open.as_secs_f64() * 1000.0,
        start.elapsed().as_secs_f64() * 1000.0
    );
    let size = fs::metadata(dir.path().join(ne_store::DB_FILE)).map_or(0, |m| m.len());
    println!("database file: {:.1} MiB", size as f64 / 1_048_576.0);
}
