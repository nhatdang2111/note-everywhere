//! Structured import of a Markdown + CSV export folder. The fixture is built
//! here, in the layout the export is documented to have, from synthetic
//! content; nothing in it comes from a real export.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use ne_core::NodeId;
use ne_core::database::{CellValue, DatabaseDoc, PropertyType};
use ne_core::markdown::ImportSink;
use ne_core::markdown::structured::{StructuredReport, StructuredSink, import_structured};
use ne_core::page::{Block, BlockKind, PageDoc};
use ne_core::tree::{NodeKind, SystemClock, WorkspaceTree};

fn id(n: u8) -> String {
    format!("{n:032x}")
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    fs::write(path, content).expect("write");
}

fn build_fixture(root: &Path) {
    let (projects, roadmap, tasks, spec, ship, notes, people, ann, archive_old, extra) = (
        id(1),
        id(2),
        id(3),
        id(4),
        id(5),
        id(6),
        id(7),
        id(8),
        id(9),
        id(10),
    );
    write(
        root,
        &format!("Projects {projects}.md"),
        &format!(
            "# Projects\n\nAn intro with a [Roadmap](Projects%20{projects}/Roadmap%20{roadmap}.md) \
             link, a [database](Tasks%20{tasks}.csv) link, a [site](https://example.test/a%20b) \
             and a [missing page](Gone%20{}.md).\n\n\
             [Roadmap](Projects%20{projects}/Roadmap%20{roadmap}.md)\n\n\
             A file: [report](Projects%20{projects}/report.pdf)\n",
            id(99)
        ),
    );
    write(
        root,
        &format!("Projects {projects}/Roadmap {roadmap}.md"),
        &format!(
            "# Roadmap\n\nBack to [Projects](../Projects%20{projects}.md).\n\n\
             ![chart](Roadmap%20{roadmap}/chart%201.png)\n\n![lost](Roadmap%20{roadmap}/lost.png)\n"
        ),
    );
    write(
        root,
        &format!("Projects {projects}/Roadmap {roadmap}/chart 1.png"),
        "PNG-BYTES",
    );
    // `Tasks.csv` and `Tasks_all.csv` hold the same database; the _all one wins.
    write(
        root,
        &format!("Tasks {tasks}.csv"),
        "Name,Status\nWrite spec,Todo\n",
    );
    write(
        root,
        &format!("Tasks {tasks}_all.csv"),
        &format!(
            "\u{feff}Name,Status,Estimate,Due,Done,Tags,Link,Owner,Notes\n\
             Write spec,In progress,3,\"November 21, 2026\",Yes,\"design, docs\",https://example.test/spec,\
             Ann (People/Ann%20{ann}.md),First line\n\
             Ship,Done,5,\"November 25, 2026 9:30 AM → November 26, 2026\",No,docs,https://example.test/ship,,Second\n\
             Review,In progress,2,,No,design,https://example.test/r,,Third\n\
             Plan,Done,1,\"December 1, 2026\",Yes,docs,https://example.test/p,,Fourth\n"
        ),
    );
    write(
        root,
        &format!("Tasks/Write spec {spec}.md"),
        "# Write spec\n\nStatus: In progress\nEstimate: 3\nDue: November 21, 2026\n\nThe actual content.\n\n\
         ![shot](Write%20spec%20{spec}/shot.png)\n"
            .replace("{spec}", &spec)
            .as_str(),
    );
    write(
        root,
        &format!("Tasks/Write spec {spec}/shot.png"),
        "SHOT-BYTES",
    );
    write(
        root,
        &format!("Tasks/Write spec {spec}/Notes {notes}.md"),
        "# Notes\n\nA subpage of a row.\n",
    );
    write(
        root,
        &format!("Tasks/Ship {ship}.md"),
        "# Ship\n\nStatus: Done\n\nShip it.\n",
    );
    write(
        root,
        &format!("Tasks/Orphan {extra}.md"),
        "# Orphan\n\nNo CSV row.\n",
    );
    write(
        root,
        &format!("People {people}.csv"),
        "Name,Team\nAnn,Core\n",
    );
    write(
        root,
        &format!("People/Ann {ann}.md"),
        "# Ann\n\nTeam: Core\n\nBio.\n",
    );
    write(
        root,
        &format!("Archive/Old {archive_old}.md"),
        "# Old\n\nArchived.\n",
    );
    write(root, "Loose.csv", "Name,Score\nOne,1\nTwo,2\n");
    write(root, "page.html", "<p>not imported</p>");
    write(root, ".DS_Store", "junk");
}

#[derive(Default)]
struct Sink {
    pages: HashMap<NodeId, PageDoc>,
    databases: HashMap<NodeId, DatabaseDoc>,
    attachments: HashMap<String, Vec<u8>>,
}

impl ImportSink for Sink {
    fn add_attachment(&mut self, _file_name: &str, bytes: Vec<u8>) -> ne_core::Result<String> {
        let hash = format!("{:064x}", bytes.len() * 7 + bytes[0] as usize);
        self.attachments.insert(hash.clone(), bytes);
        Ok(hash)
    }

    fn save_page(&mut self, id: NodeId, doc: PageDoc) -> ne_core::Result<()> {
        self.pages.insert(id, doc);
        Ok(())
    }
}

impl StructuredSink for Sink {
    fn save_database(&mut self, id: NodeId, doc: DatabaseDoc) -> ne_core::Result<()> {
        self.databases.insert(id, doc);
        Ok(())
    }
}

struct Imported {
    tree: WorkspaceTree,
    sink: Sink,
    report: StructuredReport,
}

fn import() -> Imported {
    let dir = tempfile::tempdir().expect("tempdir");
    build_fixture(dir.path());
    let tree = WorkspaceTree::new(Arc::new(SystemClock));
    let mut sink = Sink::default();
    let report = import_structured(&tree, None, dir.path(), &mut sink).expect("import");
    Imported { tree, sink, report }
}

fn child(tree: &WorkspaceTree, parent: Option<NodeId>, title: &str) -> ne_core::tree::NodeInfo {
    tree.children(parent, false)
        .expect("children")
        .into_iter()
        .find(|n| n.title == title)
        .unwrap_or_else(|| panic!("no node {title:?}"))
}

fn titles(tree: &WorkspaceTree, parent: Option<NodeId>) -> Vec<(String, NodeKind)> {
    let mut out: Vec<_> = tree
        .children(parent, false)
        .expect("children")
        .into_iter()
        .map(|n| (n.title, n.kind))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn hierarchy_titles_and_counts_match_the_export() {
    let Imported { tree, report, .. } = import();
    assert_eq!(
        titles(&tree, None),
        [
            ("Archive".to_owned(), NodeKind::Folder),
            ("Loose".to_owned(), NodeKind::Database),
            ("People".to_owned(), NodeKind::Database),
            ("Projects".to_owned(), NodeKind::Page),
            ("Tasks".to_owned(), NodeKind::Database),
        ]
    );
    let projects = child(&tree, None, "Projects");
    assert_eq!(
        titles(&tree, Some(projects.id)),
        [("Roadmap".to_owned(), NodeKind::Page)]
    );
    let archive = child(&tree, None, "Archive");
    assert_eq!(
        titles(&tree, Some(archive.id)),
        [("Old".to_owned(), NodeKind::Page)]
    );
    let tasks = child(&tree, None, "Tasks");
    let rows: Vec<String> = titles(&tree, Some(tasks.id))
        .into_iter()
        .map(|t| t.0)
        .collect();
    assert_eq!(
        rows,
        ["Orphan", "Plan", "Review", "Ship", "Write spec"],
        "CSV rows plus the page without one"
    );
    let spec = child(&tree, Some(tasks.id), "Write spec");
    assert_eq!(
        titles(&tree, Some(spec.id)),
        [("Notes".to_owned(), NodeKind::Page)],
        "subpages of rows"
    );
    assert_eq!(report.databases, 3);
    assert_eq!(report.rows, 5 + 1 + 2);
    assert_eq!(report.folders, 1);
    assert_eq!(report.pages, 4, "Projects, Roadmap, Old, Notes");
    assert!(
        report.skipped.iter().any(|s| s.contains("_all")),
        "the duplicate CSV is reported: {:?}",
        report.skipped
    );
}

#[test]
fn links_between_exported_files_become_page_links() {
    let Imported {
        tree, sink, report, ..
    } = import();
    let projects = child(&tree, None, "Projects");
    let roadmap = child(&tree, Some(projects.id), "Roadmap");
    let tasks = child(&tree, None, "Tasks");
    let blocks = sink.pages[&projects.id].blocks().expect("blocks");
    let kinds: Vec<_> = blocks.iter().map(|b| b.kind.clone()).collect();
    assert!(
        kinds.iter().any(|k| matches!(k, BlockKind::ChildPage { page_id, .. } if *page_id == roadmap.id.to_string())),
        "a subpage on its own line becomes a child page block: {kinds:?}"
    );
    let first = &blocks[0];
    let links: Vec<_> = first
        .inline
        .iter()
        .filter_map(|i| match i {
            ne_core::page::Inline::Text(run) => run.page_link(),
            ne_core::page::Inline::Break => None,
        })
        .collect();
    assert_eq!(
        links,
        [roadmap.id, tasks.id],
        "a link to the CSV points at the database"
    );
    let text = sink.pages[&projects.id].plain_text().expect("text");
    assert!(
        text.contains("site") && text.contains("missing page"),
        "unresolved link text stays"
    );
    // The back link from the subpage.
    let back = sink.pages[&roadmap.id].blocks().expect("blocks");
    let back_links: Vec<_> = back
        .iter()
        .flat_map(|b| b.inline.iter())
        .filter_map(|i| match i {
            ne_core::page::Inline::Text(run) => run.page_link(),
            ne_core::page::Inline::Break => None,
        })
        .collect();
    assert_eq!(back_links, [projects.id]);
    assert!(report.links_rewritten >= 4);
    assert_eq!(
        report.links_unresolved, 1,
        "only the link to a missing .md file"
    );
}

#[test]
fn images_are_imported_as_attachments_and_missing_ones_reported() {
    let Imported {
        tree, sink, report, ..
    } = import();
    let projects = child(&tree, None, "Projects");
    let roadmap = child(&tree, Some(projects.id), "Roadmap");
    let blocks = sink.pages[&roadmap.id].blocks().expect("blocks");
    let sources: Vec<String> = blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Image { src, .. } => Some(src.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sources.len(), 2);
    let hash = sources[0]
        .strip_prefix("attachment:")
        .expect("an attachment source");
    assert_eq!(sink.attachments[hash], b"PNG-BYTES");
    assert!(
        sources[1].contains("lost.png"),
        "an image that is not there keeps its source"
    );
    assert_eq!(report.images_missing, 1);
    assert_eq!(report.attachments, 2, "the chart and the row's screenshot");
}

fn property(doc: &DatabaseDoc, name: &str) -> ne_core::database::Property {
    doc.properties()
        .expect("properties")
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no property {name:?}"))
}

#[test]
fn a_csv_becomes_a_database_with_inferred_property_types() {
    let Imported { tree, sink, .. } = import();
    let tasks = child(&tree, None, "Tasks");
    let doc = &sink.databases[&tasks.id];
    let types: Vec<(String, PropertyType)> = doc
        .properties()
        .expect("properties")
        .into_iter()
        .map(|p| (p.name, p.ty))
        .collect();
    assert_eq!(
        types,
        [
            ("Status".to_owned(), PropertyType::Select),
            ("Estimate".to_owned(), PropertyType::Number),
            ("Due".to_owned(), PropertyType::Date),
            ("Done".to_owned(), PropertyType::Checkbox),
            ("Tags".to_owned(), PropertyType::MultiSelect),
            ("Link".to_owned(), PropertyType::Url),
            ("Owner".to_owned(), PropertyType::Relation),
            ("Notes".to_owned(), PropertyType::Text),
        ]
    );
    let status = property(doc, "Status");
    let names: Vec<_> = status.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["In progress", "Done"]);
    let spec = child(&tree, Some(tasks.id), "Write spec").id.to_string();
    let ship = child(&tree, Some(tasks.id), "Ship").id.to_string();
    let value = |row: &str, name: &str| doc.cell(row, &property(doc, name).id).expect("cell");
    assert_eq!(value(&spec, "Estimate"), Some(CellValue::Number(3.0)));
    assert_eq!(value(&spec, "Done"), Some(CellValue::Checkbox(true)));
    assert_eq!(value(&ship, "Done"), Some(CellValue::Checkbox(false)));
    assert_eq!(
        value(&spec, "Notes"),
        Some(CellValue::Text("First line".into()))
    );
    match value(&ship, "Due") {
        Some(CellValue::Date(d)) => {
            assert_eq!(d.start, "2026-11-25T09:30");
            assert_eq!(d.end.as_deref(), Some("2026-11-26"));
        }
        other => panic!("{other:?}"),
    }
    match value(&spec, "Tags") {
        Some(CellValue::MultiSelect(ids)) => assert_eq!(ids.len(), 2),
        other => panic!("{other:?}"),
    }
    match value(&spec, "Status") {
        Some(CellValue::Select(id)) => {
            assert_eq!(status.option(&id).expect("option").name, "In progress")
        }
        other => panic!("{other:?}"),
    }
    // The relation points at the row page of the People database.
    let people = child(&tree, None, "People");
    let ann = child(&tree, Some(people.id), "Ann").id.to_string();
    let owner = property(doc, "Owner");
    assert_eq!(
        owner.relation.as_ref().map(|r| r.database.clone()),
        Some(people.id.to_string())
    );
    assert_eq!(value(&spec, "Owner"), Some(CellValue::Relation(vec![ann])));
    assert_eq!(value(&ship, "Owner"), None);
}

#[test]
fn row_pages_lose_their_property_block_and_keep_their_content() {
    let Imported { tree, sink, .. } = import();
    let tasks = child(&tree, None, "Tasks");
    let spec = child(&tree, Some(tasks.id), "Write spec");
    let text = sink.pages[&spec.id].plain_text().expect("text");
    assert!(text.contains("The actual content."), "{text}");
    assert!(
        !text.contains("Estimate"),
        "the property block is gone: {text}"
    );
    let ship = child(&tree, Some(tasks.id), "Ship");
    assert_eq!(
        sink.pages[&ship.id].plain_text().expect("text").trim(),
        "Ship it."
    );
    // A CSV row without a page still gets an (empty) page.
    let review = child(&tree, Some(tasks.id), "Review");
    assert!(sink.pages.contains_key(&review.id));
    assert!(
        sink.pages[&review.id]
            .plain_text()
            .expect("text")
            .is_empty()
    );
}

#[test]
fn a_csv_without_a_row_directory_still_becomes_a_database() {
    let Imported { tree, sink, .. } = import();
    let loose = child(&tree, None, "Loose");
    let doc = &sink.databases[&loose.id];
    assert_eq!(property(doc, "Score").ty, PropertyType::Number);
    let one = child(&tree, Some(loose.id), "One").id.to_string();
    assert_eq!(
        doc.cell(&one, &property(doc, "Score").id).expect("cell"),
        Some(CellValue::Number(1.0))
    );
}

#[test]
fn unrelated_files_are_not_imported_and_nothing_hidden_is_read() {
    let Imported { tree, report, .. } = import();
    let all: Vec<String> = tree
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|n| n.title)
        .collect();
    assert!(
        !all.iter()
            .any(|t| t.contains("html") || t.contains("DS_Store"))
    );
    assert!(report.skipped.iter().all(|s| !s.contains("DS_Store")));
}

#[test]
fn a_bad_csv_is_skipped_and_the_rest_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "Broken.csv", "a,\"b\n1,2\n");
    write(dir.path(), "Fine.md", "# Fine\n\nok\n");
    let tree = WorkspaceTree::new(Arc::new(SystemClock));
    let mut sink = Sink::default();
    let report = import_structured(&tree, None, dir.path(), &mut sink).expect("import");
    assert_eq!(report.databases, 0);
    assert_eq!(report.pages, 1);
    assert!(report.skipped.iter().any(|s| s.contains("Broken.csv")));
    let _ = Block::paragraph("");
}
