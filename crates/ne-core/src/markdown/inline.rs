//! Renders inline content (text, marks, breaks) as CommonMark/GFM text.

use crate::page::{Inline, Marks, Run, normalize_inline};

/// Where the inline text ends up; headings cannot hold line breaks and must
/// escape `#`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Context {
    Paragraph,
    Heading,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mark {
    Link(String),
    Strike,
    Bold,
    Italic,
}

impl Mark {
    fn open(&self) -> &'static str {
        match self {
            Self::Link(_) => "[",
            Self::Strike => "~~",
            Self::Bold => "**",
            Self::Italic => "*",
        }
    }
}

/// The open marks of a run in nesting order, outermost first.
fn stack_of(marks: &Marks) -> Vec<Mark> {
    let mut stack = Vec::new();
    if let Some(href) = marks.link.as_deref().filter(|h| !h.is_empty()) {
        stack.push(Mark::Link(href.to_owned()));
    }
    if marks.strike {
        stack.push(Mark::Strike);
    }
    if marks.bold {
        stack.push(Mark::Bold);
    }
    if marks.italic {
        stack.push(Mark::Italic);
    }
    stack
}

/// Emphasis cannot start or end next to whitespace, so whitespace at the edge
/// of an emphasised run is moved out of the emphasis (it keeps any link).
fn split_edge_whitespace(inline: Vec<Inline>) -> Vec<Inline> {
    let mut out = Vec::with_capacity(inline.len());
    for item in inline {
        let Inline::Text(run) = item else {
            out.push(item);
            continue;
        };
        let emphasised = run.marks.bold || run.marks.italic || run.marks.strike;
        if !emphasised || run.marks.code {
            out.push(Inline::Text(run));
            continue;
        }
        let core = run.text.trim();
        let outside = Marks {
            link: run.marks.link.clone(),
            ..Marks::default()
        };
        if core.is_empty() {
            out.push(Inline::Text(Run {
                text: run.text,
                marks: outside,
            }));
            continue;
        }
        let lead = &run.text[..run.text.len() - run.text.trim_start().len()];
        let trail = &run.text[run.text.trim_end().len()..];
        for (text, marks) in [
            (lead, outside.clone()),
            (core, run.marks.clone()),
            (trail, outside),
        ] {
            if !text.is_empty() {
                out.push(Inline::Text(Run {
                    text: text.to_owned(),
                    marks,
                }));
            }
        }
    }
    normalize_inline(out)
}

/// Formats a link destination, wrapping it in `<...>` when it holds characters
/// that would end a bare destination.
pub(crate) fn format_destination(href: &str) -> String {
    let needs_brackets = href.is_empty()
        || href.chars().any(|c| {
            c.is_whitespace() || c.is_control() || matches!(c, '(' | ')' | '<' | '>' | '\\')
        });
    if !needs_brackets {
        return href.to_owned();
    }
    let mut out = String::with_capacity(href.len() + 2);
    out.push('<');
    for c in href.chars() {
        match c {
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            '\n' | '\r' | '\t' => out.push_str("%20"),
            other => out.push(other),
        }
    }
    out.push('>');
    out
}

/// Renders a code span, choosing a delimiter longer than any backtick run in
/// the text and padding where CommonMark would otherwise strip or merge.
fn code_span(text: &str) -> String {
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest + 1);
    let all_space = text.chars().all(|c| c == ' ');
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && !all_space);
    if pad {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

struct Writer {
    out: String,
    open: Vec<Mark>,
    line_start: bool,
    context: Context,
}

impl Writer {
    fn transition(&mut self, target: &[Mark]) {
        let common = self
            .open
            .iter()
            .zip(target)
            .take_while(|(a, b)| a == b)
            .count();
        while self.open.len() > common {
            match self.open.pop() {
                Some(Mark::Link(href)) => {
                    self.out.push_str("](");
                    self.out.push_str(&format_destination(&href));
                    self.out.push(')');
                }
                Some(mark) => self.out.push_str(mark.open()),
                None => {}
            }
        }
        for mark in &target[common..] {
            if matches!(mark, Mark::Link(_)) && self.out.ends_with('!') {
                // `![` would start an image.
                self.out.insert(self.out.len() - 1, '\\');
            }
            self.out.push_str(mark.open());
            self.open.push(mark.clone());
        }
    }

    fn hard_break(&mut self) {
        match self.context {
            Context::Heading => self.out.push(' '),
            Context::Paragraph => {
                self.out.push_str("\\\n");
                self.line_start = true;
            }
        }
    }

    fn text(&mut self, text: &str) {
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                self.hard_break();
            }
            self.escaped(line);
        }
    }

    fn escaped(&mut self, line: &str) {
        let chars: Vec<char> = line.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = chars.get(i + 1).copied();
            let escape = match c {
                '\\' | '`' | '*' | '[' | ']' | '<' | '~' => true,
                '_' => {
                    !prev.is_some_and(char::is_alphanumeric)
                        || !next.is_some_and(char::is_alphanumeric)
                }
                '&' => looks_like_entity(&chars[i + 1..]),
                '#' => self.line_start || self.context == Context::Heading,
                '>' | '-' | '+' | '=' => self.line_start,
                // "1." and "1)" at the start of a line open an ordered list.
                '.' | ')' => self.line_start && only_digits_before(&chars[..i]),
                _ => false,
            };
            if escape {
                self.out.push('\\');
            }
            self.out.push(c);
            let in_number_prefix =
                c.is_ascii_digit() && self.line_start && ordered_prefix(&chars[i..]);
            if c != ' ' && c != '\t' && !in_number_prefix {
                self.line_start = false;
            }
        }
    }
}

/// True when `rest` (the text after an `&`) reads as the rest of a character
/// reference such as `amp;` or `#35;`.
fn looks_like_entity(rest: &[char]) -> bool {
    let name = rest
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '#')
        .count();
    name > 0 && rest.get(name) == Some(&';')
}

/// True when `head` is optional spaces followed by one or more digits.
fn only_digits_before(head: &[char]) -> bool {
    let mut digits = head.iter().skip_while(|c| **c == ' ').peekable();
    digits.peek().is_some() && digits.all(char::is_ascii_digit)
}

/// True when `rest` is digits followed by `.` or `)`.
fn ordered_prefix(rest: &[char]) -> bool {
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && matches!(rest.get(digits), Some('.' | ')'))
}

/// Renders inline content as Markdown text. `at_line_start` says whether the
/// first character begins a line that block syntax could claim (so `#`, `-`,
/// `1.` and friends are escaped).
pub(crate) fn render_inline(inline: &[Inline], context: Context, at_line_start: bool) -> String {
    let prepared = split_edge_whitespace(normalize_inline(inline.to_vec()));
    let mut writer = Writer {
        out: String::new(),
        open: Vec::new(),
        line_start: at_line_start,
        context,
    };
    for item in &prepared {
        match item {
            Inline::Break => {
                writer.transition(&[]);
                writer.hard_break();
            }
            Inline::Text(run) => {
                writer.transition(&stack_of(&run.marks));
                if run.marks.code {
                    for (index, line) in run.text.split('\n').enumerate() {
                        if index > 0 {
                            writer.hard_break();
                        }
                        if !line.is_empty() {
                            writer.out.push_str(&code_span(line));
                            writer.line_start = false;
                        }
                    }
                } else {
                    writer.text(&run.text);
                }
            }
        }
    }
    writer.transition(&[]);
    writer.out
}
