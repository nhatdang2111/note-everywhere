//! In-memory, diacritic-insensitive search over title, username, tags and URLs.
//!
//! Only those fields are searchable; passwords, keys, note text and notes are
//! never indexed. The search text lives in the unlocked cache and is wiped with
//! it.

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use zeroize::Zeroizing;

use crate::item::{ItemBody, ItemContent};

/// Lowercases and strips diacritics, so "Ghi chú" and "ghi chu" compare equal.
/// `đ` has no decomposition, so it is mapped by hand.
pub(crate) fn fold_into(out: &mut String, text: &str) {
    for ch in text.nfd().filter(|c| !is_combining_mark(*c)) {
        match ch {
            'đ' | 'Đ' => out.push('d'),
            _ => out.extend(ch.to_lowercase()),
        }
    }
}

/// The folded text a query is matched against. Fields are separated by a
/// newline, which a query token never contains.
pub(crate) fn search_text(content: &ItemContent) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::new());
    let mut field = |text: &str| {
        fold_into(&mut out, text);
        out.push('\n');
    };
    field(&content.title);
    content.tags.iter().for_each(|t| field(t));
    if let ItemBody::Login { username, urls, .. } = &content.body {
        field(username);
        urls.iter().for_each(|u| field(u));
    }
    out
}

/// Splits a query into folded tokens; an item matches when it contains all.
pub(crate) fn query_tokens(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|token| {
            let mut folded = String::new();
            fold_into(&mut folded, token);
            folded
        })
        .collect()
}

pub(crate) fn matches(search_text: &str, tokens: &[String]) -> bool {
    tokens
        .iter()
        .all(|token| search_text.contains(token.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(text: &str) -> String {
        let mut out = String::new();
        fold_into(&mut out, text);
        out
    }

    #[test]
    fn folding_removes_vietnamese_and_other_diacritics() {
        assert_eq!(fold("Ghi chú"), "ghi chu");
        assert_eq!(fold("Đặng Ngô"), "dang ngo");
        assert_eq!(fold("ĐỖ Việt"), "do viet");
        assert_eq!(fold("Crème Brûlée"), "creme brulee");
        assert_eq!(fold("ABC"), "abc");
    }

    #[test]
    fn all_tokens_must_match_and_blank_query_matches_all() {
        let text = "ngan hang\nwork\nviet@example.test\nhttps://bank.test\n";
        assert!(matches(text, &query_tokens("NGÂN hàng")));
        assert!(matches(text, &query_tokens("bank work")));
        assert!(!matches(text, &query_tokens("bank personal")));
        assert!(matches(text, &query_tokens("   ")));
        assert!(
            matches(text, &query_tokens("hang work")),
            "tokens match in any field"
        );
    }
}
