//! Wrapping one 32-byte key under another.
//!
//! A [`WrappedKey`] is an [`aead`] blob whose associated data is
//! a fixed domain string followed by the [`WrapContext`] label. Because the
//! label is authenticated, a wrapped vault key cannot be passed off as a
//! wrapped root key, nor a password-wrapped blob as a recovery-wrapped one.
//!
//! Serialised form (also the serde form, as unpadded URL-safe base64):
//!
//! ```text
//! | aead version (1) | nonce (24) | wrapped key (32) | tag (16) |   = 73 bytes
//! ```

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroize;

use crate::aead::{self, AeadError, OVERHEAD};
use crate::keys::{KEY_LEN, SecretKey};
use crate::serde_b64;

const WRAP_DOMAIN: &[u8] = b"note-everywhere/v1/key-wrap/";

/// Serialised length of every [`WrappedKey`].
pub const WRAPPED_LEN: usize = OVERHEAD + KEY_LEN;

/// What a wrapped key is for. Bound into the associated data of the wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapContext {
    /// Account root key wrapped by the password-derived key.
    RootKeyByPassword,
    /// Account root key wrapped by the account recovery key.
    RootKeyByRecovery,
    /// Workspace key wrapped by the account root key.
    WorkspaceKey,
    /// Vault key wrapped by the vault-password-derived key.
    VaultKeyByPassword,
    /// Vault key wrapped by the vault recovery key.
    VaultKeyByRecovery,
}

impl WrapContext {
    fn label(self) -> &'static [u8] {
        match self {
            Self::RootKeyByPassword => b"root-key/password",
            Self::RootKeyByRecovery => b"root-key/recovery",
            Self::WorkspaceKey => b"workspace-key/root",
            Self::VaultKeyByPassword => b"vault-key/password",
            Self::VaultKeyByRecovery => b"vault-key/recovery",
        }
    }

    fn aad(self) -> Vec<u8> {
        let mut aad = WRAP_DOMAIN.to_vec();
        aad.extend_from_slice(self.label());
        aad
    }
}

/// A key encrypted under another key. Safe to store on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedKey(Vec<u8>);

impl WrappedKey {
    /// Parses the serialised form, checking only its length. Authenticity is
    /// checked by [`unwrap_key`].
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        (bytes.len() == WRAPPED_LEN).then(|| Self(bytes.to_vec()))
    }

    /// The versioned serialised bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Serialize for WrappedKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_b64::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for WrappedKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let bytes: Vec<u8> = serde_b64::deserialize(deserializer)?;
        Self::from_bytes(&bytes).ok_or_else(|| D::Error::custom("wrapped key has wrong length"))
    }
}

/// Wraps `key` under `wrapping_key` for the given purpose.
pub fn wrap_key<W: SecretKey, K: SecretKey>(
    wrapping_key: &W,
    context: WrapContext,
    key: &K,
) -> Result<WrappedKey, AeadError> {
    aead::seal(wrapping_key.as_bytes(), &context.aad(), key.as_bytes()).map(WrappedKey)
}

/// Unwraps a key. Fails with the same opaque error for a wrong wrapping key,
/// a different context, or any modification of the blob.
pub fn unwrap_key<W: SecretKey, K: SecretKey>(
    wrapping_key: &W,
    context: WrapContext,
    wrapped: &WrappedKey,
) -> Result<K, AeadError> {
    let mut plain = aead::open(wrapping_key.as_bytes(), &context.aad(), &wrapped.0)?;
    let result = <[u8; KEY_LEN]>::try_from(plain.as_slice())
        .map(|mut bytes| {
            let key = K::from_bytes(&bytes);
            bytes.zeroize();
            key
        })
        .map_err(|_| AeadError);
    plain.zeroize();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{RootKey, VaultKey, WorkspaceKey, WrappingKey};

    #[test]
    fn wrap_and_unwrap_round_trip() {
        let wrapper = WrappingKey::from_bytes(&[1; 32]);
        let vault = VaultKey::generate();
        let wrapped = wrap_key(&wrapper, WrapContext::VaultKeyByPassword, &vault).expect("wrap");
        assert_eq!(wrapped.as_bytes().len(), WRAPPED_LEN);
        let back: VaultKey =
            unwrap_key(&wrapper, WrapContext::VaultKeyByPassword, &wrapped).expect("unwrap");
        assert_eq!(back, vault);
    }

    #[test]
    fn wrong_wrapping_key_fails() {
        let wrapped = wrap_key(
            &WrappingKey::from_bytes(&[1; 32]),
            WrapContext::WorkspaceKey,
            &WorkspaceKey::generate(),
        )
        .expect("wrap");
        let result: Result<WorkspaceKey, _> = unwrap_key(
            &WrappingKey::from_bytes(&[2; 32]),
            WrapContext::WorkspaceKey,
            &wrapped,
        );
        assert_eq!(result, Err(AeadError));
    }

    #[test]
    fn swapped_context_label_fails() {
        let wrapper = WrappingKey::from_bytes(&[1; 32]);
        let wrapped = wrap_key(
            &wrapper,
            WrapContext::VaultKeyByPassword,
            &VaultKey::generate(),
        )
        .expect("wrap");
        for other in [
            WrapContext::RootKeyByPassword,
            WrapContext::RootKeyByRecovery,
            WrapContext::WorkspaceKey,
            WrapContext::VaultKeyByRecovery,
        ] {
            let result: Result<RootKey, _> = unwrap_key(&wrapper, other, &wrapped);
            assert_eq!(result, Err(AeadError), "{other:?}");
        }
    }

    #[test]
    fn tampered_blob_fails() {
        let wrapper = WrappingKey::from_bytes(&[1; 32]);
        let wrapped = wrap_key(
            &wrapper,
            WrapContext::WorkspaceKey,
            &WorkspaceKey::generate(),
        )
        .expect("wrap");
        let mut bytes = wrapped.as_bytes().to_vec();
        bytes[40] ^= 1;
        let tampered = WrappedKey::from_bytes(&bytes).expect("length ok");
        let result: Result<WorkspaceKey, _> =
            unwrap_key(&wrapper, WrapContext::WorkspaceKey, &tampered);
        assert_eq!(result, Err(AeadError));
    }

    #[test]
    fn from_bytes_rejects_wrong_length() {
        assert!(WrappedKey::from_bytes(&[0; WRAPPED_LEN - 1]).is_none());
        assert!(WrappedKey::from_bytes(&[0; WRAPPED_LEN + 1]).is_none());
    }

    #[test]
    fn serde_round_trip_and_rejects_bad_length() {
        let wrapper = WrappingKey::from_bytes(&[1; 32]);
        let wrapped = wrap_key(
            &wrapper,
            WrapContext::WorkspaceKey,
            &WorkspaceKey::generate(),
        )
        .expect("wrap");
        let json = serde_json::to_string(&wrapped).expect("serialise");
        let back: WrappedKey = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, wrapped);
        assert!(serde_json::from_str::<WrappedKey>("\"AAAA\"").is_err());
        assert!(serde_json::from_str::<WrappedKey>("\"!!!\"").is_err());
    }
}
