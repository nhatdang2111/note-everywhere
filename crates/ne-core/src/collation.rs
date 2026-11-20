//! Vietnamese-aware ordering of text.
//!
//! [`sort_key`] turns a string into bytes whose plain byte-wise comparison
//! orders text the way a Vietnamese reader expects, so a database can sort in
//! SQLite (or anywhere) without a collation callback.
//!
//! # Order
//!
//! 1. Letters compare by the Vietnamese alphabet, where the letters with a
//!    modifier are letters of their own:
//!    `a ă â b c d đ e ê f g h i j k l m n o ô ơ p q r s t u ư v w x y z`.
//!    So "An" < "Ăn" < "Ân" < "Bn", and "Ao" < "Ăn" is not true: the first
//!    letter decides before anything later in the word.
//! 2. Only when two strings spell the same letters, the tone marks decide, in
//!    the dictionary order: none, grave (huyền), hook above (hỏi), tilde
//!    (ngã), acute (sắc), dot below (nặng). "ma" < "mà" < "mả" < "mã" < "má" <
//!    "mạ".
//! 3. Only then case decides: lower case before upper case.
//!
//! Before letters come spaces and punctuation (by code point), then digits
//! (by value, digit by digit), then letters of other scripts (by code point).
//! Text is normalised first, so precomposed and combining spellings of the
//! same letter produce the same key.

use unicode_normalization::UnicodeNormalization;

const CLASS_SYMBOL: u8 = 1;
const CLASS_DIGIT: u8 = 2;
const CLASS_LATIN: u8 = 3;
const CLASS_OTHER: u8 = 4;

const ACUTE: char = '\u{0301}';
const GRAVE: char = '\u{0300}';
const HOOK: char = '\u{0309}';
const TILDE: char = '\u{0303}';
const DOT_BELOW: char = '\u{0323}';
const BREVE: char = '\u{0306}';
const CIRCUMFLEX: char = '\u{0302}';
const HORN: char = '\u{031B}';

/// Rank of a base letter in the Vietnamese alphabet, with its modified forms.
fn letter_rank(base: char, modifier: Option<char>) -> u16 {
    // Each plain letter owns a block of 4 ranks, so a modified form slots in
    // right behind its base letter: a, ă, â, then b.
    let slot = |letter: char| u16::from(letter as u8 - b'a') * 4;
    match (base, modifier) {
        ('a', Some(BREVE)) => slot('a') + 1,
        ('a', Some(CIRCUMFLEX)) => slot('a') + 2,
        ('e', Some(CIRCUMFLEX)) => slot('e') + 1,
        ('o', Some(CIRCUMFLEX)) => slot('o') + 1,
        ('o', Some(HORN)) => slot('o') + 2,
        ('u', Some(HORN)) => slot('u') + 1,
        _ => slot(base),
    }
}

fn tone_rank(mark: char) -> Option<u8> {
    match mark {
        GRAVE => Some(1),
        HOOK => Some(2),
        TILDE => Some(3),
        ACUTE => Some(4),
        DOT_BELOW => Some(5),
        _ => None,
    }
}

fn push_weight(out: &mut Vec<u8>, class: u8, value: u16) {
    out.push(class);
    out.extend_from_slice(&value.to_be_bytes());
}

/// The sort key of `text`; compare keys as byte strings.
pub fn sort_key(text: &str) -> Vec<u8> {
    let mut primary = Vec::with_capacity(text.len() * 3);
    let mut tones = Vec::with_capacity(text.len());
    let mut cases = Vec::with_capacity(text.len());
    // `đ` has no decomposition, map it before decomposing everything else.
    let mut chars = text.chars().flat_map(|c| match c {
        'đ' => vec!['\u{1}'],
        'Đ' => vec!['\u{2}'],
        other => other.nfd().collect(),
    });
    let mut pending = chars.next();
    while let Some(c) = pending {
        // Gather the combining marks that follow a base character.
        let mut marks = Vec::new();
        pending = chars.next();
        while let Some(next) = pending.filter(|n| ('\u{0300}'..='\u{036F}').contains(n)) {
            marks.push(next);
            pending = chars.next();
        }
        let upper = c.is_uppercase() || c == '\u{2}';
        let lower = if c == '\u{2}' {
            '\u{1}'
        } else {
            c.to_lowercase().next().unwrap_or(c)
        };
        let modifier = marks
            .iter()
            .copied()
            .find(|m| matches!(*m, BREVE | CIRCUMFLEX | HORN));
        let tone = marks.iter().copied().find_map(tone_rank).unwrap_or(0);
        if lower == '\u{1}' {
            // đ sits between d and e.
            push_weight(&mut primary, CLASS_LATIN, letter_rank('d', None) + 1);
        } else if lower.is_ascii_lowercase() {
            push_weight(&mut primary, CLASS_LATIN, letter_rank(lower, modifier));
        } else if lower.is_ascii_digit() {
            push_weight(&mut primary, CLASS_DIGIT, u16::from(lower as u8 - b'0'));
        } else if lower.is_alphabetic() {
            let value = u16::try_from(u32::from(lower).min(u32::from(u16::MAX))).unwrap_or(0);
            push_weight(&mut primary, CLASS_OTHER, value);
        } else {
            let value = u16::try_from(u32::from(lower).min(u32::from(u16::MAX))).unwrap_or(0);
            push_weight(&mut primary, CLASS_SYMBOL, value);
        }
        tones.push(tone);
        cases.push(u8::from(upper));
    }
    let mut key = primary;
    key.push(0);
    key.extend(tones);
    key.push(0);
    key.extend(cases);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(words: &[&str]) -> Vec<String> {
        let mut owned: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
        owned.sort_by_key(|w| sort_key(w));
        owned
    }

    #[test]
    fn alphabet_order_puts_modified_letters_behind_their_base() {
        let words = [
            "z", "y", "ư", "u", "ơ", "ô", "o", "ê", "e", "đ", "d", "â", "ă", "a", "b",
        ];
        assert_eq!(
            sorted(&words),
            [
                "a", "ă", "â", "b", "d", "đ", "e", "ê", "o", "ô", "ơ", "u", "ư", "y", "z"
            ]
        );
    }

    #[test]
    fn tones_follow_the_dictionary_order() {
        let words = ["mạ", "má", "mã", "mả", "mà", "ma"];
        assert_eq!(sorted(&words), ["ma", "mà", "mả", "mã", "má", "mạ"]);
    }

    #[test]
    fn letters_outrank_tones() {
        // "ba" with any tone sorts before "bă" even though it has a mark.
        assert_eq!(sorted(&["bă", "bạ", "ba"]), ["ba", "bạ", "bă"]);
        assert_eq!(sorted(&["đa", "dz", "da"]), ["da", "dz", "đa"]);
    }

    #[test]
    fn case_only_breaks_ties_and_lower_comes_first() {
        assert_eq!(sorted(&["Ba", "ba", "bb"]), ["ba", "Ba", "bb"]);
    }

    #[test]
    fn prefixes_sort_first_and_digits_before_letters() {
        assert_eq!(
            sorted(&["abc", "ab", "1", "a b", "_"]),
            ["_", "1", "a b", "ab", "abc"]
        );
        assert_eq!(sorted(&["a10", "a9", "a1"]), ["a1", "a10", "a9"]);
    }

    #[test]
    fn composed_and_decomposed_spellings_agree() {
        assert_eq!(sort_key("Việt"), sort_key("Vie\u{0323}\u{0302}t"));
        assert_eq!(sort_key("đường"), sort_key("đươ\u{0300}ng"));
    }

    #[test]
    fn a_vietnamese_name_list_sorts_like_a_phone_book() {
        let names = [
            "Nguyễn Văn An",
            "Ngô Thị Bé",
            "Đặng Minh",
            "Đinh Tuấn",
            "Âu Cơ",
            "Anh Tú",
            "Ân Nguyễn",
            "Bùi Hạnh",
            "Trần Quốc",
            "Trương Vinh",
        ];
        assert_eq!(
            sorted(&names),
            [
                "Anh Tú",
                "Ân Nguyễn",
                "Âu Cơ",
                "Bùi Hạnh",
                "Đặng Minh",
                "Đinh Tuấn",
                "Ngô Thị Bé",
                "Nguyễn Văn An",
                "Trần Quốc",
                "Trương Vinh",
            ]
        );
    }
}
