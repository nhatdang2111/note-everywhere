//! Full-text search: Vietnamese folding, prefix and substring matching,
//! ranking, snippets, and what the index leaves out.

mod store_support;

use ne_core::NodeId;
use ne_core::page::{Block, BlockKind};
use ne_core::tree::Position;
use ne_store::{SearchHit, Workspace, fold};
use store_support::*;

fn ids(hits: &[SearchHit]) -> Vec<NodeId> {
    hits.iter().map(|h| h.id).collect()
}

fn found(ws: &Workspace, query: &str) -> Vec<NodeId> {
    ids(&ws.search(query, 50).expect("search"))
}

#[test]
fn fold_removes_diacritics_and_maps_d_stroke() {
    assert_eq!(fold("Ghi chú"), "ghi chu");
    assert_eq!(fold("ĐƯỜNG đi Hà Nội"), "duong di ha noi");
    // Decomposed and precomposed spellings fold alike.
    assert_eq!(fold("u\u{0301}"), fold("ú"));
    assert_eq!(fold("e\u{0302}\u{0301}"), "e");
    assert_eq!(fold("Thương mại"), "thuong mai");
    assert_eq!(fold("ÀÁẢÃẠ"), "aaaaa");
    assert_eq!(fold("straße café"), "straße cafe");
    assert_eq!(fold("ＡＢＣ"), "ＡＢＣ".to_lowercase());
}

#[test]
fn unaccented_queries_find_accented_text() {
    let (_dir, mut ws) = new_workspace();
    let notes = page_with(
        &mut ws,
        "Ghi chú họp",
        &paragraphs(&["Nội dung cuộc họp tuần"]),
    );
    let road = page_with(
        &mut ws,
        "Đường về nhà",
        &paragraphs(&["Con đường quen thuộc"]),
    );
    page_with(&mut ws, "Khác hẳn", &paragraphs(&["không liên quan"]));

    assert_eq!(found(&ws, "ghi chu"), [notes]);
    assert_eq!(
        found(&ws, "Ghi Chú"),
        [notes],
        "accents and case in the query are ignored too"
    );
    assert_eq!(found(&ws, "duong"), [road]);
    assert_eq!(found(&ws, "đường"), [road]);
    assert_eq!(
        found(&ws, "dd"),
        Vec::<NodeId>::new(),
        "d-stroke folds to d, not dd"
    );
    assert_eq!(found(&ws, "cuoc hop"), [notes]);
    assert_eq!(
        found(&ws, "noi dung tuan"),
        [notes],
        "every term must match"
    );
    assert!(found(&ws, "ghi chu khong co").is_empty());
}

#[test]
fn decomposed_text_in_a_page_is_found_by_composed_and_plain_queries() {
    let (_dir, mut ws) = new_workspace();
    let page = page_with(
        &mut ws,
        "Tiếng Việt",
        &paragraphs(&["Ghi chu\u{0301} bu\u{031b}\u{0323}c tha\u{0300}nh"]),
    );
    for query in [
        "ghi chú",
        "ghi chu",
        "ghi chu\u{0301}",
        "bực thành",
        "buc thanh",
    ] {
        assert_eq!(found(&ws, query), [page], "query {query:?}");
    }
}

#[test]
fn prefixes_substrings_and_short_terms() {
    let (_dir, mut ws) = new_workspace();
    let a = page_with(
        &mut ws,
        "Kế hoạch tháng mười",
        &paragraphs(&["chuẩn bị ngân sách"]),
    );
    let b = page_with(&mut ws, "Nghiên cứu", &paragraphs(&["tài liệu tham khảo"]));
    let c = page_with(&mut ws, "Ăn tối", &paragraphs(&["nhà hàng gần đây"]));

    assert_eq!(found(&ws, "ke hoa"), [a], "prefix of a word");
    assert_eq!(found(&ws, "hoach"), [a]);
    assert_eq!(found(&ws, "oach thang"), [a], "substring terms, any order");
    assert_eq!(found(&ws, "ghi"), [b], "substring inside a word");
    assert_eq!(found(&ws, "tham kha"), [b]);
    assert_eq!(
        found(&ws, "to"),
        [c],
        "a short term alone matches titles only"
    );
    assert!(found(&ws, "nh").is_empty(), "\"nh\" is only in bodies");
    assert!(found(&ws, "z").is_empty());
    // A short term next to a long one filters the candidates.
    assert_eq!(found(&ws, "ngan ke"), [a]);
    assert_eq!(found(&ws, "ngan ab"), Vec::<NodeId>::new());
    assert!(found(&ws, "").is_empty());
    assert!(found(&ws, "   ").is_empty());
    assert!(ws.search("hoach", 0).expect("search").is_empty());
}

#[test]
fn queries_with_syntax_characters_are_taken_literally() {
    let (_dir, mut ws) = new_workspace();
    let page = page_with(
        &mut ws,
        "C++ và \"quote\"",
        &paragraphs(&["a OR b AND NOT c* (d) NEAR e: f-g"]),
    );
    for query in [
        "\"quote\"",
        "OR b AND",
        "NOT c*",
        "(d)",
        "NEAR e:",
        "f-g",
        "AND OR NOT",
        "\"",
        "*",
        "title:x",
    ] {
        // No query may fail; those that exist in the text are found.
        let hits = found(&ws, query);
        if ["\"quote\"", "OR b AND", "NOT c*", "(d)", "NEAR e:", "f-g"].contains(&query) {
            assert_eq!(hits, [page], "query {query:?}");
        }
    }
}

#[test]
fn title_matches_rank_above_body_matches() {
    let (_dir, mut ws) = new_workspace();
    let body_only = page_with(
        &mut ws,
        "Tạp nhớ",
        &paragraphs(&["Hôm nay tôi học về bảng tuần hoàn"]),
    );
    let in_title = page_with(&mut ws, "Bảng tuần hoàn", &paragraphs(&["các nguyên tố"]));
    let hits = ws.search("bang tuan hoan", 10).expect("search");
    assert_eq!(ids(&hits), [in_title, body_only]);
    assert!(hits[0].score > hits[1].score);
    assert!(!hits[0].title_highlights.is_empty());
    assert!(hits[1].title_highlights.is_empty());
}

#[test]
fn snippets_mark_the_matched_words_with_their_original_accents() {
    let (_dir, mut ws) = new_workspace();
    let long = format!(
        "{} Chúng tôi bàn về kế hoạch năm sau. {}",
        "đầu ".repeat(60),
        "cuối ".repeat(60)
    );
    let page = page_with(&mut ws, "Họp", &paragraphs(&[&long]));
    let hit = &ws.search("ke hoach", 5).expect("search")[0];
    assert_eq!(hit.id, page);
    let snippet = &hit.snippet;
    assert!(
        snippet.text.starts_with('…') && snippet.text.ends_with('…'),
        "{:?}",
        snippet.text
    );
    assert!(snippet.text.chars().count() < 200);
    assert!(!snippet.text.contains('\n'));
    let marked: Vec<&str> = snippet
        .highlights
        .iter()
        .map(|r| &snippet.text[r.clone()])
        .collect();
    assert_eq!(
        marked,
        ["kế", "hoạch"],
        "highlights slice the accented original"
    );
    for window in snippet.highlights.windows(2) {
        assert!(window[0].end <= window[1].start, "sorted and disjoint");
    }
}

#[test]
fn snippet_of_a_title_only_match_shows_the_start_of_the_body() {
    let (_dir, mut ws) = new_workspace();
    page_with(
        &mut ws,
        "Danh sách mua sắm",
        &paragraphs(&["sữa, trứng, bánh mì"]),
    );
    let hit = &ws.search("mua sam", 5).expect("search")[0];
    assert_eq!(hit.snippet.text, "sữa, trứng, bánh mì");
    assert!(hit.snippet.highlights.is_empty());
    let marked: Vec<&str> = hit
        .title_highlights
        .iter()
        .map(|r| &hit.title[r.clone()])
        .collect();
    assert_eq!(marked, ["mua", "sắm"]);
}

#[test]
fn snippets_handle_multibyte_text_at_the_edges() {
    let (_dir, mut ws) = new_workspace();
    let text = format!("{}😀 cần tìm ở đây 😀{}", "ấ".repeat(70), "ộ".repeat(70));
    page_with(&mut ws, "Emoji", &paragraphs(&[&text]));
    let hit = &ws.search("can tim", 5).expect("search")[0];
    let marked: Vec<&str> = hit
        .snippet
        .highlights
        .iter()
        .map(|r| &hit.snippet.text[r.clone()])
        .collect();
    assert_eq!(marked, ["cần", "tìm"]);
}

#[test]
fn the_index_follows_edits_renames_and_trash() {
    let (_dir, mut ws) = new_workspace();
    let page = page_with(&mut ws, "Nháp", &paragraphs(&["bản đầu tiên"]));
    assert_eq!(found(&ws, "ban dau"), [page]);

    ws.edit_page(page, |d| {
        d.set_inline(&[0], &[ne_core::page::Inline::text("bản sửa lại")])
    })
    .expect("edit");
    assert!(found(&ws, "ban dau").is_empty(), "old text is gone");
    assert_eq!(found(&ws, "sua lai"), [page]);

    ws.rename(page, "Chính thức").expect("rename");
    assert_eq!(found(&ws, "chinh thuc"), [page]);
    assert!(found(&ws, "nhap").is_empty());
    assert_eq!(
        found(&ws, "sua lai"),
        [page],
        "renaming keeps the body indexed"
    );

    ws.trash_node(page).expect("trash");
    assert!(found(&ws, "sua lai").is_empty());
    assert!(found(&ws, "chinh thuc").is_empty());
    ws.restore(page).expect("restore");
    assert_eq!(found(&ws, "sua lai"), [page]);
}

#[test]
fn folders_are_found_by_title_and_trashing_a_folder_hides_its_pages() {
    let (_dir, mut ws) = new_workspace();
    let folder = ws
        .create_folder(None, Position::End, "Công việc")
        .expect("folder");
    let page = page_in(
        &mut ws,
        Some(folder),
        "Báo cáo",
        &paragraphs(&["số liệu quý"]),
    );
    assert_eq!(found(&ws, "cong viec"), [folder]);
    assert_eq!(found(&ws, "so lieu"), [page]);
    ws.trash_node(folder).expect("trash");
    assert!(found(&ws, "cong viec").is_empty());
    assert!(found(&ws, "so lieu").is_empty());
    ws.restore(folder).expect("restore");
    assert_eq!(found(&ws, "so lieu"), [page]);
}

#[test]
fn limit_and_result_shape() {
    let (_dir, mut ws) = new_workspace();
    for i in 0..12 {
        page_with(
            &mut ws,
            &format!("Mục lục {i}"),
            &paragraphs(&["chung chung"]),
        );
    }
    assert_eq!(ws.search("muc luc", 5).expect("search").len(), 5);
    assert_eq!(ws.search("muc luc", 100).expect("search").len(), 12);
    let hit = &ws.search("muc luc 7", 5).expect("search")[0];
    assert_eq!(hit.title, "Mục lục 7");
    assert_eq!(hit.kind, ne_core::tree::NodeKind::Page);
}

#[test]
fn vault_documents_are_never_indexed() {
    let (_dir, mut ws) = new_workspace();
    ws.put_vault_doc("vault/main", b"secret-password-hunter2 token", b"v1")
        .expect("put");
    ws.apply_remote_update(
        &ne_store::DocRef::Vault("vault/main".into()),
        b"another-secret-update",
    )
    .expect("remote vault update");
    assert!(ws.search("hunter2", 10).expect("search").is_empty());
    assert!(ws.search("another secret", 10).expect("search").is_empty());
    let doc = ws.vault_doc("vault/main").expect("get").expect("stored");
    assert_eq!(doc.snapshot, b"secret-password-hunter2 token");
    assert_eq!(doc.updates, [b"another-secret-update".to_vec()]);
    assert_eq!(ws.vault_doc_ids().expect("ids"), ["vault/main"]);
    ws.dump_derived_indexes()
        .map(|dump| assert!(!dump.contains("hunter2")))
        .expect("dump");
    ws.delete_vault_doc("vault/main").expect("delete");
    assert!(ws.vault_doc("vault/main").expect("get").is_none());
}

#[test]
fn rebuilding_gives_the_same_results() {
    let (_dir, mut ws) = new_workspace();
    page_with(&mut ws, "Một", &paragraphs(&["quả táo đỏ"]));
    page_with(
        &mut ws,
        "Hai",
        &[
            Block::new(BlockKind::Divider),
            Block::paragraph("quả cam vàng"),
        ],
    );
    let queries = ["qua tao", "cam", "vang", "hai", "mot"];
    let before: Vec<_> = queries.iter().map(|q| found(&ws, q)).collect();
    let dump = ws.dump_derived_indexes().expect("dump");
    ws.rebuild_indexes().expect("rebuild");
    let after: Vec<_> = queries.iter().map(|q| found(&ws, q)).collect();
    assert_eq!(before, after);
    assert_eq!(dump, ws.dump_derived_indexes().expect("dump"));
}
