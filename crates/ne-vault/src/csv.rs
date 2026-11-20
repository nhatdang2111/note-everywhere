//! A small, forgiving CSV reader and writer that wipes what it reads.
//!
//! Reading accepts `\n`, `\r\n` and `\r` line ends, quoted fields with `""`
//! escapes and embedded newlines, text after a closing quote, and a final line
//! without a line end. A quote in the middle of an unquoted field is literal.
//! A quote that is never closed is reported on its record instead of being
//! guessed at. The reader owns no copy of the input beyond the record it hands
//! out, and records are [`Zeroizing`].

use std::iter::Peekable;
use std::str::Chars;

use zeroize::Zeroizing;

/// One parsed record.
pub(crate) struct Record {
    pub fields: Zeroizing<Vec<String>>,
    /// A quoted field ran to the end of the input without closing.
    pub unterminated: bool,
}

pub(crate) struct CsvReader<'a> {
    chars: Peekable<Chars<'a>>,
}

impl<'a> CsvReader<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            chars: text
                .strip_prefix('\u{feff}')
                .unwrap_or(text)
                .chars()
                .peekable(),
        }
    }

    fn quoted_field(&mut self, field: &mut String) -> bool {
        // The opening quote is already consumed.
        loop {
            match self.chars.next() {
                None => return true,
                Some('"') if self.chars.peek() == Some(&'"') => {
                    self.chars.next();
                    field.push('"');
                }
                Some('"') => return false,
                Some(c) => field.push(c),
            }
        }
    }
}

impl Iterator for CsvReader<'_> {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        self.chars.peek()?;
        let mut fields = Zeroizing::new(Vec::new());
        let mut field = String::new();
        let mut unterminated = false;
        loop {
            match self.chars.next() {
                None => break,
                Some('"') if field.is_empty() => {
                    unterminated = self.quoted_field(&mut field);
                    if unterminated {
                        break;
                    }
                }
                Some(',') => fields.push(std::mem::take(&mut field)),
                Some('\n') => break,
                Some('\r') => {
                    if self.chars.peek() == Some(&'\n') {
                        self.chars.next();
                    }
                    break;
                }
                Some(c) => field.push(c),
            }
        }
        fields.push(field);
        Some(Record {
            fields,
            unterminated,
        })
    }
}

/// Appends one record to `out`, quoting fields that need it.
pub(crate) fn write_record<S: AsRef<str>>(out: &mut String, fields: &[S]) {
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let field = field.as_ref();
        let needs_quotes = field.contains([',', '"', '\n', '\r'])
            || field.starts_with(' ')
            || field.ends_with(' ');
        if needs_quotes {
            out.push('"');
            out.push_str(&field.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(field);
        }
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Vec<String>> {
        CsvReader::new(text).map(|r| r.fields.to_vec()).collect()
    }

    #[test]
    fn plain_quoted_and_multiline_fields() {
        assert_eq!(parse("a,b\n1,2\n"), [["a", "b"], ["1", "2"]]);
        assert_eq!(parse("a,\"x,y\",c"), [["a", "x,y", "c"]]);
        assert_eq!(parse("\"he said \"\"hi\"\"\",z"), [["he said \"hi\"", "z"]]);
        assert_eq!(
            parse("a,\"line1\nline2\"\r\nb,c"),
            [["a", "line1\nline2"], ["b", "c"]]
        );
        assert_eq!(parse("a,\"l1\r\nl2\""), [["a", "l1\r\nl2"]]);
    }

    #[test]
    fn line_ends_bom_and_empties() {
        assert_eq!(parse("\u{feff}a,b\r\n1,2\r\n"), [["a", "b"], ["1", "2"]]);
        assert_eq!(parse("a\rb\rc"), [["a"], ["b"], ["c"]]);
        assert_eq!(parse("a,,c\n,\n"), [vec!["a", "", "c"], vec!["", ""]]);
        assert_eq!(parse("a\n\nb"), [vec!["a"], vec![""], vec!["b"]]);
        assert!(parse("").is_empty());
        assert_eq!(parse("a,b,"), [["a", "b", ""]]);
    }

    #[test]
    fn tolerant_of_stray_quotes() {
        assert_eq!(parse("ab\"cd,e"), [["ab\"cd", "e"]]);
        assert_eq!(parse("\"ab\"cd,e"), [["abcd", "e"]]);
    }

    #[test]
    fn unterminated_quote_is_flagged() {
        let records: Vec<_> = CsvReader::new("a,\"never closed\nstill going").collect();
        assert_eq!(records.len(), 1);
        assert!(records[0].unterminated);
    }

    #[test]
    fn writer_round_trips_through_reader() {
        let fields = [
            "plain",
            "a,b",
            "quote\"inside",
            "multi\nline",
            " padded ",
            "",
        ];
        let mut out = String::new();
        write_record(&mut out, &fields);
        write_record(&mut out, &["second", "row"]);
        let parsed = parse(&out);
        assert_eq!(parsed[0], fields);
        assert_eq!(parsed[1], ["second", "row"]);
    }
}
