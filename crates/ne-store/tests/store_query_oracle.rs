//! Random databases, filters and sorts: the SQL engine must agree with a plain
//! in-memory evaluation of the same request over the same values.

mod store_support;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use ne_core::collation::sort_key;
use ne_core::database::{
    CellValue, DateValue, FilterNode, Logic, Operator, PropertyType, QuerySpec, SortKey,
};
use ne_core::tree::Position;
use ne_store::fold;
use proptest::prelude::*;
use serde_json::{Value, json};
use store_support::new_workspace;

const WORDS: [&str; 5] = ["đường đi", "Đường", "ghi chú", "Ghi chu", "xyz"];
const DAYS: [&str; 4] = ["2026-11-19", "2026-11-20", "2026-11-21T09:00", "2026-11-22"];

#[derive(Debug, Clone)]
struct RowSpec {
    text: Option<usize>,
    number: Option<i32>,
    select: Option<usize>,
    multi: Vec<usize>,
    day: Option<usize>,
    ticked: Option<bool>,
}

fn row_spec() -> impl Strategy<Value = RowSpec> {
    (
        prop::option::of(0..WORDS.len()),
        prop::option::of(-3..4i32),
        prop::option::of(0..3usize),
        prop::collection::vec(0..3usize, 0..3),
        prop::option::of(0..DAYS.len()),
        prop::option::of(any::<bool>()),
    )
        .prop_map(|(text, number, select, mut multi, day, ticked)| {
            multi.sort_unstable();
            multi.dedup();
            RowSpec {
                text,
                number,
                select,
                multi,
                day,
                ticked,
            }
        })
}

#[derive(Debug, Clone, Copy)]
enum Prop {
    Title,
    Text,
    Number,
    Select,
    Multi,
    Date,
    Check,
}

fn condition() -> impl Strategy<Value = (Prop, Operator, Value)> {
    use Operator::*;
    let words =
        || (0..WORDS.len() + 1).prop_map(|i| json!(WORDS.get(i).copied().unwrap_or("duong")));
    let numbers = || (-4..5i32).prop_map(|n| json!(n));
    let options = || (0..3usize).prop_map(|i| json!(i));
    let days = || {
        (0..5usize).prop_map(|i| {
            json!(
                [
                    "2026-11-19",
                    "2026-11-20",
                    "2026-11-21",
                    "2026-11-22",
                    "2026-11-23"
                ][i]
            )
        })
    };
    prop_oneof![
        (
            prop::sample::select(vec![
                Contains,
                NotContains,
                Is,
                IsNot,
                StartsWith,
                EndsWith,
                IsEmpty,
                IsNotEmpty
            ]),
            words()
        )
            .prop_map(|(op, v)| (Prop::Text, op, v)),
        (
            prop::sample::select(vec![
                Contains,
                NotContains,
                Is,
                IsNot,
                StartsWith,
                EndsWith,
                IsEmpty,
                IsNotEmpty
            ]),
            words()
        )
            .prop_map(|(op, v)| (Prop::Title, op, v)),
        (
            prop::sample::select(vec![Is, IsNot, Gt, Gte, Lt, Lte, IsEmpty, IsNotEmpty]),
            numbers()
        )
            .prop_map(|(op, v)| (Prop::Number, op, v)),
        (
            prop::sample::select(vec![Is, IsNot, IsEmpty, IsNotEmpty]),
            options()
        )
            .prop_map(|(op, v)| (Prop::Select, op, v)),
        (
            prop::sample::select(vec![Contains, NotContains, IsEmpty, IsNotEmpty]),
            options()
        )
            .prop_map(|(op, v)| (Prop::Multi, op, v)),
        (
            prop::sample::select(vec![
                Is, Before, After, OnOrBefore, OnOrAfter, IsEmpty, IsNotEmpty
            ]),
            days()
        )
            .prop_map(|(op, v)| (Prop::Date, op, v)),
        prop::sample::select(vec![IsChecked, IsUnchecked]).prop_map(|op| (
            Prop::Check,
            op,
            Value::Null
        )),
    ]
}

#[derive(Debug, Clone)]
enum Tree {
    Leaf((Prop, Operator, Value)),
    Node(bool, Vec<Tree>),
}

fn tree() -> impl Strategy<Value = Tree> {
    condition()
        .prop_map(Tree::Leaf)
        .prop_recursive(2, 12, 3, |inner| {
            (any::<bool>(), prop::collection::vec(inner, 0..4))
                .prop_map(|(and, children)| Tree::Node(and, children))
        })
}

struct Ids {
    text: String,
    number: String,
    select: String,
    multi: String,
    date: String,
    check: String,
    options: Vec<String>,
    tags: Vec<String>,
}

impl Ids {
    fn property(&self, p: Prop) -> String {
        match p {
            Prop::Title => "title".into(),
            Prop::Text => self.text.clone(),
            Prop::Number => self.number.clone(),
            Prop::Select => self.select.clone(),
            Prop::Multi => self.multi.clone(),
            Prop::Date => self.date.clone(),
            Prop::Check => self.check.clone(),
        }
    }

    fn node(&self, t: &Tree) -> FilterNode {
        match t {
            Tree::Leaf((prop, operator, value)) => {
                let value = match (prop, value) {
                    (Prop::Select, Value::Number(n)) => {
                        json!(self.options[usize::try_from(n.as_u64().unwrap_or(0)).unwrap_or(0)])
                    }
                    (Prop::Multi, Value::Number(n)) => {
                        json!(self.tags[usize::try_from(n.as_u64().unwrap_or(0)).unwrap_or(0)])
                    }
                    (_, v) => v.clone(),
                };
                FilterNode::Condition {
                    property: self.property(*prop),
                    operator: *operator,
                    value: (!value.is_null())
                        .then_some(value)
                        .filter(|_| !operator.is_unary()),
                }
            }
            Tree::Node(and, children) => FilterNode::Group {
                logic: if *and { Logic::And } else { Logic::Or },
                children: children.iter().map(|c| self.node(c)).collect(),
            },
        }
    }
}

fn day(i: usize) -> &'static str {
    &DAYS[i][..10]
}

fn text_test(op: Operator, value: Option<&str>, arg: &str) -> bool {
    let arg = fold(arg);
    let present = value.map(fold);
    let has = |f: &dyn Fn(&str) -> bool| present.as_deref().is_some_and(f);
    match op {
        Operator::Contains => has(&|v| v.contains(&arg)),
        Operator::NotContains => !has(&|v| v.contains(&arg)),
        Operator::Is => has(&|v| v == arg),
        Operator::IsNot => !has(&|v| v == arg),
        Operator::StartsWith => has(&|v| v.starts_with(&arg)),
        Operator::EndsWith => !arg.is_empty() && has(&|v| v.ends_with(&arg)),
        Operator::IsEmpty => present.is_none(),
        Operator::IsNotEmpty => present.is_some(),
        _ => unreachable!(),
    }
}

fn holds(row: &RowSpec, title: &str, (prop, op, value): &(Prop, Operator, Value)) -> bool {
    use Operator::*;
    let arg_text = value.as_str().unwrap_or_default();
    let arg_index = value.as_u64().map(|n| usize::try_from(n).unwrap_or(0));
    match prop {
        Prop::Title => {
            if matches!(op, IsEmpty | IsNotEmpty) {
                return (*op == IsEmpty) == title.is_empty();
            }
            text_test(*op, Some(title), arg_text)
        }
        Prop::Text => text_test(*op, row.text.map(|i| WORDS[i]), arg_text),
        Prop::Number => {
            let arg = value.as_i64().unwrap_or(0);
            let v = row.number.map(i64::from);
            match op {
                Is => v == Some(arg),
                IsNot => v != Some(arg),
                Gt => v.is_some_and(|v| v > arg),
                Gte => v.is_some_and(|v| v >= arg),
                Lt => v.is_some_and(|v| v < arg),
                Lte => v.is_some_and(|v| v <= arg),
                IsEmpty => v.is_none(),
                _ => v.is_some(),
            }
        }
        Prop::Select => match op {
            Is => row.select == arg_index,
            IsNot => row.select != arg_index,
            IsEmpty => row.select.is_none(),
            _ => row.select.is_some(),
        },
        Prop::Multi => {
            let has = arg_index.is_some_and(|i| row.multi.contains(&i));
            match op {
                Contains => has,
                NotContains => !has,
                IsEmpty => row.multi.is_empty(),
                _ => !row.multi.is_empty(),
            }
        }
        Prop::Date => {
            let d = row.day.map(day);
            let arg = arg_text;
            match op {
                Is => d == Some(arg),
                Before => d.is_some_and(|d| d < arg),
                After => d.is_some_and(|d| d > arg),
                OnOrBefore => d.is_some_and(|d| d <= arg),
                OnOrAfter => d.is_some_and(|d| d >= arg),
                IsEmpty => d.is_none(),
                _ => d.is_some(),
            }
        }
        Prop::Check => match op {
            IsChecked => row.ticked == Some(true),
            _ => row.ticked != Some(true),
        },
    }
}

fn matches(row: &RowSpec, title: &str, t: &Tree) -> bool {
    match t {
        Tree::Leaf(c) => holds(row, title, c),
        Tree::Node(and, children) if children.is_empty() => {
            let _ = and;
            true
        }
        Tree::Node(true, children) => children.iter().all(|c| matches(row, title, c)),
        Tree::Node(false, children) => children.iter().any(|c| matches(row, title, c)),
    }
}

#[derive(Debug, Clone, Copy)]
enum SortBy {
    Title,
    Number,
    Select,
    Date,
    Check,
}

fn sort_compare(
    a: (&RowSpec, &str),
    b: (&RowSpec, &str),
    by: SortBy,
    descending: bool,
) -> Ordering {
    let directed = |o: Ordering| if descending { o.reverse() } else { o };
    // Missing values come last whichever the direction.
    let missing_last = |x: Option<()>, y: Option<()>| match (x, y) {
        (None, Some(())) => Some(Ordering::Greater),
        (Some(()), None) => Some(Ordering::Less),
        (None, None) => Some(Ordering::Equal),
        _ => None,
    };
    match by {
        SortBy::Title => directed(sort_key(a.1).cmp(&sort_key(b.1))),
        SortBy::Number => missing_last(a.0.number.map(|_| ()), b.0.number.map(|_| ()))
            .unwrap_or_else(|| directed(a.0.number.cmp(&b.0.number))),
        SortBy::Select => missing_last(a.0.select.map(|_| ()), b.0.select.map(|_| ()))
            .unwrap_or_else(|| directed(a.0.select.cmp(&b.0.select))),
        SortBy::Date => missing_last(a.0.day.map(|_| ()), b.0.day.map(|_| ()))
            .unwrap_or_else(|| directed(a.0.day.map(|i| DAYS[i]).cmp(&b.0.day.map(|i| DAYS[i])))),
        SortBy::Check => directed((a.0.ticked == Some(true)).cmp(&(b.0.ticked == Some(true)))),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn the_engine_agrees_with_a_plain_evaluation(
        rows in prop::collection::vec(row_spec(), 1..14),
        filter in tree(),
        sorts in prop::collection::vec((prop::sample::select(vec![SortBy::Title, SortBy::Number, SortBy::Select, SortBy::Date, SortBy::Check]), any::<bool>()), 0..3),
    ) {
        let (_dir, mut ws) = new_workspace();
        let db = ws.create_database(None, Position::End, "DB").expect("db");
        let mut add = |name: &str, ty| ws.database_edit(db, |d| d.add_property(name, ty, None)).expect("property").0;
        let text = add("Text", PropertyType::Text);
        let number = add("Number", PropertyType::Number);
        let select = add("Select", PropertyType::Select);
        let multi = add("Multi", PropertyType::MultiSelect);
        let date = add("Date", PropertyType::Date);
        let check = add("Check", PropertyType::Checkbox);
        let mut options = Vec::new();
        let mut tags = Vec::new();
        for i in 0..3 {
            options.push(ws.database_edit(db, |d| d.add_option(&select, &format!("opt{i}"), None)).expect("option").0);
            tags.push(ws.database_edit(db, |d| d.add_option(&multi, &format!("tag{i}"), None)).expect("tag").0);
        }
        let ids = Ids { text, number, select, multi, date, check, options, tags };
        let titles: Vec<String> = (0..rows.len()).map(|i| format!("{} {i}", ["Ân", "An", "Đức", "Bình"][i % 4])).collect();
        for (spec, title) in rows.iter().zip(&titles) {
            let mut values: BTreeMap<String, CellValue> = BTreeMap::new();
            if let Some(i) = spec.text { values.insert(ids.text.clone(), CellValue::Text(WORDS[i].into())); }
            if let Some(n) = spec.number { values.insert(ids.number.clone(), CellValue::Number(f64::from(n))); }
            if let Some(i) = spec.select { values.insert(ids.select.clone(), CellValue::Select(ids.options[i].clone())); }
            if !spec.multi.is_empty() { values.insert(ids.multi.clone(), CellValue::MultiSelect(spec.multi.iter().map(|i| ids.tags[*i].clone()).collect())); }
            if let Some(i) = spec.day { values.insert(ids.date.clone(), CellValue::Date(DateValue::new(DAYS[i]))); }
            if let Some(t) = spec.ticked { values.insert(ids.check.clone(), CellValue::Checkbox(t)); }
            ws.database_create_row(db, title, Position::End, &values).expect("row");
        }
        let sort_keys: Vec<SortKey> = sorts
            .iter()
            .map(|(by, descending)| SortKey {
                property: match by {
                    SortBy::Title => "title".into(),
                    SortBy::Number => ids.number.clone(),
                    SortBy::Select => ids.select.clone(),
                    SortBy::Date => ids.date.clone(),
                    SortBy::Check => ids.check.clone(),
                },
                descending: *descending,
            })
            .collect();
        let spec = QuerySpec {
            filter: Some(ids.node(&filter)),
            sorts: sort_keys,
            limit: 1000,
            ..QuerySpec::default()
        };
        let actual: Vec<String> = ws.database_query(db, &spec).expect("query").rows.into_iter().map(|r| r.title).collect();

        let mut expected: Vec<usize> = (0..rows.len()).filter(|&i| matches(&rows[i], &titles[i], &filter)).collect();
        // The manual order is the base; each key then sorts stably, so the
        // last key is applied first.
        for (by, descending) in sorts.iter().rev() {
            expected.sort_by(|&a, &b| sort_compare((&rows[a], &titles[a]), (&rows[b], &titles[b]), *by, *descending));
        }
        let expected: Vec<String> = expected.into_iter().map(|i| titles[i].clone()).collect();
        prop_assert_eq!(actual, expected, "filter {:?} sorts {:?}", filter, sorts);
    }
}
