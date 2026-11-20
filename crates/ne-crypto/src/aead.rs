//! XChaCha20-Poly1305 sealing with a random nonce and mandatory associated data.
//!
//! Sealed layout:
//!
//! ```text
//! | version (1) | nonce (24) | ciphertext (n) | tag (16) |
//! ```
//!
//! `version` is currently `1`. The associated data is not stored; the opener
//! must supply the same bytes. Every failure on the opening side (wrong key,
//! wrong associated data, truncation, tampering, unknown version) returns the
//! same opaque [`AeadError`].

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};

use crate::random;

/// Current sealed-format version byte.
pub const VERSION: u8 = 1;
/// XChaCha20 nonce length.
pub const NONCE_LEN: usize = 24;
/// Poly1305 tag length.
pub const TAG_LEN: usize = 16;
/// Bytes added to every plaintext by [`seal`].
pub const OVERHEAD: usize = 1 + NONCE_LEN + TAG_LEN;

/// Opaque failure of sealing or opening. Deliberately carries no detail so it
/// cannot act as an oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AeadError;

impl std::fmt::Display for AeadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("authenticated encryption failed")
    }
}

impl std::error::Error for AeadError {}

/// Encrypts with an explicit nonce and returns `ciphertext || tag`.
///
/// The caller must guarantee that `(key, nonce)` is never reused.
pub(crate) fn seal_with_nonce(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, AeadError> {
    XChaCha20Poly1305::new(&Array(*key))
        .encrypt(
            &Array(*nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| AeadError)
}

/// Decrypts `ciphertext || tag` produced by [`seal_with_nonce`].
pub(crate) fn open_with_nonce(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext_and_tag: &[u8],
) -> Result<Vec<u8>, AeadError> {
    XChaCha20Poly1305::new(&Array(*key))
        .decrypt(
            &Array(*nonce),
            Payload {
                msg: ciphertext_and_tag,
                aad,
            },
        )
        .map_err(|_| AeadError)
}

/// Encrypts `plaintext` under `key`, binding `aad`.
pub fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AeadError> {
    let nonce = random::array::<NONCE_LEN>();
    let body = seal_with_nonce(key, &nonce, aad, plaintext)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + body.len());
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decrypts the output of [`seal`].
pub fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AeadError> {
    if sealed.len() < OVERHEAD || sealed[0] != VERSION {
        return Err(AeadError);
    }
    let (nonce, body) = sealed[1..].split_at(NONCE_LEN);
    let nonce: &[u8; NONCE_LEN] = nonce.try_into().map_err(|_| AeadError)?;
    open_with_nonce(key, nonce, aad, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [0x11; 32];

    /// XChaCha20-Poly1305 test vector from draft-irtf-cfrg-xchacha-03,
    /// appendix A.3.1.
    #[test]
    fn xchacha20_poly1305_known_answer() {
        let key: Vec<u8> = (0x80u8..=0x9f).collect();
        let key: [u8; 32] = key.try_into().expect("32 bytes");
        let nonce: Vec<u8> = (0x40u8..=0x57).collect();
        let nonce: [u8; 24] = nonce.try_into().expect("24 bytes");
        let aad = [
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
        ];
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let expected_ciphertext = hex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb\
             731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452\
             2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9\
             21f9664c97637da9768812f615c68b13b52e",
        );
        let expected_tag = hex("c0875924c1c7987947deafd8780acf49");

        let sealed = seal_with_nonce(&key, &nonce, &aad, plaintext).expect("seal");
        let (ciphertext, tag) = sealed.split_at(sealed.len() - TAG_LEN);
        assert_eq!(ciphertext, expected_ciphertext.as_slice());
        assert_eq!(tag, expected_tag.as_slice());
        let opened = open_with_nonce(&key, &nonce, &aad, &sealed).expect("open");
        assert_eq!(opened, plaintext);
    }

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        digits
            .chunks(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).expect("ascii");
                u8::from_str_radix(pair, 16).expect("hex")
            })
            .collect()
    }

    #[test]
    fn round_trip_and_layout() {
        let sealed = seal(&KEY, b"ctx", b"hello").expect("seal");
        assert_eq!(sealed[0], VERSION);
        assert_eq!(sealed.len(), 5 + OVERHEAD);
        assert_eq!(open(&KEY, b"ctx", &sealed).expect("open"), b"hello");
    }

    #[test]
    fn empty_plaintext_round_trips() {
        let sealed = seal(&KEY, b"", b"").expect("seal");
        assert_eq!(sealed.len(), OVERHEAD);
        assert!(open(&KEY, b"", &sealed).expect("open").is_empty());
    }

    #[test]
    fn nonces_are_fresh() {
        let first = seal(&KEY, b"a", b"same").expect("seal");
        let second = seal(&KEY, b"a", b"same").expect("seal");
        assert_ne!(first, second);
    }

    #[test]
    fn wrong_key_wrong_aad_and_bad_input_all_fail_identically() {
        let sealed = seal(&KEY, b"ctx", b"hello").expect("seal");
        assert_eq!(open(&[0x22; 32], b"ctx", &sealed), Err(AeadError));
        assert_eq!(open(&KEY, b"other", &sealed), Err(AeadError));
        assert_eq!(
            open(&KEY, b"ctx", &sealed[..sealed.len() - 1]),
            Err(AeadError)
        );
        assert_eq!(open(&KEY, b"ctx", &sealed[..OVERHEAD - 1]), Err(AeadError));
        assert_eq!(open(&KEY, b"ctx", &[]), Err(AeadError));

        let mut unknown_version = sealed.clone();
        unknown_version[0] = 2;
        assert_eq!(open(&KEY, b"ctx", &unknown_version), Err(AeadError));
    }

    #[test]
    fn any_flipped_bit_is_detected() {
        let sealed = seal(&KEY, b"ctx", b"hello world").expect("seal");
        for index in 0..sealed.len() {
            let mut tampered = sealed.clone();
            tampered[index] ^= 0x01;
            assert_eq!(
                open(&KEY, b"ctx", &tampered),
                Err(AeadError),
                "byte {index}"
            );
        }
    }
}
