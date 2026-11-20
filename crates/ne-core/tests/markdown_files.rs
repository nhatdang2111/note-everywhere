//! Windows-safe file names, folder export and folder import.

mod page_support;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ne_core::markdown::filenames::{MAX_STEM_CHARS, NameAllocator, sanitize_stem};
use ne_core::markdown::{Attachment, ImportSink, PageSource, export_tree, import_directory};
use ne_core::page::{Block, BlockKind, PageDoc, attachment_src, parse_attachment_src};
use ne_core::tree::{NodeInfo, NodeKind, Position, WorkspaceTree};
use ne_core::{Error, NodeId, Result};
use page_support::replica;

// ---------------------------------------------------------------------------
// File names
// ---------------------------------------------------------------------------

#[test]
fn forbidden_characters_and_controls_are_replaced() {
    assert_eq!(
        sanitize_stem("a<b>c:d\"e/f\\g|h?i*j"),
        "a_b_c_d_e_f_g_h_i_j"
    );
    assert_eq!(
        sanitize_stem("tab\there\nnewline\u{7f}"),
        "tab_here_newline_"
    );
    assert_eq!(
        sanitize_stem("Ghi chú: đường/Hà Nội?"),
        "Ghi chú_ đường_Hà Nội_"
    );
}

#[test]
fn trailing_dots_and_spaces_are_removed() {
    assert_eq!(sanitize_stem("name. . "), "name");
    assert_eq!(sanitize_stem("  padded  "), "padded");
    assert_eq!(sanitize_stem("end..."), "end");
    assert_eq!(sanitize_stem("dot.inside"), "dot.inside");
}

#[test]
fn empty_and_dot_only_names_fall_back() {
    for name in ["", "   ", "...", " . ", "\u{0}"] {
        let stem = sanitize_stem(name);
        assert!(
            !stem.is_empty() && stem != "." && stem != "..",
            "{name:?} -> {stem:?}"
        );
    }
    assert_eq!(sanitize_stem(""), "Untitled");
    assert_eq!(sanitize_stem(".hidden"), "_hidden");
    assert_eq!(sanitize_stem(".."), "Untitled");
}

#[test]
fn reserved_device_names_are_prefixed_with_or_without_extension() {
    for name in [
        "CON",
        "con",
        "Prn",
        "AUX",
        "nul",
        "COM1",
        "com9",
        "LPT1",
        "lpt9",
        "COM0",
        "COM¹",
        "LPT²",
        "con.txt",
        "NUL.tar.gz",
        "Aux.",
        "CON ",
    ] {
        let stem = sanitize_stem(name);
        assert!(stem.starts_with('_'), "{name:?} -> {stem:?}");
    }
    // Names that only look similar are fine.
    for name in ["CONSOLE", "COM10", "LPT", "COMM1", "a.CON", "my CON"] {
        assert_eq!(sanitize_stem(name), name);
    }
}

#[test]
fn long_names_are_capped_without_cutting_a_character() {
    let long = "đ".repeat(300);
    let stem = sanitize_stem(&long);
    assert_eq!(stem.chars().count(), MAX_STEM_CHARS);
    // Windows counts UTF-16 units: even all-astral text stays under 255.
    let astral = sanitize_stem(&"😀".repeat(300));
    assert!(astral.encode_utf16().count() <= 255);
    assert_eq!(
        sanitize_stem(&format!("{}...", "a".repeat(MAX_STEM_CHARS))).len(),
        MAX_STEM_CHARS
    );
    // A cut that lands on a trailing dot or space still ends clean.
    let tricky = format!("{} . .tail", "a".repeat(MAX_STEM_CHARS - 1));
    let stem = sanitize_stem(&tricky);
    assert!(!stem.ends_with(['.', ' ']), "{stem:?}");
}

#[test]
fn names_are_unique_ignoring_case() {
    let mut names = NameAllocator::new();
    names.reserve("assets");
    let got: Vec<String> = ["Notes", "notes", "NOTES", "Assets", "CON", "con", "Notes"]
        .iter()
        .map(|title| names.unique_stem(title))
        .collect();
    assert_eq!(
        got,
        [
            "Notes",
            "notes (2)",
            "NOTES (3)",
            "Assets (2)",
            "_CON",
            "_con (2)",
            "Notes (4)"
        ]
    );
}

#[test]
fn uniqueness_suffix_respects_the_length_cap() {
    let mut names = NameAllocator::new();
    let long = "x".repeat(250);
    let first = names.unique_stem(&long);
    let second = names.unique_stem(&long);
    assert_ne!(first.to_lowercase(), second.to_lowercase());
    assert!(second.chars().count() <= MAX_STEM_CHARS);
    assert!(second.ends_with(" (2)"));
}

#[test]
fn attachment_names_keep_their_extension() {
    let mut names = NameAllocator::new();
    assert_eq!(names.unique_file_name("Sơ đồ.PNG"), "Sơ đồ.PNG");
    // File systems compare names without case, so these clash.
    assert_eq!(names.unique_file_name("sơ đồ.png"), "sơ đồ (2).png");
    assert_eq!(names.unique_file_name("Sơ đồ.PNG"), "Sơ đồ (3).PNG");
    assert_eq!(names.unique_file_name("con.png"), "_con.png");
    assert_eq!(names.unique_file_name("a:b.png"), "a_b.png");
    let long = format!("{}.jpeg", "n".repeat(300));
    let name = names.unique_file_name(&long);
    assert!(
        name.ends_with(".jpeg") && name.chars().count() <= MAX_STEM_CHARS + 5,
        "{name}"
    );
    assert_eq!(names.unique_file_name("noext"), "noext");
    assert_eq!(names.unique_file_name(""), "Untitled");
}

// ---------------------------------------------------------------------------
// Folder export and import
// ---------------------------------------------------------------------------

fn fresh_tree() -> WorkspaceTree {
    let ticks = std::sync::atomic::AtomicI64::new(1);
    WorkspaceTree::with_peer_id(
        1,
        Arc::new(move || ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst)),
    )
    .expect("tree")
}

fn fake_hash(bytes: &[u8]) -> String {
    let mut out = String::new();
    for salt in 0u8..4 {
        let mut hasher = DefaultHasher::new();
        salt.hash(&mut hasher);
        bytes.hash(&mut hasher);
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

#[derive(Default)]
struct Source {
    pages: HashMap<NodeId, PageDoc>,
    attachments: HashMap<String, Attachment>,
}

impl PageSource for Source {
    fn page_doc(&self, id: NodeId) -> Result<Option<PageDoc>> {
        Ok(self.pages.get(&id).map(|doc| {
            PageDoc::from_snapshot(&doc.export_snapshot().expect("snapshot")).expect("load")
        }))
    }

    fn attachment(&self, hash: &str) -> Result<Option<Attachment>> {
        Ok(self.attachments.get(hash).cloned())
    }
}

#[derive(Default)]
struct Sink {
    pages: BTreeMap<NodeId, PageDoc>,
    attachments: BTreeMap<String, (String, Vec<u8>)>,
}

impl ImportSink for Sink {
    fn add_attachment(&mut self, file_name: &str, bytes: Vec<u8>) -> Result<String> {
        let hash = fake_hash(&bytes);
        self.attachments
            .insert(hash.clone(), (file_name.to_owned(), bytes));
        Ok(hash)
    }

    fn save_page(&mut self, id: NodeId, doc: PageDoc) -> Result<()> {
        self.pages.insert(id, doc);
        Ok(())
    }
}

struct Fixture {
    tree: WorkspaceTree,
    source: Source,
    folder: NodeId,
    parent: NodeId,
    child: NodeId,
    reserved: NodeId,
    png: Vec<u8>,
}

fn page_with(source: &mut Source, id: NodeId, blocks: &[Block]) {
    let doc = replica(1);
    doc.append_blocks(&[], blocks).expect("blocks");
    doc.delete_block(&[0]).expect("skeleton");
    source.pages.insert(id, doc);
}

fn fixture() -> Fixture {
    let tree = fresh_tree();
    let mut source = Source::default();
    let png = b"\x89PNG fake image bytes".to_vec();
    let hash = fake_hash(&png);
    source.attachments.insert(
        hash.clone(),
        Attachment {
            file_name: "Sơ đồ.png".into(),
            bytes: png.clone(),
        },
    );

    let folder = tree
        .create_folder(None, Position::End, "Dự án: Alpha?")
        .expect("folder");
    let reserved = tree
        .create_page(Some(folder), Position::End, "CON")
        .expect("page");
    let twin = tree
        .create_page(Some(folder), Position::End, "con")
        .expect("page");
    let parent = tree
        .create_page(Some(folder), Position::End, "Parent.")
        .expect("page");
    let child = tree
        .create_page(Some(parent), Position::End, "Child #1")
        .expect("page");
    let deep = tree
        .create_page(Some(child), Position::End, "Deep")
        .expect("page");
    let top = tree
        .create_page(None, Position::End, "assets")
        .expect("page");
    let blank = tree.create_page(None, Position::End, "").expect("page");
    let trashed = tree.create_page(None, Position::End, "Gone").expect("page");
    tree.trash_node(trashed).expect("trash");

    page_with(&mut source, reserved, &[Block::paragraph("Trang CON")]);
    page_with(&mut source, twin, &[Block::paragraph("twin")]);
    page_with(&mut source, deep, &[Block::paragraph("sâu")]);
    page_with(&mut source, top, &[Block::paragraph("assets page")]);
    page_with(&mut source, blank, &[Block::paragraph("no title")]);
    page_with(&mut source, trashed, &[Block::paragraph("never exported")]);
    page_with(
        &mut source,
        parent,
        &[
            Block::paragraph("Xin chào"),
            Block::new(BlockKind::ChildPage {
                page_id: child.to_string(),
                title: "stale".into(),
            }),
            Block::new(BlockKind::PageLink {
                page_id: reserved.to_string(),
                title: "stale".into(),
            }),
            Block::new(BlockKind::PageLink {
                page_id: "99@7".into(),
                title: "Outside".into(),
            }),
            Block::new(BlockKind::Image {
                src: attachment_src(&hash),
                alt: "Sơ đồ".into(),
            }),
            Block::new(BlockKind::Image {
                src: attachment_src(&"b".repeat(64)),
                alt: "missing".into(),
            }),
        ],
    );
    page_with(
        &mut source,
        child,
        &[
            Block::new(BlockKind::PageLink {
                page_id: parent.to_string(),
                title: "back".into(),
            }),
            Block::new(BlockKind::Image {
                src: attachment_src(&hash),
                alt: "again".into(),
            }),
            Block::new(BlockKind::ChildPage {
                page_id: deep.to_string(),
                title: "Deep".into(),
            }),
        ],
    );
    Fixture {
        tree,
        source,
        folder,
        parent,
        child,
        reserved,
        png,
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read file")
}

fn listing(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .expect("read dir")
            .map(|e| e.expect("entry").path())
            .collect();
        entries.sort();
        for path in entries {
            let rel = path
                .strip_prefix(base)
                .expect("inside")
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &path, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn export_mirrors_the_hierarchy_with_valid_unique_names() {
    let f = fixture();
    let dest = tempfile::tempdir().expect("tempdir");
    let report = export_tree(&f.tree, None, &f.source, dest.path()).expect("export");

    assert_eq!(
        listing(dest.path()),
        [
            "D\u{1ef1} \u{00e1}n_ Alpha_/",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent/",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent/Child #1/",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent/Child #1/Deep.md",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent/Child #1.md",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent/assets/",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent/assets/S\u{01a1} \u{0111}\u{1ed3}.png",
            "D\u{1ef1} \u{00e1}n_ Alpha_/Parent.md",
            "D\u{1ef1} \u{00e1}n_ Alpha_/_CON.md",
            "D\u{1ef1} \u{00e1}n_ Alpha_/_con (2).md",
            "D\u{1ef1} \u{00e1}n_ Alpha_/assets/",
            "D\u{1ef1} \u{00e1}n_ Alpha_/assets/S\u{01a1} \u{0111}\u{1ed3}.png",
            "Untitled.md",
            "assets (2).md",
        ]
    );
    assert_eq!(report.folders, 1);
    assert_eq!(report.pages, 7, "trashed pages are skipped");
    assert_eq!(report.attachments, 2);
    assert_eq!(report.missing_attachments, ["b".repeat(64)]);
}

#[test]
fn export_writes_relative_links_titles_and_attachments() {
    let f = fixture();
    let dest = tempfile::tempdir().expect("tempdir");
    export_tree(&f.tree, None, &f.source, dest.path()).expect("export");
    let dir = dest.path().join("Dự án_ Alpha_");

    let parent_md = read(&dir.join("Parent.md"));
    assert_eq!(
        parent_md,
        "# Parent.\n\nXin chào\n\n[Child \\#1](<Parent/Child #1.md>)\n\n[CON](_CON.md)\n\n\
         [Outside](ne-page:99@7)\n\n![Sơ đồ](<assets/Sơ đồ.png>)\n\n![missing](attachment:"
            .to_owned()
            + &"b".repeat(64)
            + ")\n"
    );
    // The attachment is written once per directory that has a page showing it.
    let child_md = read(&dir.join("Parent").join("Child #1.md"));
    assert_eq!(
        child_md,
        "# Child \\#1\n\n[Parent.](../Parent.md)\n\n![again](<assets/Sơ đồ.png>)\n\n[Deep](<Child #1/Deep.md>)\n"
    );
    assert_eq!(
        fs::read(dir.join("Parent").join("assets").join("Sơ đồ.png")).expect("attachment"),
        f.png
    );
    assert_eq!(
        fs::read(dir.join("assets").join("Sơ đồ.png")).expect("attachment"),
        f.png
    );
    assert_eq!(read(&dest.path().join("Untitled.md")), "no title\n");
}

#[test]
fn export_of_a_subtree_starts_at_that_node() {
    let f = fixture();
    let dest = tempfile::tempdir().expect("tempdir");
    export_tree(&f.tree, Some(f.child), &f.source, dest.path()).expect("export");
    assert_eq!(
        listing(dest.path()),
        [
            "Child #1/",
            "Child #1/Deep.md",
            "Child #1.md",
            "assets/",
            "assets/Sơ đồ.png"
        ]
    );
    // The link back to a page outside the export keeps the page scheme.
    assert!(read(&dest.path().join("Child #1.md")).contains(&format!("(ne-page:{})", f.parent)));
}

#[test]
fn export_never_overwrites_existing_files() {
    let f = fixture();
    let dest = tempfile::tempdir().expect("tempdir");
    export_tree(&f.tree, Some(f.reserved), &f.source, dest.path()).expect("first export");
    let err = export_tree(&f.tree, Some(f.reserved), &f.source, dest.path()).expect_err("clash");
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert_eq!(read(&dest.path().join("_CON.md")), "# CON\n\nTrang CON\n");
}

fn titles(nodes: &[NodeInfo]) -> Vec<(String, NodeKind)> {
    nodes.iter().map(|n| (n.title.clone(), n.kind)).collect()
}

#[test]
fn export_then_import_restores_hierarchy_titles_links_and_images() {
    let f = fixture();
    let dest = tempfile::tempdir().expect("tempdir");
    export_tree(&f.tree, Some(f.folder), &f.source, dest.path()).expect("export");

    let target = fresh_tree();
    let mut sink = Sink::default();
    let report = import_directory(&target, None, dest.path(), &mut sink).expect("import");
    assert_eq!(report.pages, 5);
    assert_eq!(
        report.folders, 1,
        "only the exported folder node, not the page directories"
    );
    assert_eq!(
        report.attachments, 2,
        "one image file per folder, the sink dedups by content"
    );
    assert_eq!(
        sink.attachments.len(),
        1,
        "the same bytes are one stored attachment"
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);

    let top = target.children(None, false).expect("top");
    assert_eq!(
        titles(&top),
        [("Dự án_ Alpha_".to_owned(), NodeKind::Folder)]
    );
    let inside = target.children(Some(top[0].id), false).expect("inside");
    // Siblings come back ordered by file name; titles are restored from the headings.
    assert_eq!(
        titles(&inside),
        [
            ("CON".to_owned(), NodeKind::Page),
            ("con".to_owned(), NodeKind::Page),
            ("Parent.".to_owned(), NodeKind::Page),
        ]
    );
    let parent = inside[2].id;
    let kids = target.children(Some(parent), false).expect("children");
    assert_eq!(titles(&kids), [("Child #1".to_owned(), NodeKind::Page)]);
    let child = kids[0].id;
    let deep = target.children(Some(child), false).expect("grandchildren");
    assert_eq!(titles(&deep), [("Deep".to_owned(), NodeKind::Page)]);

    // Page content: links resolved to the new ids, images stored as attachments.
    let imported_hash = fake_hash(&f.png);
    let parent_blocks = sink.pages[&parent].blocks().expect("blocks");
    assert_eq!(parent_blocks[0].plain_text(), "Xin chào");
    assert_eq!(
        parent_blocks[1].kind,
        BlockKind::ChildPage {
            page_id: child.to_string(),
            title: "Child #1".into()
        }
    );
    assert_eq!(
        parent_blocks[2].kind,
        BlockKind::PageLink {
            page_id: inside[0].id.to_string(),
            title: "CON".into()
        }
    );
    assert_eq!(
        parent_blocks[3].kind,
        BlockKind::PageLink {
            page_id: "99@7".into(),
            title: "Outside".into()
        }
    );
    assert_eq!(
        parent_blocks[4].kind,
        BlockKind::Image {
            src: attachment_src(&imported_hash),
            alt: "Sơ đồ".into()
        }
    );
    assert!(matches!(
        &parent_blocks[5].kind,
        BlockKind::Image { src, .. } if src == &format!("attachment:{}", "b".repeat(64))
    ));
    assert_eq!(sink.attachments[&imported_hash].1, f.png);
    assert_eq!(sink.attachments[&imported_hash].0, "Sơ đồ.png");
    assert!(parse_attachment_src(&attachment_src(&imported_hash)).is_some());

    let child_blocks = sink.pages[&child].blocks().expect("blocks");
    assert_eq!(
        child_blocks[0].kind,
        BlockKind::PageLink {
            page_id: parent.to_string(),
            title: "Parent.".into()
        }
    );
    assert_eq!(
        child_blocks[2].kind,
        BlockKind::ChildPage {
            page_id: deep[0].id.to_string(),
            title: "Deep".into()
        }
    );
    assert_eq!(sink.pages[&deep[0].id].plain_text().expect("text"), "sâu");

    // Re-exporting the imported tree reproduces the same files.
    let again = tempfile::tempdir().expect("tempdir");
    let source = Source {
        pages: sink.pages.into_iter().collect(),
        attachments: HashMap::from([(
            imported_hash.clone(),
            Attachment {
                file_name: "Sơ đồ.png".into(),
                bytes: f.png.clone(),
            },
        )]),
    };
    export_tree(&target, Some(top[0].id), &source, again.path()).expect("re-export");
    assert_eq!(listing(dest.path()), listing(again.path()));
    for name in ["_CON.md", "_con (2).md"] {
        assert_eq!(
            read(&dest.path().join("Dự án_ Alpha_").join(name)),
            read(&again.path().join("Dự án_ Alpha_").join(name))
        );
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

#[test]
fn import_reads_a_foreign_markdown_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(&root.join("Readme.md"), "Top level page\n");
    write(&root.join("b-notes/Zeta.md"), "# Zeta title\n\nz\n");
    write(&root.join("b-notes/alpha.MD"), "alpha\n");
    write(
        &root.join("Guide.md"),
        "Guide [intro](guide/Intro%20Page.md) and [bad](../outside.md)\n\n![pic](img/p%20q.png)\n![up](../secret.png)\n![web](https://e.org/x.png)\n![via link](outer/x.png)\n",
    );
    write(
        &root.join("guide/Intro Page.md"),
        "intro\n\n[back](../Guide.md#top)\n",
    );
    write(&root.join("guide/img-only/ignored.txt"), "not markdown");
    write(&root.join("empty-dir/.gitkeep"), "");
    write(&root.join(".hidden/Secret.md"), "hidden");
    write(&root.join("notes.txt"), "not markdown");
    write(&root.join("img/p q.png"), "PNGDATA");
    write(&root.join("../outside.md"), "outside");
    let outside = tempfile::tempdir().expect("tempdir");
    write(&outside.path().join("x.png"), "OUTSIDE");
    fs::write(root.join("../secret.png"), "SECRET").expect("secret");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("img/p q.png"), root.join("link.md"))
            .expect("symlink");
        std::os::unix::fs::symlink(root.join("b-notes"), root.join("loop")).expect("symlink");
        std::os::unix::fs::symlink(outside.path(), root.join("outer")).expect("symlink");
    }

    let tree = fresh_tree();
    let anchor = tree
        .create_folder(None, Position::End, "Imported")
        .expect("anchor");
    let mut sink = Sink::default();
    let report = import_directory(&tree, Some(anchor), root, &mut sink).expect("import");

    let top = tree.children(Some(anchor), false).expect("top");
    assert_eq!(
        titles(&top),
        [
            ("b-notes".to_owned(), NodeKind::Folder),
            ("Guide".to_owned(), NodeKind::Page),
            ("Readme".to_owned(), NodeKind::Page),
        ],
        "ordered by name, empty and hidden directories left out"
    );
    let notes = tree.children(Some(top[0].id), false).expect("notes");
    assert_eq!(
        titles(&notes),
        [
            ("alpha".to_owned(), NodeKind::Page),
            ("Zeta title".to_owned(), NodeKind::Page)
        ]
    );
    let guide = top[1].id;
    let intro = tree.children(Some(guide), false).expect("guide children");
    assert_eq!(titles(&intro), [("Intro Page".to_owned(), NodeKind::Page)]);

    let guide_blocks = sink.pages[&guide].blocks().expect("blocks");
    let Block { inline, .. } = &guide_blocks[0];
    let hrefs: Vec<_> = inline
        .iter()
        .filter_map(|i| match i {
            ne_core::page::Inline::Text(r) => r.marks.link.clone(),
            ne_core::page::Inline::Break => None,
        })
        .collect();
    assert_eq!(
        hrefs,
        [
            format!("ne-page:{}", intro[0].id),
            "../outside.md".to_owned()
        ]
    );
    let images: Vec<_> = guide_blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Image { src, .. } => Some(src.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(images[0], attachment_src(&fake_hash(b"PNGDATA")));
    assert_eq!(
        images[1], "../secret.png",
        "files outside the folder are never read"
    );
    assert_eq!(images[2], "https://e.org/x.png");
    assert_eq!(
        images[3], "outer/x.png",
        "a symlinked directory cannot lead outside"
    );
    assert_eq!(sink.attachments.len(), 1);
    assert_eq!(report.attachments, 1);
    assert!(
        sink.attachments
            .values()
            .all(|(_, bytes)| bytes != b"OUTSIDE")
    );
    assert_eq!(
        report.skipped,
        ["outer/x.png: image not found or not readable"]
    );

    // The link back from the child resolves to its parent despite the `#top` fragment.
    let intro_blocks = sink.pages[&intro[0].id].blocks().expect("blocks");
    assert_eq!(
        intro_blocks[1].kind,
        BlockKind::PageLink {
            page_id: guide.to_string(),
            title: "Guide".into()
        }
    );
    assert!(sink.pages.values().all(|doc| doc.blocks().is_ok()));
    assert_eq!(
        report.pages, 5,
        "symlinks and hidden files are not imported: {report:?}"
    );
    let _ = fs::remove_file(root.join("../secret.png"));
    let _ = fs::remove_file(root.join("../outside.md"));
}

#[test]
fn import_rejects_a_missing_directory_and_handles_an_empty_one() {
    let tree = fresh_tree();
    let mut sink = Sink::default();
    let missing = tempfile::tempdir().expect("tempdir").path().join("nope");
    assert!(matches!(
        import_directory(&tree, None, &missing, &mut sink),
        Err(Error::Io(_))
    ));
    let empty = tempfile::tempdir().expect("tempdir");
    let report = import_directory(&tree, None, empty.path(), &mut sink).expect("empty import");
    assert_eq!(report.pages, 0);
    assert!(tree.snapshot().expect("snapshot").is_empty());
}
