//! A small RFC 4180 reader for the CSV files of an export: quoted fields,
//! doubled quotes, line breaks inside quotes, an optional byte-order mark.

use crate::error::{Error, Result};

/// Most rows read from one file.
pub const MAX_CSV_ROWS: usize = 200_000;
/// Most columns of one row.
pub const MAX_CSV_COLUMNS: usize = 1_000;

/// Parses CSV text into rows of fields. Blank lines are skipped; every row
/// keeps the number of fields it has (rows can be ragged).
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut field_started = false;
    let mut chars = text.chars().peekable();
    let end_row = |row: &mut Vec<String>,
                   field: &mut String,
                   started: &mut bool,
                   rows: &mut Vec<Vec<String>>|
     -> Result<()> {
        if *started || !row.is_empty() || !field.is_empty() {
            row.push(std::mem::take(field));
            if row.len() > MAX_CSV_COLUMNS {
                return Err(Error::Invalid("the CSV has too many columns".into()));
            }
            // A line with one empty field is a blank line.
            if !(row.len() == 1 && row[0].is_empty() && !*started) {
                rows.push(std::mem::take(row));
            } else {
                row.clear();
            }
            if rows.len() > MAX_CSV_ROWS {
                return Err(Error::Invalid("the CSV has too many rows".into()));
            }
        }
        *started = false;
        Ok(())
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                field_started = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                field_started = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                end_row(&mut row, &mut field, &mut field_started, &mut rows)?;
            }
            '\n' => end_row(&mut row, &mut field, &mut field_started, &mut rows)?,
            other => {
                field.push(other);
                field_started = true;
            }
        }
    }
    if quoted {
        return Err(Error::Invalid("the CSV ends inside a quoted field".into()));
    }
    end_row(&mut row, &mut field, &mut field_started, &mut rows)?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_quoted_and_multiline_fields() {
        let rows = parse_csv("a,b,c\n1,\"x, y\",\"line1\nline2\"\n\"he said \"\"hi\"\"\",,3\n")
            .expect("csv");
        assert_eq!(rows[0], ["a", "b", "c"]);
        assert_eq!(rows[1], ["1", "x, y", "line1\nline2"]);
        assert_eq!(rows[2], ["he said \"hi\"", "", "3"]);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn byte_order_mark_crlf_and_blank_lines() {
        let rows = parse_csv("\u{feff}Tên,Giá\r\n\r\nA,1\r\nB,2").expect("csv");
        assert_eq!(rows, [vec!["Tên", "Giá"], vec!["A", "1"], vec!["B", "2"]]);
    }

    #[test]
    fn empty_trailing_field_and_unterminated_quote() {
        assert_eq!(parse_csv("a,b,\n").expect("csv"), [vec!["a", "b", ""]]);
        assert!(parse_csv("a,\"b").is_err());
        assert!(parse_csv("").expect("empty").is_empty());
    }
}
