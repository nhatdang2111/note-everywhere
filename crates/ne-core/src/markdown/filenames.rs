//! File and folder names for export that are valid on Windows and macOS.

use std::collections::HashSet;

/// Longest stem produced, in characters. Windows limits a path component to
/// 255 UTF-16 units; this leaves room for a ` (n)` suffix and an extension.
pub const MAX_STEM_CHARS: usize = 100;

const FALLBACK: &str = "Untitled";
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// True for the device names Windows reserves, with or without an extension.
fn is_reserved(base: &str) -> bool {
    let upper = base.trim_end().to_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            let mut chars = rest.chars();
            if let (Some(digit), None) = (chars.next(), chars.next()) {
                // Windows also reserves the superscript digit forms.
                if matches!(digit, '0'..='9' | '¹' | '²' | '³') {
                    return true;
                }
            }
        }
    }
    false
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn trim_end_dots_and_spaces(text: &str) -> &str {
    text.trim_end_matches(['.', ' ', '\t'])
}

/// Turns arbitrary text into a name that is valid as one path component on
/// Windows and macOS: forbidden characters and controls become `_`, trailing
/// dots and spaces go, reserved device names get a `_` prefix, a leading dot
/// (a hidden file) becomes `_`, the length is capped and an empty result is
/// `Untitled`. The result never contains a path separator.
pub fn sanitize_stem(title: &str) -> String {
    let mapped: String = title
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut name = trim_end_dots_and_spaces(mapped.trim_start()).to_owned();
    if name.starts_with('.') {
        name.replace_range(..1, "_");
    }
    if name.is_empty() {
        return FALLBACK.to_owned();
    }
    let base = name.split('.').next().unwrap_or_default();
    if is_reserved(base) {
        name.insert(0, '_');
    }
    let capped = truncate_chars(&name, MAX_STEM_CHARS);
    let capped = trim_end_dots_and_spaces(&capped);
    if capped.is_empty() {
        FALLBACK.to_owned()
    } else {
        capped.to_owned()
    }
}

/// Splits a file name into stem and extension at the last dot, keeping only a
/// short alphanumeric extension.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 10
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            (stem, ext)
        }
        _ => (name, ""),
    }
}

/// Hands out names that are unique within one folder, comparing without case
/// because Windows and macOS file systems do.
#[derive(Debug, Default)]
pub struct NameAllocator {
    used: HashSet<String>,
}

impl NameAllocator {
    /// An allocator with nothing taken.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a name as taken so it is never handed out.
    pub fn reserve(&mut self, name: &str) {
        self.used.insert(name.to_lowercase());
    }

    /// A unique stem for `title` (no extension). The allocator remembers it.
    pub fn unique_stem(&mut self, title: &str) -> String {
        let base = sanitize_stem(title);
        self.claim(&base, "")
    }

    /// A unique file name keeping the extension of `name`, for attachments.
    pub fn unique_file_name(&mut self, name: &str) -> String {
        // Split before sanitising so a long name cannot cut the extension off.
        let (stem, ext) = split_extension(name);
        self.claim(&sanitize_stem(stem), ext)
    }

    fn claim(&mut self, stem: &str, ext: &str) -> String {
        let suffix = if ext.is_empty() {
            String::new()
        } else {
            format!(".{ext}")
        };
        let mut candidate = format!("{stem}{suffix}");
        let mut n = 2;
        while self.used.contains(&candidate.to_lowercase()) {
            let tag = format!(" ({n})");
            let room = MAX_STEM_CHARS.saturating_sub(tag.chars().count());
            let cut = truncate_chars(stem, room);
            candidate = format!("{}{tag}{suffix}", trim_end_dots_and_spaces(&cut));
            n += 1;
        }
        self.used.insert(candidate.to_lowercase());
        candidate
    }
}
