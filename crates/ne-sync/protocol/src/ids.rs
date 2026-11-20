//! Identifier rules and hex helpers used on both sides of the wire.

use crate::limits::MAX_ID_LEN;

/// Document, device and workspace identifiers: 1 to 64 bytes of
/// `A-Z a-z 0-9 . _ : -`.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// Lowercases and checks a username: 3 to 64 bytes of `a-z 0-9 . _ @ + -`.
pub fn normalize_username(raw: &str) -> Option<String> {
    let name = raw.trim().to_ascii_lowercase();
    let ok = (3..=MAX_ID_LEN).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._@+-".contains(&b));
    ok.then_some(name)
}

/// A device label shown in the session list: 1 to 64 characters, no control
/// characters.
pub fn is_valid_label(label: &str) -> bool {
    let chars = label.chars().count();
    (1..=64).contains(&chars) && !label.chars().any(char::is_control)
}

/// Lowercase hex encoding.
pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}

/// Decodes lowercase hex of exactly `N` bytes.
pub fn hex_decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    let raw = text.as_bytes();
    if raw.len() != N * 2 {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    let (pairs, _) = raw.as_chunks::<2>();
    for (slot, pair) in out.iter_mut().zip(pairs) {
        *slot = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_accept_the_documented_alphabet_only() {
        assert!(is_valid_id("doc-1.a_b:c"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("has space"));
        assert!(!is_valid_id("ünï"));
        assert!(!is_valid_id(&"x".repeat(65)));
        assert!(is_valid_id(&"x".repeat(64)));
    }

    #[test]
    fn usernames_are_lowercased_and_checked() {
        assert_eq!(
            normalize_username("  Alice@Example.com "),
            Some("alice@example.com".into())
        );
        assert_eq!(normalize_username("ab"), None);
        assert_eq!(normalize_username("bad name"), None);
    }

    #[test]
    fn labels_reject_control_characters() {
        assert!(is_valid_label("Work laptop"));
        assert!(!is_valid_label(""));
        assert!(!is_valid_label("a\nb"));
        assert!(!is_valid_label(&"x".repeat(65)));
    }

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        let bytes = [0u8, 1, 0xab, 0xff];
        let text = hex_encode(&bytes);
        assert_eq!(text, "0001abff");
        assert_eq!(hex_decode::<4>(&text), Some(bytes));
        assert_eq!(hex_decode::<4>("0001ABFF"), None);
        assert_eq!(hex_decode::<4>("0001abf"), None);
        assert_eq!(hex_decode::<3>(&text), None);
    }
}
