//! Random password and passphrase generation.
//!
//! All randomness comes from the operating system through `getrandom`, with
//! rejection sampling so no value is favoured.
//!
//! # Word list
//!
//! Passphrases draw from the 2048-word English list of BIP-39
//! (<https://github.com/bitcoin/bips/blob/master/bip-0039/english.txt>),
//! published under the MIT licence (the BIP states "This BIP falls under the
//! MIT License."). Its words are 3 to 8 letters long and unique in their first
//! four letters, which makes typos easy to spot. The file is embedded
//! unchanged as `data/wordlist-en.txt`.

use std::sync::OnceLock;

use zeroize::{Zeroize, Zeroizing};

/// Shortest generated password.
pub const MIN_PASSWORD_LENGTH: usize = 4;
/// Longest generated password.
pub const MAX_PASSWORD_LENGTH: usize = 256;
/// Fewest words in a passphrase.
pub const MIN_WORDS: usize = 3;
/// Most words in a passphrase.
pub const MAX_WORDS: usize = 32;
/// Longest passphrase separator, in characters.
pub const MAX_SEPARATOR_CHARS: usize = 8;

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.<>/?~";
/// Characters that are easy to confuse when read or typed.
const AMBIGUOUS: &[u8] = b"O0Il1|";
const MAX_ATTEMPTS: usize = 100_000;

static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();

/// The embedded passphrase word list.
pub fn word_list() -> &'static [&'static str] {
    WORDS.get_or_init(|| {
        include_str!("../data/wordlist-en.txt")
            .lines()
            .filter(|w| !w.is_empty())
            .collect()
    })
}

/// Why generation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GeneratorError {
    /// Every character class is switched off.
    #[error("at least one character class must be enabled")]
    NoCharacterClass,
    /// The length is outside the allowed range.
    #[error("password length must be {MIN_PASSWORD_LENGTH} to {MAX_PASSWORD_LENGTH}")]
    LengthOutOfRange,
    /// The word count is outside the allowed range.
    #[error("passphrase word count must be {MIN_WORDS} to {MAX_WORDS}")]
    WordCountOutOfRange,
    /// The separator is too long or contains control characters.
    #[error("passphrase separator must be at most {MAX_SEPARATOR_CHARS} printable characters")]
    InvalidSeparator,
    /// Sampling kept missing a class; practically unreachable.
    #[error("could not satisfy the character class requirements")]
    Exhausted,
}

/// A generated secret and how strong it is. The value is wiped on drop.
pub struct Generated {
    /// The password or passphrase.
    pub value: Zeroizing<String>,
    /// Bits of entropy: log2 of the number of equally likely outputs for the
    /// chosen settings. Does not count knowledge an attacker has of the
    /// settings themselves.
    pub entropy_bits: f64,
}

impl std::fmt::Debug for Generated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Generated({:.1} bits, <redacted>)", self.entropy_bits)
    }
}

/// Settings for [`generate_password`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordSpec {
    /// Number of characters.
    pub length: usize,
    /// Include `a-z`.
    pub lowercase: bool,
    /// Include `A-Z`.
    pub uppercase: bool,
    /// Include `0-9`.
    pub digits: bool,
    /// Include punctuation and symbols.
    pub symbols: bool,
    /// Leave out `O 0 I l 1 |`.
    pub exclude_ambiguous: bool,
}

impl Default for PasswordSpec {
    fn default() -> Self {
        Self {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            exclude_ambiguous: false,
        }
    }
}

/// Settings for [`generate_passphrase`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassphraseSpec {
    /// Number of words.
    pub words: usize,
    /// Text between words.
    pub separator: String,
    /// Capitalise the first letter of each word.
    pub capitalize: bool,
    /// Append one random digit to one random word.
    pub include_number: bool,
}

impl Default for PassphraseSpec {
    fn default() -> Self {
        Self {
            words: 5,
            separator: "-".into(),
            capitalize: false,
            include_number: false,
        }
    }
}

/// Uniform integer in `0..n` by rejection sampling. `n` must be non-zero.
fn random_below(n: usize) -> usize {
    let n = u32::try_from(n).expect("range fits in u32");
    let limit = u32::MAX - (u32::MAX % n);
    loop {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).expect("operating system random number generator unavailable");
        let x = u32::from_le_bytes(bytes);
        if x < limit {
            return usize::try_from(x % n).expect("below n");
        }
    }
}

fn classes(spec: &PasswordSpec) -> Vec<Vec<u8>> {
    [
        (spec.lowercase, LOWER),
        (spec.uppercase, UPPER),
        (spec.digits, DIGITS),
        (spec.symbols, SYMBOLS),
    ]
    .into_iter()
    .filter(|(enabled, _)| *enabled)
    .map(|(_, set)| {
        set.iter()
            .copied()
            .filter(|c| !(spec.exclude_ambiguous && AMBIGUOUS.contains(c)))
            .collect()
    })
    .collect()
}

/// Generates a password uniformly among all strings of the requested length
/// that contain at least one character of every enabled class (by rejection
/// sampling), and reports the entropy of exactly that set.
pub fn generate_password(spec: &PasswordSpec) -> Result<Generated, GeneratorError> {
    let classes = classes(spec);
    if classes.is_empty() {
        return Err(GeneratorError::NoCharacterClass);
    }
    if !(MIN_PASSWORD_LENGTH..=MAX_PASSWORD_LENGTH).contains(&spec.length) {
        return Err(GeneratorError::LengthOutOfRange);
    }
    let pool: Vec<u8> = classes.iter().flatten().copied().collect();
    let mut attempt = Zeroizing::new(Vec::with_capacity(spec.length));
    for _ in 0..MAX_ATTEMPTS {
        attempt.clear();
        attempt.extend((0..spec.length).map(|_| pool[random_below(pool.len())]));
        if classes
            .iter()
            .all(|c| attempt.iter().any(|b| c.contains(b)))
        {
            let bytes = std::mem::take(&mut *attempt);
            let value = String::from_utf8(bytes).expect("the pools are ASCII");
            return Ok(Generated {
                value: Zeroizing::new(value),
                entropy_bits: password_entropy(&classes, pool.len(), spec.length),
            });
        }
        attempt.zeroize();
    }
    Err(GeneratorError::Exhausted)
}

/// `log2` of the number of strings of `length` over the pool that use every
/// class, by inclusion-exclusion over the classes left out. Terms are scaled
/// by `pool^length` so they stay in `0..=1`.
fn password_entropy(classes: &[Vec<u8>], pool: usize, length: usize) -> f64 {
    let exponent = i32::try_from(length).expect("length is bounded");
    let mut share = 0.0;
    for mask in 0u32..(1 << classes.len()) {
        let removed: usize = classes
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, c)| c.len())
            .sum();
        let term = ((pool - removed) as f64 / pool as f64).powi(exponent);
        share += if mask.count_ones() % 2 == 0 {
            term
        } else {
            -term
        };
    }
    length as f64 * (pool as f64).log2() + share.max(f64::MIN_POSITIVE).log2()
}

/// Generates a passphrase of random words from [`word_list`].
pub fn generate_passphrase(spec: &PassphraseSpec) -> Result<Generated, GeneratorError> {
    if !(MIN_WORDS..=MAX_WORDS).contains(&spec.words) {
        return Err(GeneratorError::WordCountOutOfRange);
    }
    if spec.separator.chars().count() > MAX_SEPARATOR_CHARS
        || spec.separator.chars().any(char::is_control)
    {
        return Err(GeneratorError::InvalidSeparator);
    }
    let list = word_list();
    let mut words: Vec<String> = (0..spec.words)
        .map(|_| list[random_below(list.len())].to_owned())
        .collect();
    let mut entropy = spec.words as f64 * (list.len() as f64).log2();
    if spec.capitalize {
        words.iter_mut().for_each(|w| {
            if let Some(first) = w.get_mut(..1) {
                first.make_ascii_uppercase();
            }
        });
    }
    if spec.include_number {
        let at = random_below(words.len());
        words[at].push(char::from(
            b'0' + u8::try_from(random_below(10)).expect("a digit"),
        ));
        entropy += (words.len() as f64 * 10.0).log2();
    }
    let value = Zeroizing::new(words.join(&spec.separator));
    words.iter_mut().for_each(Zeroize::zeroize);
    Ok(Generated {
        value,
        entropy_bits: entropy,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use proptest::prelude::*;

    use super::*;

    #[test]
    fn word_list_is_2048_unique_lowercase_words() {
        let words = word_list();
        assert_eq!(words.len(), 2048);
        assert_eq!(words.iter().collect::<HashSet<_>>().len(), 2048);
        assert!(
            words
                .iter()
                .all(|w| (3..=8).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_lowercase()))
        );
    }

    #[test]
    fn invalid_specs_are_rejected() {
        let off = PasswordSpec {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
            ..PasswordSpec::default()
        };
        assert_eq!(
            generate_password(&off).err(),
            Some(GeneratorError::NoCharacterClass)
        );
        for length in [0, 3, 257] {
            let spec = PasswordSpec {
                length,
                ..PasswordSpec::default()
            };
            assert_eq!(
                generate_password(&spec).err(),
                Some(GeneratorError::LengthOutOfRange)
            );
        }
        let words = |n| PassphraseSpec {
            words: n,
            ..PassphraseSpec::default()
        };
        assert!(generate_passphrase(&words(2)).is_err());
        assert!(generate_passphrase(&words(33)).is_err());
        let sep = PassphraseSpec {
            separator: "x".repeat(9),
            ..PassphraseSpec::default()
        };
        assert_eq!(
            generate_passphrase(&sep).err(),
            Some(GeneratorError::InvalidSeparator)
        );
        let ctl = PassphraseSpec {
            separator: "\n".into(),
            ..PassphraseSpec::default()
        };
        assert!(generate_passphrase(&ctl).is_err());
    }

    #[test]
    fn entropy_matches_hand_computation() {
        // One class of 26 letters: every string qualifies, so log2(26^20).
        let spec = PasswordSpec {
            length: 20,
            uppercase: false,
            digits: false,
            symbols: false,
            ..PasswordSpec::default()
        };
        let bits = generate_password(&spec).expect("valid").entropy_bits;
        assert!((bits - 20.0 * 26f64.log2()).abs() < 1e-9);
        // Four words from 2048 = 44 bits, separator adds nothing.
        let phrase = generate_passphrase(&PassphraseSpec {
            words: 4,
            ..PassphraseSpec::default()
        })
        .expect("valid");
        assert!((phrase.entropy_bits - 44.0).abs() < 1e-9);
        let numbered = generate_passphrase(&PassphraseSpec {
            words: 4,
            include_number: true,
            ..PassphraseSpec::default()
        })
        .expect("valid");
        assert!((numbered.entropy_bits - (44.0 + 40f64.log2())).abs() < 1e-9);
    }

    #[test]
    fn class_guarantee_costs_entropy_only_when_it_binds() {
        let all = PasswordSpec::default();
        let pool = (26 + 26 + 10 + SYMBOLS.len()) as f64;
        let unconstrained = 20.0 * pool.log2();
        let bits = generate_password(&all).expect("valid").entropy_bits;
        assert!(bits < unconstrained && bits > unconstrained - 1.0);
    }

    #[test]
    fn passphrase_shape() {
        let spec = PassphraseSpec {
            words: 6,
            separator: "::".into(),
            capitalize: true,
            include_number: true,
        };
        let phrase = generate_passphrase(&spec).expect("valid");
        let parts: Vec<&str> = phrase.value.split("::").collect();
        assert_eq!(parts.len(), 6);
        assert!(
            parts
                .iter()
                .all(|p| p.starts_with(|c: char| c.is_ascii_uppercase()))
        );
        assert_eq!(phrase.value.chars().filter(char::is_ascii_digit).count(), 1);
    }

    #[test]
    fn random_below_is_roughly_uniform() {
        let mut counts = [0usize; 7];
        for _ in 0..7000 {
            counts[random_below(7)] += 1;
        }
        assert!(
            counts.iter().all(|&c| (800..1200).contains(&c)),
            "{counts:?}"
        );
    }

    proptest! {
        #[test]
        fn passwords_obey_their_spec(
            length in MIN_PASSWORD_LENGTH..=64usize,
            lowercase in any::<bool>(),
            uppercase in any::<bool>(),
            digits in any::<bool>(),
            symbols in any::<bool>(),
            exclude_ambiguous in any::<bool>(),
        ) {
            let spec = PasswordSpec { length, lowercase, uppercase, digits, symbols, exclude_ambiguous };
            if !(lowercase || uppercase || digits || symbols) {
                prop_assert_eq!(generate_password(&spec).err(), Some(GeneratorError::NoCharacterClass));
            } else {
                let generated = generate_password(&spec).expect("valid spec");
                let text = generated.value.as_str();
                prop_assert_eq!(text.len(), length);
                prop_assert_eq!(text.chars().any(|c| c.is_ascii_lowercase()), lowercase);
                prop_assert_eq!(text.chars().any(|c| c.is_ascii_uppercase()), uppercase);
                prop_assert_eq!(text.chars().any(|c| c.is_ascii_digit()), digits);
                prop_assert_eq!(text.bytes().any(|b| SYMBOLS.contains(&b)), symbols);
                if exclude_ambiguous {
                    prop_assert!(!text.bytes().any(|b| AMBIGUOUS.contains(&b)));
                }
                prop_assert!(generated.entropy_bits > 0.0);
                prop_assert!(generated.entropy_bits <= length as f64 * 94f64.log2());
            }
        }

        #[test]
        fn passphrases_use_listed_words(words in MIN_WORDS..=MAX_WORDS) {
            let spec = PassphraseSpec { words, ..PassphraseSpec::default() };
            let phrase = generate_passphrase(&spec).expect("valid");
            let list = word_list();
            let parts: Vec<&str> = phrase.value.split('-').collect();
            prop_assert_eq!(parts.len(), words);
            prop_assert!(parts.iter().all(|p| list.contains(p)));
        }
    }
}
