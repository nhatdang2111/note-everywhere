//! The query engine of databases: a filter, sort, group and page request turned
//! into SQL over the derived index (`db_cells`, `db_links`, `db_rows`) joined
//! with `nodes`, which supplies the title, the manual order (sibling position)
//! and visibility (trashed rows are hidden).
//!
//! A row exists for a query when it is a visible page child of the database.
//! A cell that is not stored is empty: `is_empty` is "no index row", an
//! unchecked box is "no index row or 0".

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ne_core::database::{
    Bucket, CellValue, DEFAULT_LIMIT, FilterNode, Logic, MAX_LIMIT, Operator, Property,
    PropertyType, QueryResult, QueryRow, QuerySpec, SortKey, TITLE_PROPERTY,
};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::error::{Error, Result};
use crate::search::fold;

struct Sql<'a> {
    properties: &'a [Property],
    params: Vec<Value>,
}

impl Sql<'_> {
    fn param(&mut self, value: impl Into<Value>) -> usize {
        self.params.push(value.into());
        self.params.len()
    }

    fn property(&self, id: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.id == id)
    }

    fn filter(&mut self, node: &FilterNode) -> String {
        match node {
            FilterNode::Group { logic, children } => {
                let parts: Vec<String> = children.iter().map(|c| self.filter(c)).collect();
                if parts.is_empty() {
                    return "1".into();
                }
                let glue = match logic {
                    Logic::And => " AND ",
                    Logic::Or => " OR ",
                };
                format!("({})", parts.join(glue))
            }
            FilterNode::Condition {
                property,
                operator,
                value,
            } => self.condition(property, *operator, value.as_ref()),
        }
    }

    /// `EXISTS` over the cell rows of one property of the current row.
    fn cell(&mut self, property: &str, extra: &str) -> String {
        let p = self.param(property.to_owned());
        format!(
            "EXISTS (SELECT 1 FROM db_cells c WHERE c.db_id = ?1 AND c.prop_id = ?{p} \
             AND c.row_id = n.id{extra})"
        )
    }

    fn condition(
        &mut self,
        property: &str,
        op: Operator,
        value: Option<&serde_json::Value>,
    ) -> String {
        use Operator::*;
        // A condition that no longer applies (deleted property, a type change
        // that made the operator meaningless) matches everything.
        let always = "1".to_owned();
        let text = value.and_then(|v| v.as_str()).map(str::to_owned);
        let number = value.and_then(serde_json::Value::as_f64);
        if property == TITLE_PROPERTY {
            return self.text_condition("n.title_norm", op, text.as_deref(), None);
        }
        let Some(prop) = self.property(property).cloned() else {
            return always;
        };
        if !ne_core::database::operators_for(Some(prop.ty)).contains(&op) {
            return always;
        }
        let id = prop.id.as_str();
        match prop.ty {
            PropertyType::Text | PropertyType::Url => {
                self.text_condition("c.txt", op, text.as_deref(), Some(id))
            }
            PropertyType::Number => {
                let n = match (op.is_unary(), number) {
                    (true, _) => 0.0,
                    (false, Some(n)) => n,
                    (false, None) => return always,
                };
                let cmp = |sql: &mut Self, symbol: &str| {
                    let v = sql.param(n);
                    sql.cell(id, &format!(" AND c.num {symbol} ?{v}"))
                };
                match op {
                    Is => cmp(self, "="),
                    IsNot => format!("NOT {}", cmp(self, "=")),
                    Gt => cmp(self, ">"),
                    Gte => cmp(self, ">="),
                    Lt => cmp(self, "<"),
                    Lte => cmp(self, "<="),
                    IsEmpty => format!("NOT {}", self.cell(id, "")),
                    _ => self.cell(id, ""),
                }
            }
            PropertyType::Select | PropertyType::MultiSelect => {
                let Some(option) = text else {
                    return match op {
                        IsEmpty => format!("NOT {}", self.cell(id, "")),
                        IsNotEmpty => self.cell(id, ""),
                        _ => always,
                    };
                };
                let v = self.param(option);
                let has = self.cell(id, &format!(" AND c.opt = ?{v}"));
                match op {
                    Is | Contains => has,
                    IsNot | NotContains => format!("NOT {has}"),
                    IsEmpty => format!("NOT {}", self.cell(id, "")),
                    _ => self.cell(id, ""),
                }
            }
            PropertyType::Date => self.date_condition(id, op, text),
            PropertyType::Checkbox => {
                let checked = self.cell(id, " AND c.num = 1");
                match op {
                    IsChecked => checked,
                    _ => format!("NOT {checked}"),
                }
            }
            PropertyType::Relation => self.relation_condition(&prop, op, text),
        }
    }

    fn text_condition(
        &mut self,
        column: &str,
        op: Operator,
        value: Option<&str>,
        prop: Option<&str>,
    ) -> String {
        use Operator::*;
        let wrap = |sql: &mut Self, test: &str| -> String {
            match prop {
                Some(id) => sql.cell(id, &format!(" AND {test}")),
                None => test.to_owned(),
            }
        };
        if matches!(op, IsEmpty | IsNotEmpty) {
            let empty = match prop {
                Some(id) => format!("NOT {}", self.cell(id, "")),
                None => "n.title = ''".to_owned(),
            };
            return if op == IsEmpty {
                empty
            } else {
                format!("NOT ({empty})")
            };
        }
        let Some(value) = value else {
            return "1".into();
        };
        let folded = fold(value);
        let len = i64::try_from(folded.chars().count()).unwrap_or(i64::MAX);
        let v = self.param(folded);
        let positive = |sql: &mut Self, op: Operator| match op {
            Contains | NotContains => wrap(sql, &format!("instr({column}, ?{v}) > 0")),
            Is | IsNot => wrap(sql, &format!("{column} = ?{v}")),
            StartsWith => {
                let l = sql.param(len);
                wrap(sql, &format!("substr({column}, 1, ?{l}) = ?{v}"))
            }
            EndsWith => {
                let l = sql.param(len);
                wrap(
                    sql,
                    &format!("(?{l} > 0 AND substr({column}, -?{l}) = ?{v})"),
                )
            }
            _ => "1".into(),
        };
        let sql = positive(self, op);
        match op {
            NotContains | IsNot => format!("NOT ({sql})"),
            _ => sql,
        }
    }

    fn date_condition(&mut self, id: &str, op: Operator, day: Option<String>) -> String {
        use Operator::*;
        if matches!(op, IsEmpty | IsNotEmpty) {
            let any = self.cell(id, "");
            return if op == IsEmpty {
                format!("NOT {any}")
            } else {
                any
            };
        }
        let Some(day) = day else {
            return "1".into();
        };
        let v = self.param(day);
        let start = "substr(c.txt, 1, 10)";
        let test = match op {
            // A range matches every day it covers.
            Is => format!(
                "({start} = ?{v} OR (c.txt2 IS NOT NULL AND {start} <= ?{v} \
                 AND substr(c.txt2, 1, 10) >= ?{v}))"
            ),
            Before => format!("{start} < ?{v}"),
            After => format!("{start} > ?{v}"),
            OnOrBefore => format!("{start} <= ?{v}"),
            _ => format!("{start} >= ?{v}"),
        };
        self.cell(id, &format!(" AND {test}"))
    }

    fn relation_condition(
        &mut self,
        prop: &Property,
        op: Operator,
        target: Option<String>,
    ) -> String {
        use Operator::*;
        // A backlink reads the links of the property it mirrors from the other
        // end. Links to or from rows that are trashed or gone do not count.
        let (forward, mine, other) =
            match prop.relation.as_ref().and_then(|r| r.backlink_of.clone()) {
                Some(forward) => (forward, "dst_row", "src_row"),
                None => (prop.id.clone(), "src_row", "dst_row"),
            };
        let p = self.param(forward);
        let mut exists = format!(
            "EXISTS (SELECT 1 FROM db_links l JOIN nodes t ON t.id = l.{other} AND t.hidden = 0 \
             WHERE l.prop_id = ?{p} AND l.{mine} = n.id"
        );
        if matches!(op, Contains | NotContains) {
            let Some(target) = target else {
                return "1".into();
            };
            let v = self.param(target);
            exists.push_str(&format!(" AND l.{other} = ?{v}"));
        }
        exists.push(')');
        match op {
            Contains | IsNotEmpty => exists,
            _ => format!("NOT {exists}"),
        }
    }

    /// The `LEFT JOIN` and `ORDER BY` term of one sort key, if it applies.
    fn sort(&mut self, index: usize, key: &SortKey) -> Option<(String, String)> {
        let direction = if key.descending { "DESC" } else { "ASC" };
        if key.property == TITLE_PROPERTY {
            return Some((String::new(), format!("n.title_key {direction}")));
        }
        let prop = self.property(&key.property)?.clone();
        let (column, nulls_last) = match prop.ty {
            PropertyType::Text | PropertyType::Url => ("skey", true),
            PropertyType::Number | PropertyType::Select | PropertyType::MultiSelect => {
                ("num", true)
            }
            PropertyType::Date => ("txt", true),
            PropertyType::Checkbox => ("num", false),
            PropertyType::Relation => return None,
        };
        let p = self.param(prop.id);
        let join = format!(
            " LEFT JOIN db_cells s{index} ON s{index}.db_id = ?1 AND s{index}.prop_id = ?{p} \
             AND s{index}.row_id = n.id AND s{index}.ord = 0"
        );
        let term = if nulls_last {
            format!("s{index}.{column} IS NULL, s{index}.{column} {direction}")
        } else {
            format!("COALESCE(s{index}.{column}, 0) {direction}")
        };
        Some((join, term))
    }
}

/// Everything a query needs besides the connection.
pub(crate) struct Request<'a> {
    pub db_id: &'a str,
    pub properties: &'a [Property],
    pub spec: &'a QuerySpec,
}

fn row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<(QueryRow, Option<String>)> {
    Ok((
        QueryRow {
            id: row.get(0)?,
            title: row.get(1)?,
            icon: row.get(2)?,
            values: BTreeMap::new(),
        },
        row.get(3)?,
    ))
}

fn parse_values(json: Option<String>) -> BTreeMap<String, CellValue> {
    json.and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

const ROW_COLUMNS: &str = "n.id, n.title, n.icon, r.vals";
const ROW_JOIN: &str = " LEFT JOIN db_rows r ON r.row_id = n.id AND r.db_id = ?1";

/// Runs a query over a database.
pub(crate) fn run(conn: &Connection, request: &Request<'_>) -> Result<QueryResult> {
    let spec = request.spec;
    let mut sql = Sql {
        properties: request.properties,
        params: vec![Value::from(request.db_id.to_owned())],
    };
    let mut conditions = vec![
        "n.parent_id = ?1".to_owned(),
        "n.hidden = 0".to_owned(),
        "n.kind = 'page'".to_owned(),
    ];
    if let Some(filter) = &spec.filter {
        conditions.push(sql.filter(filter));
    }
    if let (Some(range), Some(prop)) = (&spec.date_range, spec.date_property.as_deref()) {
        if sql
            .property(prop)
            .is_some_and(|p| p.ty == PropertyType::Date)
        {
            let to = sql.param(range.to.clone());
            let from = sql.param(range.from.clone());
            conditions.push(sql.cell(
                prop,
                &format!(
                    " AND substr(c.txt, 1, 10) <= ?{to} \
                     AND substr(COALESCE(c.txt2, c.txt), 1, 10) >= ?{from}"
                ),
            ));
        } else {
            return Err(Error::InvalidInput(
                "a calendar needs a date property".into(),
            ));
        }
    }
    let where_sql = conditions.join(" AND ");
    let filter_params = sql.params.clone();
    let limit = if spec.limit == 0 {
        DEFAULT_LIMIT
    } else {
        spec.limit.min(MAX_LIMIT)
    };

    let total: i64 = conn
        .prepare_cached(&format!("SELECT COUNT(*) FROM nodes n WHERE {where_sql}"))?
        .query_row(params_from_iter(filter_params.iter()), |row| row.get(0))?;
    let mut result = QueryResult {
        total: usize::try_from(total).unwrap_or_default(),
        ..QueryResult::default()
    };

    let mut joins = String::new();
    let mut order: Vec<String> = Vec::new();
    for (index, key) in spec.sorts.iter().enumerate() {
        if let Some((join, term)) = sql.sort(index, key) {
            joins.push_str(&join);
            order.push(term);
        }
    }
    order.push("n.position ASC".into());
    order.push("n.id ASC".into());
    let order_sql = order.join(", ");

    let group = spec
        .group
        .as_ref()
        .and_then(|g| sql.property(&g.property).cloned().map(|p| (g, p)))
        .filter(|(_, p)| {
            matches!(
                p.ty,
                PropertyType::Select | PropertyType::MultiSelect | PropertyType::Checkbox
            )
        });
    let mut related: BTreeSet<String> = BTreeSet::new();
    match group {
        None => {
            let (o, l) = (
                sql.param(i64::try_from(spec.offset).unwrap_or(i64::MAX)),
                sql.param(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
            let statement = format!(
                "SELECT {ROW_COLUMNS} FROM nodes n{ROW_JOIN}{joins} WHERE {where_sql} \
                 ORDER BY {order_sql} LIMIT ?{l} OFFSET ?{o}"
            );
            let rows: Vec<(QueryRow, Option<String>)> = conn
                .prepare_cached(&statement)?
                .query_map(params_from_iter(sql.params.iter()), row_from_sql)?
                .collect::<rusqlite::Result<_>>()?;
            result.rows = rows
                .into_iter()
                .map(|(mut row, vals)| {
                    row.values = parse_values(vals);
                    row
                })
                .collect();
            fill_backlinks(conn, request.properties, &mut result.rows)?;
            for row in &mut result.rows {
                collect_related(&row.values, &mut related);
            }
        }
        Some((config, property)) => {
            let g = sql.param(property.id.clone());
            let statement = format!(
                "SELECT n.id, g.opt, g.num FROM nodes n{joins} \
                 LEFT JOIN db_cells g ON g.db_id = ?1 AND g.prop_id = ?{g} AND g.row_id = n.id \
                 WHERE {where_sql} ORDER BY {order_sql}"
            );
            type Member = (String, Option<String>, Option<f64>);
            let members: Vec<Member> = conn
                .prepare_cached(&statement)?
                .query_map(params_from_iter(sql.params.iter()), |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            result.groups = buckets(config.hide_empty, &property, members, limit);
            let ids: Vec<String> = result
                .groups
                .iter()
                .flat_map(|b| b.rows.iter().map(|r| r.id.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let mut loaded = load_rows(conn, request.db_id, &ids)?;
            fill_backlinks_map(conn, request.properties, &mut loaded)?;
            for bucket in &mut result.groups {
                for row in &mut bucket.rows {
                    if let Some(full) = loaded.get(&row.id) {
                        *row = full.clone();
                    }
                    collect_related(&row.values, &mut related);
                }
            }
        }
    }
    result.titles = visible_titles(conn, &related)?;
    let titles = &result.titles;
    let strip = |row: &mut QueryRow| {
        for value in row.values.values_mut() {
            if let CellValue::Relation(ids) = value {
                ids.retain(|id| titles.contains_key(id));
            }
        }
        row.values
            .retain(|_, v| !matches!(v, CellValue::Relation(ids) if ids.is_empty()));
    };
    result.rows.iter_mut().for_each(strip);
    result
        .groups
        .iter_mut()
        .flat_map(|b| b.rows.iter_mut())
        .for_each(strip);
    Ok(result)
}

fn buckets(
    hide_empty: bool,
    property: &Property,
    members: Vec<(String, Option<String>, Option<f64>)>,
    limit: usize,
) -> Vec<Bucket> {
    let mut keys: Vec<Option<String>> = match property.ty {
        PropertyType::Checkbox => vec![Some("false".into()), Some("true".into())],
        _ => property
            .options
            .iter()
            .map(|o| Some(o.id.clone()))
            .collect(),
    };
    keys.push(None);
    let mut by_key: HashMap<Option<String>, (usize, Vec<String>)> = HashMap::new();
    for (row, opt, num) in members {
        let key = match property.ty {
            PropertyType::Checkbox => {
                Some(if num == Some(1.0) { "true" } else { "false" }.to_owned())
            }
            _ => opt.filter(|o| property.option(o).is_some()),
        };
        let entry = by_key.entry(key).or_default();
        entry.0 += 1;
        if entry.1.len() < limit {
            entry.1.push(row);
        }
    }
    keys.into_iter()
        .filter_map(|key| {
            let (count, ids) = by_key.remove(&key).unwrap_or_default();
            if hide_empty && count == 0 {
                return None;
            }
            // The rows without a value get a group only when there are some.
            if key.is_none() && count == 0 {
                return None;
            }
            Some(Bucket {
                key,
                count,
                rows: ids
                    .into_iter()
                    .map(|id| QueryRow {
                        id,
                        title: String::new(),
                        icon: None,
                        values: BTreeMap::new(),
                    })
                    .collect(),
            })
        })
        .collect()
}

fn load_rows(conn: &Connection, db_id: &str, ids: &[String]) -> Result<HashMap<String, QueryRow>> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(500) {
        let marks = (0..chunk.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let statement =
            format!("SELECT {ROW_COLUMNS} FROM nodes n{ROW_JOIN} WHERE n.id IN ({marks})");
        let mut params: Vec<Value> = vec![db_id.to_owned().into()];
        params.extend(chunk.iter().cloned().map(Value::from));
        let rows = conn
            .prepare(&statement)?
            .query_map(params_from_iter(params.iter()), row_from_sql)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (mut row, vals) in rows {
            row.values = parse_values(vals);
            out.insert(row.id.clone(), row);
        }
    }
    Ok(out)
}

/// Adds the computed values of backlink properties to rows.
fn fill_backlinks(conn: &Connection, properties: &[Property], rows: &mut [QueryRow]) -> Result<()> {
    let mut map: HashMap<String, QueryRow> = HashMap::new();
    if properties.iter().all(|p| !p.is_backlink()) || rows.is_empty() {
        return Ok(());
    }
    for row in rows.iter() {
        map.insert(row.id.clone(), row.clone());
    }
    fill_backlinks_map(conn, properties, &mut map)?;
    for row in rows.iter_mut() {
        if let Some(full) = map.remove(&row.id) {
            *row = full;
        }
    }
    Ok(())
}

fn fill_backlinks_map(
    conn: &Connection,
    properties: &[Property],
    rows: &mut HashMap<String, QueryRow>,
) -> Result<()> {
    let ids: Vec<String> = rows.keys().cloned().collect();
    for property in properties.iter().filter(|p| p.is_backlink()) {
        let Some(forward) = property
            .relation
            .as_ref()
            .and_then(|r| r.backlink_of.clone())
        else {
            continue;
        };
        for chunk in ids.chunks(500) {
            let marks = (0..chunk.len())
                .map(|i| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(",");
            let statement = format!(
                "SELECT l.dst_row, l.src_row FROM db_links l \
                 JOIN nodes s ON s.id = l.src_row AND s.hidden = 0 \
                 WHERE l.prop_id = ?1 AND l.dst_row IN ({marks}) ORDER BY l.src_row"
            );
            let mut params: Vec<Value> = vec![forward.clone().into()];
            params.extend(chunk.iter().cloned().map(Value::from));
            let links = conn
                .prepare(&statement)?
                .query_map(params_from_iter(params.iter()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (dst, src) in links {
                grouped.entry(dst).or_default().push(src);
            }
            for (dst, sources) in grouped {
                if let Some(row) = rows.get_mut(&dst) {
                    row.values
                        .insert(property.id.clone(), CellValue::Relation(sources));
                }
            }
        }
    }
    Ok(())
}

fn collect_related(values: &BTreeMap<String, CellValue>, into: &mut BTreeSet<String>) {
    for value in values.values() {
        if let CellValue::Relation(ids) = value {
            into.extend(ids.iter().cloned());
        }
    }
}

fn visible_titles(conn: &Connection, ids: &BTreeSet<String>) -> Result<BTreeMap<String, String>> {
    let ids: Vec<&String> = ids.iter().collect();
    let mut out = BTreeMap::new();
    for chunk in ids.chunks(500) {
        let marks = (0..chunk.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let statement = format!("SELECT id, title FROM nodes WHERE hidden = 0 AND id IN ({marks})");
        let rows = conn
            .prepare(&statement)?
            .query_map(params_from_iter(chunk.iter().copied()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out.extend(rows);
    }
    Ok(out)
}
