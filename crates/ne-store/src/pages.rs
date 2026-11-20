//! Derived data of pages: body text for search, links for backlinks and
//! attachment references. All of it is computed from the page document.

use ne_core::NodeId;
use ne_core::page::{LinkKind, PageDoc};
use rusqlite::{Connection, params};

use crate::blobs;
use crate::error::{Error, Result};
use crate::nodes;
use crate::search;

/// A page that links to another page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    /// The linking page.
    pub source: NodeId,
    /// Its title.
    pub title: String,
    /// How it links: a link block, inline link text, or both.
    pub kinds: Vec<LinkKind>,
}

fn kind_code(kind: LinkKind) -> i64 {
    match kind {
        LinkKind::ChildPage => 0,
        LinkKind::PageLink => 1,
        LinkKind::Inline => 2,
    }
}

fn kind_from_code(code: i64) -> Option<LinkKind> {
    match code {
        0 => Some(LinkKind::ChildPage),
        1 => Some(LinkKind::PageLink),
        2 => Some(LinkKind::Inline),
        _ => None,
    }
}

/// Recomputes the derived data of page `id` from `doc`: its search entry,
/// body text, outgoing links and attachment references. A page whose node is
/// not in the index yet is skipped; it is indexed when the node appears.
pub(crate) fn index_page(conn: &Connection, id: NodeId, doc: &PageDoc) -> Result<()> {
    let Some(rid) = nodes::rid_of(conn, id)? else {
        return Ok(());
    };
    // A document that is not a readable page (an empty replica, a corrupt
    // root) indexes as an empty page instead of blocking every write.
    let summary = match doc.summary() {
        Ok(summary) => summary,
        Err(ne_core::Error::InvalidPage(_)) => Default::default(),
        Err(other) => return Err(Error::Core(other)),
    };
    let body_norm = search::fold(&summary.text);
    conn.prepare_cached(
        "INSERT INTO page_text (rid, body, body_norm, word_count) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(rid) DO UPDATE SET body = excluded.body, body_norm = excluded.body_norm,
                                        word_count = excluded.word_count",
    )?
    .execute(params![
        rid,
        summary.text,
        body_norm,
        i64::try_from(summary.word_count).unwrap_or(i64::MAX)
    ])?;
    let title_norm: String =
        conn.query_row("SELECT title_norm FROM nodes WHERE rid = ?1", [rid], |r| {
            r.get(0)
        })?;
    search::index_node(conn, rid, &title_norm, &body_norm)?;

    let source = id.to_string();
    conn.execute("DELETE FROM links WHERE source_id = ?1", [&source])?;
    let mut insert = conn.prepare_cached(
        "INSERT OR IGNORE INTO links (source_id, target_id, kind) VALUES (?1, ?2, ?3)",
    )?;
    for link in &summary.links {
        insert.execute(params![
            source,
            link.target.to_string(),
            kind_code(link.kind)
        ])?;
    }
    blobs::set_page_refs(conn, &source, &summary.attachments)
}

/// Refreshes the search entry of a node after its title changed, keeping the
/// stored body.
pub(crate) fn reindex_title(conn: &Connection, id: NodeId) -> Result<()> {
    let Some(rid) = nodes::rid_of(conn, id)? else {
        return Ok(());
    };
    let title_norm: String =
        conn.query_row("SELECT title_norm FROM nodes WHERE rid = ?1", [rid], |r| {
            r.get(0)
        })?;
    let body_norm = search::body_norm_of(conn, rid)?.unwrap_or_default();
    search::index_node(conn, rid, &title_norm, &body_norm)
}

/// Drops everything derived from a node that was permanently deleted.
pub(crate) fn remove_page(conn: &Connection, id: NodeId, rid: Option<i64>) -> Result<()> {
    if let Some(rid) = rid {
        search::remove_node(conn, rid)?;
        conn.execute("DELETE FROM page_text WHERE rid = ?1", [rid])?;
    }
    let source = id.to_string();
    conn.execute("DELETE FROM links WHERE source_id = ?1", [&source])?;
    blobs::clear_page_refs(conn, &source)
}

/// Empties every derived table (not the attachments themselves).
pub(crate) fn clear_all(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM search", [])?;
    conn.execute("DELETE FROM page_text", [])?;
    conn.execute("DELETE FROM links", [])?;
    conn.execute("DELETE FROM attachment_refs", [])?;
    conn.execute("UPDATE attachments SET ref_count = 0", [])?;
    Ok(())
}

/// Visible pages that link to `target`, sorted by title, excluding the target
/// itself and links that are only a child page block.
pub(crate) fn backlinks(conn: &Connection, target: NodeId) -> Result<Vec<Backlink>> {
    let mut statement = conn.prepare_cached(
        "SELECT l.source_id, n.title, group_concat(l.kind)
         FROM links l JOIN nodes n ON n.id = l.source_id
         WHERE l.target_id = ?1 AND l.source_id != l.target_id
               AND l.kind != 0 AND n.hidden = 0
         GROUP BY l.source_id",
    )?;
    let rows = statement.query_map([target.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (source, title, kinds) = row?;
        let source = source
            .parse()
            .map_err(|_| Error::Corrupt(format!("link source {source:?}")))?;
        let mut kinds: Vec<LinkKind> = kinds
            .split(',')
            .filter_map(|c| c.parse().ok().and_then(kind_from_code))
            .collect();
        kinds.sort();
        out.push(Backlink {
            source,
            title,
            kinds,
        });
    }
    // Alphabetical by title as a reader sees it, accents and case aside.
    out.sort_by_cached_key(|b| (search::fold(&b.title), b.source));
    Ok(out)
}
