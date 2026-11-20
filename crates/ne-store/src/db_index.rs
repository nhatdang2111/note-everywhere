//! The derived index of databases: one SQLite row per cell value, kept in step
//! with the database documents incrementally.
//!
//! A cell edit changes the cells of one row, so only that row's index rows are
//! replaced. A schema change (a property or option added, changed or removed)
//! can change every row's effective values, so the whole database is
//! re-indexed. Nothing here is the source of truth: `rebuild` recomputes all of
//! it from the documents.

use std::collections::BTreeMap;

use ne_core::NodeId;
use ne_core::collation::sort_key;
use ne_core::database::{CellValue, DatabaseDoc, DbChanges, Property, PropertyType};
use rusqlite::{Connection, params};

use crate::error::{Error, Result};
use crate::pages;
use crate::search::fold;

/// The text of a row's properties that search should find: text and web
/// address values, and the names of the options picked.
fn search_text(properties: &[Property], values: &BTreeMap<String, CellValue>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for property in properties {
        match values.get(&property.id) {
            Some(CellValue::Text(s) | CellValue::Url(s)) => parts.push(s.clone()),
            Some(CellValue::Select(id)) => {
                parts.extend(property.option(id).map(|o| o.name.clone()));
            }
            Some(CellValue::MultiSelect(ids)) => {
                parts.extend(
                    ids.iter()
                        .filter_map(|id| property.option(id))
                        .map(|o| o.name.clone()),
                );
            }
            _ => {}
        }
    }
    parts.join("\n")
}

fn rank(property: &Property, option: &str) -> f64 {
    property
        .options
        .iter()
        .position(|o| o.id == option)
        .map_or(f64::MAX, |i| i as f64)
}

/// Inserts the cell rows (and links) of one row's effective values.
fn insert_cells(
    conn: &Connection,
    db_id: &str,
    properties: &[Property],
    row: &str,
    values: &BTreeMap<String, CellValue>,
) -> Result<()> {
    let mut cell = conn.prepare_cached(
        "INSERT OR REPLACE INTO db_cells (db_id, prop_id, row_id, ord, num, txt, txt2, opt, skey)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut link = conn.prepare_cached(
        "INSERT OR IGNORE INTO db_links (prop_id, src_row, dst_row) VALUES (?1, ?2, ?3)",
    )?;
    for property in properties {
        let Some(value) = values.get(&property.id) else {
            continue;
        };
        let prop = property.id.as_str();
        match value {
            CellValue::Text(s) | CellValue::Url(s) => {
                cell.execute(params![
                    db_id,
                    prop,
                    row,
                    0,
                    None::<f64>,
                    fold(s),
                    None::<String>,
                    None::<String>,
                    sort_key(s)
                ])?;
            }
            CellValue::Number(n) => {
                cell.execute(params![
                    db_id,
                    prop,
                    row,
                    0,
                    n,
                    None::<String>,
                    None::<String>,
                    None::<String>,
                    None::<Vec<u8>>
                ])?;
            }
            CellValue::Checkbox(on) => {
                cell.execute(params![
                    db_id,
                    prop,
                    row,
                    0,
                    f64::from(u8::from(*on)),
                    None::<String>,
                    None::<String>,
                    None::<String>,
                    None::<Vec<u8>>
                ])?;
            }
            CellValue::Select(id) => {
                cell.execute(params![
                    db_id,
                    prop,
                    row,
                    0,
                    rank(property, id),
                    None::<String>,
                    None::<String>,
                    id,
                    None::<Vec<u8>>
                ])?;
            }
            CellValue::MultiSelect(ids) => {
                for (ord, id) in ids.iter().enumerate() {
                    let ord = i64::try_from(ord).unwrap_or(i64::MAX);
                    cell.execute(params![
                        db_id,
                        prop,
                        row,
                        ord,
                        rank(property, id),
                        None::<String>,
                        None::<String>,
                        id,
                        None::<Vec<u8>>
                    ])?;
                }
            }
            CellValue::Date(date) => {
                cell.execute(params![
                    db_id,
                    prop,
                    row,
                    0,
                    None::<f64>,
                    date.start,
                    date.end,
                    None::<String>,
                    None::<Vec<u8>>
                ])?;
            }
            CellValue::Relation(targets) => {
                if property.ty == PropertyType::Relation && !property.is_backlink() {
                    for target in targets {
                        link.execute(params![prop, row, target])?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn write_row_summary(
    conn: &Connection,
    db_id: &str,
    properties: &[Property],
    row: &str,
    values: &BTreeMap<String, CellValue>,
) -> Result<()> {
    if values.is_empty() {
        conn.prepare_cached("DELETE FROM db_rows WHERE row_id = ?1")?
            .execute([row])?;
        return Ok(());
    }
    let vals = serde_json::to_string(values)
        .map_err(|e| Error::Corrupt(format!("row values do not encode: {e}")))?;
    let raw = search_text(properties, values);
    conn.prepare_cached(
        "INSERT INTO db_rows (row_id, db_id, vals, search_raw, search_norm) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(row_id) DO UPDATE SET db_id = excluded.db_id, vals = excluded.vals,
             search_raw = excluded.search_raw, search_norm = excluded.search_norm",
    )?
    .execute(params![row, db_id, vals, raw, fold(&raw)])?;
    Ok(())
}

fn refresh_search(conn: &Connection, row: &str) -> Result<()> {
    if let Ok(id) = row.parse::<NodeId>() {
        pages::reindex_title(conn, id)?;
    }
    Ok(())
}

/// Replaces the index rows of one row.
fn index_row(
    conn: &Connection,
    db_id: &str,
    properties: &[Property],
    row: &str,
    values: &BTreeMap<String, CellValue>,
) -> Result<()> {
    conn.prepare_cached("DELETE FROM db_cells WHERE row_id = ?1 AND db_id = ?2")?
        .execute([row, db_id])?;
    let mut unlink =
        conn.prepare_cached("DELETE FROM db_links WHERE src_row = ?1 AND prop_id = ?2")?;
    for property in properties.iter().filter(|p| p.ty == PropertyType::Relation) {
        unlink.execute([row, property.id.as_str()])?;
    }
    insert_cells(conn, db_id, properties, row, values)?;
    write_row_summary(conn, db_id, properties, row, values)?;
    refresh_search(conn, row)
}

/// Brings the index of database `db_id` in line with `doc` after `changes`.
pub(crate) fn apply_changes(
    conn: &Connection,
    db_id: NodeId,
    doc: &DatabaseDoc,
    changes: &DbChanges,
) -> Result<()> {
    if changes.is_empty() || (changes.rows.is_empty() && !changes.schema) {
        return Ok(());
    }
    if changes.schema {
        return rebuild(conn, db_id, doc);
    }
    let key = db_id.to_string();
    let properties = doc.properties()?;
    for row in &changes.rows {
        let values = doc.row_values(row, &properties);
        index_row(conn, &key, &properties, row, &values)?;
    }
    Ok(())
}

/// Recomputes the whole index of one database from its document.
pub(crate) fn rebuild(conn: &Connection, db_id: NodeId, doc: &DatabaseDoc) -> Result<()> {
    let key = db_id.to_string();
    let properties = doc.properties()?;
    let previous: Vec<String> = conn
        .prepare_cached("SELECT row_id FROM db_rows WHERE db_id = ?1")?
        .query_map([&key], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    conn.execute(
        "DELETE FROM db_links WHERE src_row IN (SELECT row_id FROM db_rows WHERE db_id = ?1)",
        [&key],
    )?;
    conn.execute("DELETE FROM db_cells WHERE db_id = ?1", [&key])?;
    conn.execute("DELETE FROM db_rows WHERE db_id = ?1", [&key])?;
    let rows = doc.all_row_values(&properties);
    for (row, values) in &rows {
        insert_cells(conn, &key, &properties, row, values)?;
        write_row_summary(conn, &key, &properties, row, values)?;
    }
    let mut touched: Vec<&String> = previous.iter().chain(rows.keys()).collect();
    touched.sort();
    touched.dedup();
    for row in touched {
        refresh_search(conn, row)?;
    }
    Ok(())
}

/// Drops everything indexed for a database that was deleted.
pub(crate) fn remove_database(conn: &Connection, db_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM db_links WHERE src_row IN (SELECT row_id FROM db_rows WHERE db_id = ?1)",
        [db_id],
    )?;
    conn.execute("DELETE FROM db_cells WHERE db_id = ?1", [db_id])?;
    conn.execute("DELETE FROM db_rows WHERE db_id = ?1", [db_id])?;
    Ok(())
}

/// Drops everything indexed for a row page that was deleted.
pub(crate) fn remove_row(conn: &Connection, row_id: &str) -> Result<()> {
    conn.prepare_cached("DELETE FROM db_cells WHERE row_id = ?1")?
        .execute([row_id])?;
    conn.prepare_cached("DELETE FROM db_links WHERE src_row = ?1")?
        .execute([row_id])?;
    conn.prepare_cached("DELETE FROM db_rows WHERE row_id = ?1")?
        .execute([row_id])?;
    Ok(())
}

/// The database whose cells hold values for this row, if any.
pub(crate) fn row_database(conn: &Connection, row_id: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .prepare_cached("SELECT db_id FROM db_rows WHERE row_id = ?1")?
        .query_row([row_id], |row| row.get(0))
        .optional()?)
}
