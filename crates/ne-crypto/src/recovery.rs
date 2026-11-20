//! Recovery keys: a high-entropy secret the user writes down.
//!
//! A recovery key is 25 random bytes (200 bits). It is shown as 44 Crockford
//! base32 symbols in 11 groups of four:
//!
//! ```text
//! 40 data symbols (the 25 bytes) | 4 checksum symbols (20 bits of SHA-256)
//! XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-CCCC
//! ```
//!
//! The checksum catches typos before any decryption is attempted: a wrong
//! symbol is reported as a checksum mismatch rather than as a failed unlock
//! (a random error slips through with probability 2^-20). Parsing is
//! case-insensitive, ignores dashes and whitespace, and follows Crockford's
//! rule that `O` reads as `0` and `I`/`L` read as `1`.
//!
//! A recovery key already has full entropy, so the wrapping key is derived
//! with HKDF alone, without password stretching. Account and vault recovery
//! keys derive from different labels, so mixing them up fails closed.

use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::keys::{KEY_LEN, SecretKey, WrappingKey};
use crate::random;

/// Entropy of a recovery key in bytes (200 bits).
pub const SECRET_LEN: usize = 25;
const DATA_SYMBOLS: usize = SECRET_LEN * 8 / 5;
const CHECKSUM_SYMBOLS: usize = 4;
const TOTAL_SYMBOLS: usize = DATA_SYMBOLS + CHECKSUM_SYMBOLS;
const GROUP: usize = 4;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHECKSUM_DOMAIN: &[u8] = b"note-everywhere/v1/recovery-checksum";

/// Which secret a recovery key protects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKind {
    /// Protects the account root key.
    Account,
    /// Protects the vault key.
    Vault,
}

impl RecoveryKind {
    fn info(self) -> &'static [u8] {
        match self {
            Self::Account => b"note-everywhere/v1/recovery/account",
            Self::Vault => b"note-everywhere/v1/recovery/vault",
        }
    }
}

/// Why a typed recovery key was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryParseError {
    /// A character outside the Crockford alphabet.
    InvalidCharacter,
    /// Wrong number of symbols after removing separators.
    WrongLength,
    /// All symbols are valid but the checksum disagrees: a typo.
    ChecksumMismatch,
}

impl std::fmt::Display for RecoveryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidCharacter => "recovery key contains an invalid character",
            Self::WrongLength => "recovery key has the wrong length",
            Self::ChecksumMismatch => "recovery key checksum does not match; check for typos",
        })
    }
}

impl std::error::Error for RecoveryParseError {}

/// A recovery key. Wiped on drop; `Debug` prints no secret.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RecoveryKey([u8; SECRET_LEN]);

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(<redacted>)")
    }
}

impl RecoveryKey {
    /// Generates a new random recovery key.
    pub fn generate() -> Self {
        Self(random::array::<SECRET_LEN>())
    }

    /// Renders the key for the user to write down. Show it once, never log it.
    pub fn to_display_string(&self) -> Zeroizing<String> {
        let mut symbols = encode_symbols(&self.0);
        symbols.extend(checksum_symbols(&self.0));
        let mut text = String::with_capacity(TOTAL_SYMBOLS + TOTAL_SYMBOLS / GROUP);
        for (i, symbol) in symbols.iter().enumerate() {
            if i > 0 && i % GROUP == 0 {
                text.push('-');
            }
            text.push(char::from(ALPHABET[usize::from(*symbol)]));
        }
        symbols.zeroize();
        Zeroizing::new(text)
    }

    /// Parses what the user typed, verifying the checksum.
    pub fn parse(input: &str) -> Result<Self, RecoveryParseError> {
        let mut symbols = Zeroizing::new(Vec::with_capacity(TOTAL_SYMBOLS));
        for ch in input.chars() {
            if ch == '-' || ch == '_' || ch.is_whitespace() {
                continue;
            }
            symbols.push(decode_symbol(ch).ok_or(RecoveryParseError::InvalidCharacter)?);
        }
        if symbols.len() != TOTAL_SYMBOLS {
            return Err(RecoveryParseError::WrongLength);
        }
        let (data, checksum) = symbols.split_at(DATA_SYMBOLS);
        let mut secret = [0u8; SECRET_LEN];
        decode_symbols(data, &mut secret);
        if checksum_symbols(&secret).as_slice() != checksum {
            secret.zeroize();
            return Err(RecoveryParseError::ChecksumMismatch);
        }
        Ok(Self(secret))
    }

    /// Derives the key that wraps the root key (or vault key) for `kind`.
    pub fn wrapping_key(&self, kind: RecoveryKind) -> WrappingKey {
        let hkdf = Hkdf::<Sha256>::new(None, &self.0);
        let mut out = [0u8; KEY_LEN];
        // 32 bytes is far below the HKDF-SHA256 output limit, so this cannot fail.
        hkdf.expand(kind.info(), &mut out)
            .expect("32-byte HKDF output is within limits");
        let key = WrappingKey::from_bytes(&out);
        out.zeroize();
        key
    }
}

impl PartialEq for RecoveryKey {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for RecoveryKey {}

/// Splits bytes into 5-bit symbol values. `bytes.len() * 8` must be a multiple of 5.
fn encode_symbols(bytes: &[u8]) -> Vec<u8> {
    let mut symbols = Vec::with_capacity(bytes.len() * 8 / 5);
    let mut buffer = 0u16;
    let mut bits = 0u32;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            symbols.push(((buffer >> bits) & 0x1f) as u8);
        }
        buffer &= (1 << bits) - 1;
    }
    symbols
}

/// Inverse of [`encode_symbols`] for a whole number of bytes.
fn decode_symbols(symbols: &[u8], out: &mut [u8]) {
    let mut buffer = 0u16;
    let mut bits = 0u32;
    let mut index = 0;
    for symbol in symbols {
        buffer = (buffer << 5) | u16::from(*symbol);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out[index] = (buffer >> bits) as u8;
            index += 1;
            buffer &= (1 << bits) - 1;
        }
    }
}

fn checksum_symbols(secret: &[u8; SECRET_LEN]) -> [u8; CHECKSUM_SYMBOLS] {
    let digest = Sha256::new()
        .chain_update(CHECKSUM_DOMAIN)
        .chain_update(secret)
        .finalize();
    let bits =
        (u32::from(digest[0]) << 12) | (u32::from(digest[1]) << 4) | (u32::from(digest[2]) >> 4);
    let mut out = [0u8; CHECKSUM_SYMBOLS];
    for (i, symbol) in out.iter_mut().enumerate() {
        *symbol = ((bits >> (15 - 5 * i)) & 0x1f) as u8;
    }
    out
}

fn decode_symbol(ch: char) -> Option<u8> {
    let upper = ch.to_ascii_uppercase();
    let normalised = match upper {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    let byte = u8::try_from(normalised).ok()?;
    ALPHABET.iter().position(|a| *a == byte).map(|p| p as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_format_is_grouped_and_round_trips() {
        let key = RecoveryKey::generate();
        let text = key.to_display_string();
        assert_eq!(text.len(), TOTAL_SYMBOLS + 10);
        assert_eq!(text.split('-').count(), 11);
        assert!(text.split('-').all(|g| g.len() == 4));
        assert_eq!(RecoveryKey::parse(&text).expect("parse"), key);
    }

    #[test]
    fn known_symbol_encoding() {
        // 25 zero bytes encode to 40 zeros; all-ones bytes to 40 'Z'.
        assert!(encode_symbols(&[0; SECRET_LEN]).iter().all(|s| *s == 0));
        assert!(encode_symbols(&[0xff; SECRET_LEN]).iter().all(|s| *s == 31));
        // 0x00 0x44 0x32 0x14 0xC7 -> "0123456789" in Crockford (RFC test pattern).
        let symbols = encode_symbols(&[0x00, 0x44, 0x32, 0x14, 0xC7]);
        assert_eq!(symbols, [0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn parsing_ignores_case_separators_and_whitespace() {
        let key = RecoveryKey::generate();
        let text = key.to_display_string();
        let messy = format!(" {} ", text.to_lowercase().replace('-', "  _ "));
        assert_eq!(RecoveryKey::parse(&messy).expect("parse"), key);
        let compact = text.replace('-', "");
        assert_eq!(RecoveryKey::parse(&compact).expect("parse"), key);
    }

    #[test]
    fn ambiguous_letters_read_as_digits() {
        let key = RecoveryKey([0; SECRET_LEN]);
        let text = key.to_display_string().replace('0', "O");
        assert_eq!(RecoveryKey::parse(&text).expect("parse"), key);
    }

    /// Uses a fixed key so the 2^-20 chance of a checksum collision cannot
    /// make this test flaky.
    #[test]
    fn every_single_symbol_typo_is_detected() {
        let key = RecoveryKey([7; SECRET_LEN]);
        let text = key.to_display_string();
        let symbols: Vec<char> = text.chars().collect();
        for index in (0..symbols.len()).filter(|i| symbols[*i] != '-') {
            for replacement in ALPHABET.iter().map(|b| char::from(*b)) {
                if replacement == symbols[index] {
                    continue;
                }
                let mut typo = symbols.clone();
                typo[index] = replacement;
                let typo: String = typo.into_iter().collect();
                assert!(
                    RecoveryKey::parse(&typo).is_err(),
                    "typo at {index} was accepted"
                );
            }
        }
    }

    #[test]
    fn typo_is_reported_as_checksum_mismatch() {
        let key = RecoveryKey::generate();
        let text = key.to_display_string();
        let first = text.chars().next().expect("non-empty");
        let replacement = if first == '2' { '3' } else { '2' };
        let typo = format!("{replacement}{}", &text[1..]);
        assert_eq!(
            RecoveryKey::parse(&typo).err(),
            Some(RecoveryParseError::ChecksumMismatch)
        );
    }

    #[test]
    fn rejects_bad_characters_and_lengths() {
        let key = RecoveryKey::generate();
        let text = key.to_display_string();
        assert_eq!(
            RecoveryKey::parse(&text.replacen(|c: char| c != '-', "U", 1)).err(),
            Some(RecoveryParseError::InvalidCharacter)
        );
        assert_eq!(
            RecoveryKey::parse(&text[..text.len() - 1]).err(),
            Some(RecoveryParseError::WrongLength)
        );
        assert_eq!(
            RecoveryKey::parse("").err(),
            Some(RecoveryParseError::WrongLength)
        );
        assert_eq!(
            RecoveryKey::parse(&format!("{}0", text.as_str())).err(),
            Some(RecoveryParseError::WrongLength)
        );
    }

    #[test]
    fn wrapping_key_depends_on_secret_and_kind() {
        let a = RecoveryKey::generate();
        let b = RecoveryKey::generate();
        assert_eq!(
            a.wrapping_key(RecoveryKind::Account),
            a.wrapping_key(RecoveryKind::Account)
        );
        assert_ne!(
            a.wrapping_key(RecoveryKind::Account),
            a.wrapping_key(RecoveryKind::Vault)
        );
        assert_ne!(
            a.wrapping_key(RecoveryKind::Account),
            b.wrapping_key(RecoveryKind::Account)
        );
    }

    #[test]
    fn debug_is_redacted() {
        assert_eq!(
            format!("{:?}", RecoveryKey::generate()),
            "RecoveryKey(<redacted>)"
        );
    }
}
