//! Chunked encryption for attachments of any size.
//!
//! Layout:
//!
//! ```text
//! | version (1) | stream salt (32) | chunk 0 | chunk 1 | ... | final chunk |
//! chunk = ciphertext (1..=65536 bytes, or 0 for an empty stream) | tag (16)
//! ```
//!
//! Every chunk except the last carries exactly [`CHUNK_SIZE`] plaintext
//! bytes; the last carries 0 to [`CHUNK_SIZE`] (an empty stream is a single
//! empty final chunk, and an exact multiple of the chunk size ends with a full
//! final chunk).
//!
//! * A per-stream subkey is derived with HKDF-SHA256 from the workspace key and
//!   the random stream salt, so the same workspace key can safely encrypt any
//!   number of streams.
//! * The 24-byte nonce of a chunk is `15 zero bytes | chunk index u64 BE |
//!   final flag u8`. Within one subkey every nonce is therefore unique.
//! * The caller's associated data, the chunk index and the final flag are also
//!   authenticated, which detects reordering, dropped or duplicated chunks, and
//!   truncation at a chunk boundary (the new last chunk was not sealed as
//!   final). Binding caller data such as an attachment id stops a stream being
//!   swapped for another attachment.
//!
//! Decryption writes each chunk only after it authenticates, but a truncated or
//! tampered stream is only noticed when the bad chunk is reached, so callers
//! must discard the output (for example a temporary file) if an error is
//! returned.

use std::io::{self, Read, Write};

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroize;

use crate::aead::{self, AeadError, TAG_LEN};
use crate::keys::{SecretKey, WorkspaceKey};
use crate::random;

/// Plaintext bytes per chunk.
pub const CHUNK_SIZE: usize = 64 * 1024;
/// Stream format version byte.
pub const VERSION: u8 = 1;
/// Length of the per-stream random salt.
pub const SALT_LEN: usize = 32;
/// Length of the stream header.
pub const HEADER_LEN: usize = 1 + SALT_LEN;

const SUBKEY_INFO: &[u8] = b"note-everywhere/v1/stream-key";
const AAD_DOMAIN: &[u8] = b"note-everywhere/v1/stream-chunk";
const SEALED_CHUNK: usize = CHUNK_SIZE + TAG_LEN;

/// Failure of a stream operation.
#[derive(Debug)]
pub enum StreamError {
    /// Reading or writing failed.
    Io(io::Error),
    /// Authentication failed or the stream is malformed (opaque on purpose).
    Crypto,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "stream i/o error: {err}"),
            Self::Crypto => f.write_str("authenticated encryption failed"),
        }
    }
}

impl std::error::Error for StreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Crypto => None,
        }
    }
}

impl From<io::Error> for StreamError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<AeadError> for StreamError {
    fn from(_: AeadError) -> Self {
        Self::Crypto
    }
}

struct ChunkCipher {
    subkey: [u8; 32],
    aad_prefix: Vec<u8>,
}

impl ChunkCipher {
    fn new(key: &WorkspaceKey, salt: &[u8; SALT_LEN], caller_aad: &[u8]) -> Self {
        let mut subkey = [0u8; 32];
        Hkdf::<Sha256>::new(Some(salt), key.as_bytes())
            .expand(SUBKEY_INFO, &mut subkey)
            .expect("32-byte HKDF output is within limits");
        let mut aad_prefix = AAD_DOMAIN.to_vec();
        aad_prefix.extend_from_slice(&(caller_aad.len() as u64).to_be_bytes());
        aad_prefix.extend_from_slice(caller_aad);
        Self { subkey, aad_prefix }
    }

    fn nonce_and_aad(&self, index: u64, last: bool) -> ([u8; 24], Vec<u8>) {
        let mut nonce = [0u8; 24];
        nonce[15..23].copy_from_slice(&index.to_be_bytes());
        nonce[23] = u8::from(last);
        let mut aad = self.aad_prefix.clone();
        aad.extend_from_slice(&index.to_be_bytes());
        aad.push(u8::from(last));
        (nonce, aad)
    }

    fn seal(&self, index: u64, last: bool, plaintext: &[u8]) -> Result<Vec<u8>, AeadError> {
        let (nonce, aad) = self.nonce_and_aad(index, last);
        aead::seal_with_nonce(&self.subkey, &nonce, &aad, plaintext)
    }

    fn open(&self, index: u64, last: bool, sealed: &[u8]) -> Result<Vec<u8>, AeadError> {
        let (nonce, aad) = self.nonce_and_aad(index, last);
        aead::open_with_nonce(&self.subkey, &nonce, &aad, sealed)
    }
}

impl Drop for ChunkCipher {
    fn drop(&mut self) {
        self.subkey.zeroize();
    }
}

/// Reads until `buf` is full or the reader is exhausted; returns bytes read.
fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(filled)
}

fn next_index(index: u64) -> Result<u64, StreamError> {
    index.checked_add(1).ok_or(StreamError::Crypto)
}

/// Encrypts everything from `reader` into `writer`, binding `aad`.
pub fn encrypt_stream<R: Read, W: Write>(
    key: &WorkspaceKey,
    aad: &[u8],
    mut reader: R,
    mut writer: W,
) -> Result<(), StreamError> {
    let salt = random::array::<SALT_LEN>();
    let cipher = ChunkCipher::new(key, &salt, aad);
    writer.write_all(&[VERSION])?;
    writer.write_all(&salt)?;

    let mut current = vec![0u8; CHUNK_SIZE];
    let mut ahead = vec![0u8; CHUNK_SIZE];
    let mut current_len = read_full(&mut reader, &mut current)?;
    let mut index = 0u64;
    loop {
        // A short read means end of input; a full chunk needs a look-ahead to
        // know whether it is the final one.
        let ahead_len = if current_len == CHUNK_SIZE {
            read_full(&mut reader, &mut ahead)?
        } else {
            0
        };
        let last = ahead_len == 0;
        writer.write_all(&cipher.seal(index, last, &current[..current_len])?)?;
        if last {
            return Ok(());
        }
        std::mem::swap(&mut current, &mut ahead);
        current_len = ahead_len;
        index = next_index(index)?;
    }
}

/// Decrypts a stream produced by [`encrypt_stream`] with the same `key` and `aad`.
pub fn decrypt_stream<R: Read, W: Write>(
    key: &WorkspaceKey,
    aad: &[u8],
    mut reader: R,
    mut writer: W,
) -> Result<(), StreamError> {
    let mut header = [0u8; HEADER_LEN];
    if read_full(&mut reader, &mut header)? != HEADER_LEN || header[0] != VERSION {
        return Err(StreamError::Crypto);
    }
    let salt: [u8; SALT_LEN] = header[1..].try_into().map_err(|_| StreamError::Crypto)?;
    let cipher = ChunkCipher::new(key, &salt, aad);

    let mut current = vec![0u8; SEALED_CHUNK];
    let mut ahead = vec![0u8; SEALED_CHUNK];
    let mut current_len = read_full(&mut reader, &mut current)?;
    let mut index = 0u64;
    loop {
        if current_len < TAG_LEN {
            // Covers a stream with no chunks at all and a cut inside a tag.
            return Err(StreamError::Crypto);
        }
        let ahead_len = if current_len == SEALED_CHUNK {
            read_full(&mut reader, &mut ahead)?
        } else {
            0
        };
        let last = ahead_len == 0;
        let plain = cipher.open(index, last, &current[..current_len])?;
        writer.write_all(&plain)?;
        if last {
            return Ok(());
        }
        std::mem::swap(&mut current, &mut ahead);
        current_len = ahead_len;
        index = next_index(index)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn encrypt(key: &WorkspaceKey, aad: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encrypt_stream(key, aad, data, &mut out).expect("encrypt");
        out
    }

    fn decrypt(key: &WorkspaceKey, aad: &[u8], data: &[u8]) -> Result<Vec<u8>, StreamError> {
        let mut out = Vec::new();
        decrypt_stream(key, aad, data, &mut out)?;
        Ok(out)
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn sealed_len(plain_len: usize) -> usize {
        let chunks = plain_len.div_ceil(CHUNK_SIZE).max(1);
        HEADER_LEN + plain_len + chunks * TAG_LEN
    }

    #[test]
    fn round_trips_at_boundary_sizes() {
        let key = WorkspaceKey::generate();
        for len in [
            0,
            1,
            CHUNK_SIZE - 1,
            CHUNK_SIZE,
            CHUNK_SIZE + 1,
            2 * CHUNK_SIZE,
            2 * CHUNK_SIZE + 17,
        ] {
            let data = pattern(len);
            let sealed = encrypt(&key, b"att-1", &data);
            assert_eq!(sealed.len(), sealed_len(len), "len {len}");
            assert_eq!(
                decrypt(&key, b"att-1", &sealed).expect("decrypt"),
                data,
                "len {len}"
            );
        }
    }

    #[test]
    fn wrong_key_or_aad_fails() {
        let key = WorkspaceKey::generate();
        let sealed = encrypt(&key, b"att-1", &pattern(1000));
        assert!(matches!(
            decrypt(&WorkspaceKey::generate(), b"att-1", &sealed),
            Err(StreamError::Crypto)
        ));
        assert!(matches!(
            decrypt(&key, b"att-2", &sealed),
            Err(StreamError::Crypto)
        ));
    }

    #[test]
    fn truncation_is_detected_everywhere() {
        let key = WorkspaceKey::generate();
        let sealed = encrypt(&key, b"a", &pattern(2 * CHUNK_SIZE + 5));
        // Cut at a chunk boundary, mid-chunk, inside the header, and empty.
        let boundary = HEADER_LEN + 2 * SEALED_CHUNK;
        for cut in [
            boundary,
            boundary - 1,
            HEADER_LEN + SEALED_CHUNK,
            HEADER_LEN + 3,
            10,
            0,
        ] {
            assert!(
                matches!(
                    decrypt(&key, b"a", &sealed[..cut]),
                    Err(StreamError::Crypto)
                ),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn truncation_of_an_exact_multiple_is_detected() {
        let key = WorkspaceKey::generate();
        let sealed = encrypt(&key, b"a", &pattern(2 * CHUNK_SIZE));
        let cut = HEADER_LEN + SEALED_CHUNK;
        assert!(matches!(
            decrypt(&key, b"a", &sealed[..cut]),
            Err(StreamError::Crypto)
        ));
    }

    #[test]
    fn appended_data_is_detected() {
        let key = WorkspaceKey::generate();
        let mut sealed = encrypt(&key, b"a", &pattern(100));
        sealed.extend_from_slice(&[0u8; 40]);
        assert!(matches!(
            decrypt(&key, b"a", &sealed),
            Err(StreamError::Crypto)
        ));
    }

    #[test]
    fn reordered_or_duplicated_chunks_are_detected() {
        let key = WorkspaceKey::generate();
        let sealed = encrypt(&key, b"a", &pattern(3 * CHUNK_SIZE + 9));
        let chunk =
            |i: usize| &sealed[HEADER_LEN + i * SEALED_CHUNK..HEADER_LEN + (i + 1) * SEALED_CHUNK];
        let tail = &sealed[HEADER_LEN + 3 * SEALED_CHUNK..];

        let mut swapped = sealed[..HEADER_LEN].to_vec();
        for part in [chunk(1), chunk(0), chunk(2), tail] {
            swapped.extend_from_slice(part);
        }
        assert!(matches!(
            decrypt(&key, b"a", &swapped),
            Err(StreamError::Crypto)
        ));

        let mut duplicated = sealed[..HEADER_LEN].to_vec();
        for part in [chunk(0), chunk(0), chunk(1), chunk(2), tail] {
            duplicated.extend_from_slice(part);
        }
        assert!(matches!(
            decrypt(&key, b"a", &duplicated),
            Err(StreamError::Crypto)
        ));
    }

    #[test]
    fn chunks_cannot_move_between_streams() {
        let key = WorkspaceKey::generate();
        let first = encrypt(&key, b"a", &pattern(CHUNK_SIZE + 3));
        let second = encrypt(&key, b"a", &pattern(CHUNK_SIZE + 3));
        let mut mixed = first[..HEADER_LEN + SEALED_CHUNK].to_vec();
        mixed.extend_from_slice(&second[HEADER_LEN + SEALED_CHUNK..]);
        assert!(matches!(
            decrypt(&key, b"a", &mixed),
            Err(StreamError::Crypto)
        ));
    }

    #[test]
    fn bad_version_and_flipped_bits_fail() {
        let key = WorkspaceKey::generate();
        let sealed = encrypt(&key, b"a", &pattern(300));
        let mut wrong_version = sealed.clone();
        wrong_version[0] = 9;
        assert!(decrypt(&key, b"a", &wrong_version).is_err());
        for index in (0..sealed.len()).step_by(7) {
            let mut tampered = sealed.clone();
            tampered[index] ^= 1;
            assert!(decrypt(&key, b"a", &tampered).is_err(), "byte {index}");
        }
    }

    #[test]
    fn same_plaintext_encrypts_differently_each_time() {
        let key = WorkspaceKey::generate();
        assert_ne!(encrypt(&key, b"a", b"data"), encrypt(&key, b"a", b"data"));
    }

    /// A reader that returns a few bytes at a time exercises partial reads.
    struct Trickle<'a>(&'a [u8]);

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.0.len()).min(1021);
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    #[test]
    fn partial_reads_are_handled() {
        let key = WorkspaceKey::generate();
        let data = pattern(CHUNK_SIZE + 5000);
        let mut sealed = Vec::new();
        encrypt_stream(&key, b"a", Trickle(&data), &mut sealed).expect("encrypt");
        let mut out = Vec::new();
        decrypt_stream(&key, b"a", Trickle(&sealed), &mut out).expect("decrypt");
        assert_eq!(out, data);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn stream_round_trips_for_random_sizes(
            len in prop_oneof![
                0usize..300,
                Just(CHUNK_SIZE),
                Just(2 * CHUNK_SIZE),
                (CHUNK_SIZE - 3)..(CHUNK_SIZE + 3),
                0usize..(3 * CHUNK_SIZE),
            ],
            seed in any::<u8>(),
            aad in proptest::collection::vec(any::<u8>(), 0..16),
        ) {
            let data: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_add(seed)).collect();
            let key = WorkspaceKey::generate();
            let sealed = encrypt(&key, &aad, &data);
            prop_assert_eq!(decrypt(&key, &aad, &sealed).expect("decrypt"), data);
        }
    }
}
