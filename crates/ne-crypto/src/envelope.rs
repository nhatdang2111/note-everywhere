//! Sync update envelopes.
//!
//! One envelope carries one encrypted CRDT update. Layout:
//!
//! ```text
//! | version (1) | nonce (24) | ciphertext (n) | tag (16) |
//! ```
//!
//! This is the [`aead`] layout, so the overhead is 41 bytes per
//! update. The workspace, document and device identifiers are not stored in
//! the envelope (the server already knows them); they are authenticated as
//! associated data:
//!
//! ```text
//! "note-everywhere/v1/sync-update"
//!   | len(workspace_id) u16 BE | workspace_id
//!   | len(doc_id)       u16 BE | doc_id
//!   | len(device_id)    u16 BE | device_id
//! ```
//!
//! Length prefixes make the encoding injective, so `("ab", "c")` and
//! `("a", "bc")` can never collide. An envelope therefore opens only under the
//! exact workspace, document and device it was sealed for, which stops a
//! malicious server replaying an update into another document.
//!
//! The version byte is the single format version of both the envelope and the
//! AEAD layout; any change to either bumps it.

use crate::aead::{self, AeadError};
use crate::keys::{SecretKey, WorkspaceKey};

const AAD_DOMAIN: &[u8] = b"note-everywhere/v1/sync-update";

/// Failure to seal or open an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeError {
    /// An identifier is longer than 65535 bytes.
    IdTooLong,
    /// Encryption or authentication failed (opaque on purpose).
    Crypto,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::IdTooLong => "identifier too long",
            Self::Crypto => "authenticated encryption failed",
        })
    }
}

impl std::error::Error for EnvelopeError {}

impl From<AeadError> for EnvelopeError {
    fn from(_: AeadError) -> Self {
        Self::Crypto
    }
}

fn associated_data(
    workspace_id: &str,
    doc_id: &str,
    device_id: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    let mut aad = AAD_DOMAIN.to_vec();
    for id in [workspace_id, doc_id, device_id] {
        let len = u16::try_from(id.len()).map_err(|_| EnvelopeError::IdTooLong)?;
        aad.extend_from_slice(&len.to_be_bytes());
        aad.extend_from_slice(id.as_bytes());
    }
    Ok(aad)
}

/// Encrypts one update for `(workspace_id, doc_id, device_id)`.
pub fn seal_update(
    workspace_key: &WorkspaceKey,
    workspace_id: &str,
    doc_id: &str,
    device_id: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    let aad = associated_data(workspace_id, doc_id, device_id)?;
    Ok(aead::seal(workspace_key.as_bytes(), &aad, plaintext)?)
}

/// Decrypts an envelope; fails unless all three identifiers match those it was
/// sealed with.
pub fn open_update(
    workspace_key: &WorkspaceKey,
    workspace_id: &str,
    doc_id: &str,
    device_id: &str,
    envelope: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    let aad = associated_data(workspace_id, doc_id, device_id)?;
    Ok(aead::open(workspace_key.as_bytes(), &aad, envelope)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = WorkspaceKey::generate();
        let envelope = seal_update(&key, "ws", "doc", "dev", b"update").expect("seal");
        assert_eq!(envelope.len(), 6 + aead::OVERHEAD);
        let plain = open_update(&key, "ws", "doc", "dev", &envelope).expect("open");
        assert_eq!(plain, b"update");
    }

    #[test]
    fn rejects_other_document_workspace_or_device() {
        let key = WorkspaceKey::generate();
        let envelope = seal_update(&key, "ws", "doc-a", "dev", b"update").expect("seal");
        for (ws, doc, dev) in [
            ("ws", "doc-b", "dev"),
            ("other", "doc-a", "dev"),
            ("ws", "doc-a", "other"),
        ] {
            assert_eq!(
                open_update(&key, ws, doc, dev, &envelope),
                Err(EnvelopeError::Crypto)
            );
        }
    }

    #[test]
    fn identifier_boundaries_cannot_be_shifted() {
        let key = WorkspaceKey::generate();
        let envelope = seal_update(&key, "ab", "c", "d", b"x").expect("seal");
        assert_eq!(
            open_update(&key, "a", "bc", "d", &envelope),
            Err(EnvelopeError::Crypto)
        );
    }

    #[test]
    fn rejects_wrong_key_and_tampering() {
        let key = WorkspaceKey::generate();
        let mut envelope = seal_update(&key, "ws", "doc", "dev", b"update").expect("seal");
        assert!(open_update(&WorkspaceKey::generate(), "ws", "doc", "dev", &envelope).is_err());
        let last = envelope.len() - 1;
        envelope[last] ^= 1;
        assert!(open_update(&key, "ws", "doc", "dev", &envelope).is_err());
    }

    #[test]
    fn oversized_identifier_is_rejected() {
        let key = WorkspaceKey::generate();
        let long = "x".repeat(70_000);
        assert_eq!(
            seal_update(&key, &long, "doc", "dev", b"u"),
            Err(EnvelopeError::IdTooLong)
        );
    }
}
