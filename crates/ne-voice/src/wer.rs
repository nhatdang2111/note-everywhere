//! Word error rate with the normalisation used for the quality gate:
//! Unicode NFC, lowercase, punctuation stripped, diacritics kept.

use unicode_normalization::UnicodeNormalization;

/// Splits text into comparable words. Punctuation becomes a separator, except
/// apostrophes inside words ("don't") and separators between digits ("9:30",
/// "3.5", "1,000"), which are dropped so the number stays one word.
pub fn normalize(text: &str) -> Vec<String> {
    let nfc: String = text.nfc().collect::<String>().to_lowercase();
    let chars: Vec<char> = nfc.chars().collect();
    let mut cleaned = String::with_capacity(nfc.len());
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
        let between_alnum =
            prev.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
        let between_digits =
            prev.is_some_and(|p| p.is_ascii_digit()) && next.is_some_and(|n| n.is_ascii_digit());
        if c.is_alphanumeric() {
            cleaned.push(c);
        } else if (c == '\'' || c == '\u{2019}') && between_alnum {
            // Keep contractions as one word without the apostrophe.
        } else if matches!(c, ':' | '.' | ',') && between_digits {
            // Keep numbers like 9:30 as a single word.
        } else {
            cleaned.push(' ');
        }
    }
    cleaned.split_whitespace().map(str::to_owned).collect()
}

/// Edit distance counts between a reference and a hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WordErrors {
    pub edits: usize,
    pub reference_words: usize,
}

impl WordErrors {
    /// Edits divided by reference length. 0.0 when both are empty.
    pub fn rate(&self) -> f64 {
        if self.reference_words == 0 {
            return if self.edits == 0 { 0.0 } else { 1.0 };
        }
        self.edits as f64 / self.reference_words as f64
    }

    pub fn add(&mut self, other: WordErrors) {
        self.edits += other.edits;
        self.reference_words += other.reference_words;
    }
}

/// Word-level Levenshtein distance after normalisation.
pub fn word_errors(reference: &str, hypothesis: &str) -> WordErrors {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, hw) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(rw != hw);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    WordErrors {
        edits: prev[h.len()],
        reference_words: r.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_and_strips_punctuation() {
        assert_eq!(normalize("Hello, World!"), ["hello", "world"]);
    }

    #[test]
    fn normalize_keeps_diacritics() {
        assert_eq!(
            normalize("Hôm nay, tôi cần mua sữa."),
            ["hôm", "nay", "tôi", "cần", "mua", "sữa"]
        );
    }

    #[test]
    fn normalize_unifies_composed_and_decomposed_forms() {
        let decomposed = "e\u{0302}\u{0301}"; // e + circumflex + acute
        let composed = "\u{1EBF}"; // ế
        assert_eq!(normalize(decomposed), normalize(composed));
    }

    #[test]
    fn normalize_keeps_numbers_and_contractions_whole() {
        assert_eq!(
            normalize("at 9:30, don't pay 3.5"),
            ["at", "930", "dont", "pay", "35"]
        );
        assert_eq!(normalize("end. Next"), ["end", "next"]);
    }

    #[test]
    fn identical_text_has_zero_errors() {
        let e = word_errors("Tôi cần mua sữa.", "tôi cần mua sữa");
        assert_eq!(e.edits, 0);
        assert_eq!(e.rate(), 0.0);
    }

    #[test]
    fn counts_substitution_insertion_deletion() {
        assert_eq!(word_errors("a b c d", "a x c d").edits, 1);
        assert_eq!(word_errors("a b c d", "a b c d e").edits, 1);
        assert_eq!(word_errors("a b c d", "a b d").edits, 1);
        let e = word_errors("a b c d", "d c b a");
        assert_eq!(e.reference_words, 4);
        assert_eq!(e.edits, 4);
    }

    #[test]
    fn diacritic_difference_is_an_error() {
        assert_eq!(word_errors("sữa", "sua").edits, 1);
    }

    #[test]
    fn empty_inputs() {
        assert_eq!(word_errors("", "").rate(), 0.0);
        assert_eq!(word_errors("", "x").rate(), 1.0);
        assert_eq!(word_errors("a b", "").rate(), 1.0);
    }

    #[test]
    fn totals_accumulate() {
        let mut total = WordErrors::default();
        total.add(word_errors("a b", "a c"));
        total.add(word_errors("x y z", "x y z"));
        assert_eq!((total.edits, total.reference_words), (1, 5));
        assert!((total.rate() - 0.2).abs() < 1e-9);
    }
}
