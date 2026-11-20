//! Full-text search: folding text, the index, ranked queries and snippets.
//!
//! # Folding
//!
//! Titles and bodies are folded before they enter the index, and queries are
//! folded the same way: lowercase, canonical decomposition, combining marks
//! dropped, and `đ`/`Đ` mapped to `d` (SQLite's own `remove_diacritics` does
//! not do the last one). "ghi chu" therefore finds "Ghi chú", and "duong"
//! finds "đường", whichever way the accents were typed.
//!
//! # Matching
//!
//! The index is an FTS5 table with the trigram tokenizer, so any term of three
//! or more characters matches as a substring (which includes prefixes). All
//! terms must match (AND). Terms of one or two characters cannot use the
//! index: next to longer terms they filter the candidates; on their own they
//! match titles only.

use std::collections::HashSet;
use std::ops::Range;

use ne_core::NodeId;
use ne_core::tree::NodeKind;
use rusqlite::{Connection, OptionalExtension, params};
use unicode_normalization::char::{decompose_canonical, is_combining_mark};

use crate::error::Result;

/// Terms shorter than this many characters cannot use the trigram index.
const MIN_INDEXED_TERM_CHARS: usize = 3;
/// Most terms of a query that are used; the rest are ignored.
const MAX_TERMS: usize = 8;
/// Characters of context kept on each side of the first match in a snippet.
const SNIPPET_CONTEXT_CHARS: usize = 60;
/// Longest snippet when nothing in the body matched.
const SNIPPET_FALLBACK_CHARS: usize = 140;
/// Weight of a title match relative to a body match when ranking.
const TITLE_WEIGHT: f64 = 8.0;

/// Folds text for matching: lowercase, no diacritics, `đ` as `d`.
pub fn fold(text: &str) -> String {
    fold_with_spans(text).text.into_iter().collect()
}

fn push_folded(c: char, out: &mut Vec<char>) {
    for lower in c.to_lowercase() {
        decompose_canonical(lower, |piece| {
            if !is_combining_mark(piece) {
                out.push(if piece == 'đ' { 'd' } else { piece });
            }
        });
    }
}

/// Folded text with, for every folded character, the byte range of the
/// original character it came from (a standalone combining mark yields no
/// folded character; a character that expands yields several sharing a range).
struct Folded {
    text: Vec<char>,
    spans: Vec<Range<usize>>,
}

fn fold_with_spans(text: &str) -> Folded {
    let mut folded = Folded {
        text: Vec::with_capacity(text.len()),
        spans: Vec::with_capacity(text.len()),
    };
    for (start, c) in text.char_indices() {
        let before = folded.text.len();
        push_folded(c, &mut folded.text);
        let span = start..start + c.len_utf8();
        folded
            .spans
            .extend(std::iter::repeat_n(span, folded.text.len() - before));
    }
    folded
}

/// Splits a user query into distinct folded terms.
pub(crate) fn terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    fold(query)
        .split_whitespace()
        .filter(|t| seen.insert(t.to_owned()))
        .take(MAX_TERMS)
        .map(str::to_owned)
        .collect()
}

fn is_indexed(term: &str) -> bool {
    term.chars().count() >= MIN_INDEXED_TERM_CHARS
}

/// The FTS5 expression for the terms the index can answer.
fn match_expression(terms: &[String]) -> String {
    terms
        .iter()
        .filter(|t| is_indexed(t))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// A short excerpt of a page body with the matched words marked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snippet {
    /// The excerpt, on one line, with `…` where text was cut.
    pub text: String,
    /// Byte ranges of `text` that matched the query, sorted and disjoint.
    pub highlights: Vec<Range<usize>>,
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The page or folder found.
    pub id: NodeId,
    /// Folder or page.
    pub kind: NodeKind,
    /// Its title.
    pub title: String,
    /// Its icon, if any.
    pub icon: Option<String>,
    /// Byte ranges of `title` that matched the query.
    pub title_highlights: Vec<Range<usize>>,
    /// Excerpt of the body around the first match (the start of the body when
    /// only the title matched).
    pub snippet: Snippet,
    /// Relevance; higher is better. Title matches weigh more than body matches.
    pub score: f64,
}

/// Every character range of `folded` where one of `terms` occurs, mapped to
/// byte ranges of the original text, merged and sorted.
fn match_ranges(folded: &Folded, terms: &[String]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for term in terms {
        let needle: Vec<char> = term.chars().collect();
        if needle.is_empty() || needle.len() > folded.text.len() {
            continue;
        }
        let last_start = folded.text.len() - needle.len();
        for start in 0..=last_start {
            if folded.text[start] == needle[0]
                && folded.text[start..start + needle.len()] == needle[..]
            {
                ranges.push(folded.spans[start].start..folded.spans[start + needle.len() - 1].end);
            }
        }
    }
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// Byte offset of the character `count` characters before `from`.
fn back_chars(text: &str, from: usize, count: usize) -> usize {
    text[..from]
        .char_indices()
        .rev()
        .nth(count.saturating_sub(1))
        .map_or(0, |(i, _)| i)
        .min(from)
}

/// Byte offset of the character `count` characters after `from`.
fn forward_chars(text: &str, from: usize, count: usize) -> usize {
    text[from..]
        .char_indices()
        .nth(count)
        .map_or(text.len(), |(i, _)| from + i)
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Cuts an excerpt of `body` around the first match of `terms`.
fn snippet_of(body: &str, folded: &Folded, terms: &[String]) -> Snippet {
    let matches = match_ranges(folded, terms);
    let Some(first) = matches.first() else {
        let end = forward_chars(body, 0, SNIPPET_FALLBACK_CHARS);
        let mut text = one_line(&body[..end]);
        if end < body.len() {
            text.push('…');
        }
        return Snippet {
            text,
            highlights: Vec::new(),
        };
    };
    let mut start = back_chars(body, first.start, SNIPPET_CONTEXT_CHARS);
    if start > 0 {
        // Begin at a word boundary instead of inside a word.
        if let Some(space) = body[start..first.start].find(char::is_whitespace) {
            start += space + 1;
        }
    }
    let end = forward_chars(body, first.start, SNIPPET_CONTEXT_CHARS * 2);
    let lead = if start > 0 { "…" } else { "" };
    let mut text = String::from(lead);
    text.push_str(&one_line(&body[start..end]));
    if end < body.len() {
        text.push('…');
    }
    let highlights = matches
        .iter()
        .filter(|m| m.start >= start && m.end <= end)
        .map(|m| (m.start - start + lead.len())..(m.end - start + lead.len()))
        .collect();
    Snippet { text, highlights }
}

// ---------------------------------------------------------------------------
// Index maintenance
// ---------------------------------------------------------------------------

/// Replaces the index entry of node `rid`. Both texts are already folded.
pub(crate) fn index_node(
    conn: &Connection,
    rid: i64,
    title_norm: &str,
    body_norm: &str,
) -> Result<()> {
    conn.prepare_cached("DELETE FROM search WHERE rowid = ?1")?
        .execute([rid])?;
    conn.prepare_cached("INSERT INTO search (rowid, title, body) VALUES (?1, ?2, ?3)")?
        .execute(params![rid, title_norm, body_norm])?;
    Ok(())
}

/// Removes the index entry of node `rid`.
pub(crate) fn remove_node(conn: &Connection, rid: i64) -> Result<()> {
    conn.execute("DELETE FROM search WHERE rowid = ?1", [rid])?;
    Ok(())
}

/// The folded body of a page, if it has been indexed.
pub(crate) fn body_norm_of(conn: &Connection, rid: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT body_norm FROM page_text WHERE rid = ?1",
            [rid],
            |r| r.get(0),
        )
        .optional()?)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

struct Candidate {
    id: String,
    kind: String,
    title: String,
    icon: Option<String>,
    body: String,
    score: f64,
}

fn candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        id: row.get(0)?,
        kind: row.get(1)?,
        title: row.get(2)?,
        icon: row.get(3)?,
        body: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        score: row.get::<_, Option<f64>>(5)?.unwrap_or_default(),
    })
}

/// Nodes matching the indexed terms (`expression`) and containing every short
/// term, best first.
fn indexed_candidates(
    conn: &Connection,
    expression: &str,
    short: &[&String],
    limit: usize,
) -> Result<Vec<Candidate>> {
    // ?1 expression, ?2 limit, ?3 title weight, ?4.. short terms.
    let filters: String = (0..short.len())
        .map(|i| {
            format!(
                " AND instr(n.title_norm || ' ' || COALESCE(p.body_norm, ''), ?{}) > 0",
                i + 4
            )
        })
        .collect();
    let mut statement = conn.prepare_cached(&format!(
        "SELECT n.id, n.kind, n.title, n.icon, p.body, -bm25(search, ?3, 1.0)
         FROM search
         JOIN nodes n ON n.rid = search.rowid
         LEFT JOIN page_text p ON p.rid = n.rid
         WHERE search MATCH ?1 AND n.hidden = 0{filters}
         ORDER BY bm25(search, ?3, 1.0)
         LIMIT ?2"
    ))?;
    let mut values: Vec<rusqlite::types::Value> = vec![
        expression.to_owned().into(),
        i64::try_from(limit).unwrap_or(i64::MAX).into(),
        TITLE_WEIGHT.into(),
    ];
    values.extend(
        short
            .iter()
            .map(|t| rusqlite::types::Value::from((*t).clone())),
    );
    let rows = statement.query_map(rusqlite::params_from_iter(values), candidate)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Titles that contain every term, for queries the index cannot answer: the
/// closest to what was typed first (earlier match, then shorter title).
fn title_candidates(conn: &Connection, terms: &[String], limit: usize) -> Result<Vec<Candidate>> {
    let filters: String = (0..terms.len())
        .map(|i| format!(" AND instr(n.title_norm, ?{}) > 0", i + 2))
        .collect();
    let mut statement = conn.prepare_cached(&format!(
        "SELECT n.id, n.kind, n.title, n.icon, p.body, 0.0
         FROM nodes n LEFT JOIN page_text p ON p.rid = n.rid
         WHERE n.hidden = 0{filters}
         ORDER BY instr(n.title_norm, ?2), length(n.title_norm), n.id
         LIMIT ?1"
    ))?;
    let mut values: Vec<rusqlite::types::Value> =
        vec![i64::try_from(limit).unwrap_or(i64::MAX).into()];
    values.extend(
        terms
            .iter()
            .map(|t| rusqlite::types::Value::from(t.clone())),
    );
    let rows = statement.query_map(rusqlite::params_from_iter(values), candidate)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Runs a search; see the [module documentation](self) for the rules. Trashed
/// pages (and everything below a trashed node) are not found.
pub(crate) fn run(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let terms = terms(query);
    if terms.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let expression = match_expression(&terms);
    let short: Vec<&String> = terms.iter().filter(|t| !is_indexed(t)).collect();
    let candidates = if expression.is_empty() {
        title_candidates(conn, &terms, limit)?
    } else {
        indexed_candidates(conn, &expression, &short, limit)?
    };
    let mut hits = Vec::with_capacity(candidates.len());
    for c in candidates {
        let Ok(id) = c.id.parse::<NodeId>() else {
            continue;
        };
        let kind = if c.kind == "page" {
            NodeKind::Page
        } else {
            NodeKind::Folder
        };
        let title_folded = fold_with_spans(&c.title);
        let snippet = snippet_of(&c.body, &fold_with_spans(&c.body), &terms);
        hits.push(SearchHit {
            id,
            kind,
            title_highlights: match_ranges(&title_folded, &terms),
            snippet,
            title: c.title,
            icon: c.icon,
            score: c.score,
        });
    }
    Ok(hits)
}
