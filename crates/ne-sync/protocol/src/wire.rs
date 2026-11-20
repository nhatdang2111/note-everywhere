//! JSON bodies of the HTTP API.
//!
//! The account and vault records mirror what `ne-crypto::account` serialises
//! (URL-safe base64 without padding for every byte string). The server cannot
//! link the client-only parts of `ne-crypto`, so it validates the shape and the
//! lengths here and stores the records without ever opening them.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

/// Length of a wrapped key: version, nonce, 32 key bytes, tag.
pub const WRAPPED_KEY_LEN: usize = 1 + 24 + 32 + 16;
/// Length of the Argon2 salts and of the auth key and verifier.
pub const SALT_LEN: usize = 16;
pub const KEY_LEN: usize = 32;

pub fn b64_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes unpadded URL-safe base64 of exactly `N` bytes.
pub fn b64_decode_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()
}

fn is_b64_of_len(text: &str, len: usize) -> bool {
    // 4 characters per 3 bytes, so the decoded length is known up front and an
    // oversized string is rejected before any allocation.
    text.len() == (len * 4).div_ceil(3)
        && URL_SAFE_NO_PAD.decode(text).is_ok_and(|b| b.len() == len)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KdfParamsWire {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParamsWire {
    /// Sanity bounds only; the client enforces its own, stricter policy.
    pub fn is_sane(&self) -> bool {
        (8..=1024 * 1024).contains(&self.memory_kib)
            && (1..=16).contains(&self.iterations)
            && (1..=16).contains(&self.parallelism)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRecordWire {
    pub kdf_salt: String,
    pub kdf_params: KdfParamsWire,
    pub auth_verifier: String,
    /// Verifier of the recovery auth key. Absent on accounts created before
    /// recovery verification existed; those cannot be recovered through the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_verifier: Option<String>,
    pub root_key_by_password: String,
    pub root_key_by_recovery: String,
    pub workspace_key: String,
}

impl AccountRecordWire {
    pub fn is_valid(&self) -> bool {
        self.kdf_params.is_sane()
            && is_b64_of_len(&self.kdf_salt, SALT_LEN)
            && is_b64_of_len(&self.auth_verifier, KEY_LEN)
            && self
                .recovery_verifier
                .as_deref()
                .is_none_or(|v| is_b64_of_len(v, KEY_LEN))
            && is_b64_of_len(&self.root_key_by_password, WRAPPED_KEY_LEN)
            && is_b64_of_len(&self.root_key_by_recovery, WRAPPED_KEY_LEN)
            && is_wrapped_key(&self.workspace_key)
    }
}

/// True when `text` is a base64 encoded wrapped key.
pub fn is_wrapped_key(text: &str) -> bool {
    is_b64_of_len(text, WRAPPED_KEY_LEN)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultRecordWire {
    pub kdf_salt: String,
    pub kdf_params: KdfParamsWire,
    pub vault_key_by_password: String,
    pub vault_key_by_recovery: String,
}

impl VaultRecordWire {
    pub fn is_valid(&self) -> bool {
        self.kdf_params.is_sane()
            && is_b64_of_len(&self.kdf_salt, SALT_LEN)
            && is_wrapped_key(&self.vault_key_by_password)
            && is_wrapped_key(&self.vault_key_by_recovery)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaltRequest {
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaltResponse {
    pub kdf_salt: String,
    pub kdf_params: KdfParamsWire,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    pub setup_code: String,
    pub username: String,
    pub device_label: String,
    pub auth_key: String,
    /// Recovery auth key (`ne-crypto::recovery`); the server stores its verifier.
    pub recovery_auth_key: String,
    pub record: AccountRecordWire,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub username: String,
    pub auth_key: String,
    pub device_label: String,
}

/// Returned by register and login.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionResponse {
    /// Bearer token; the server keeps only its SHA-256.
    pub token: String,
    pub session_id: String,
    /// Unix milliseconds.
    pub expires_at: u64,
    /// Identifier bound into every sync envelope.
    pub workspace_id: String,
    pub record: AccountRecordWire,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    pub current_auth_key: String,
    pub new_auth_key: String,
    pub record: AccountRecordWire,
}

/// Fetches the blobs a recovery key opens.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryLookupRequest {
    pub username: String,
}

/// The wrapped keys the recovery key opens. Unknown names get random-looking
/// blobs of the same shape, so the answer does not reveal whether the account exists.
#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryLookupResponse {
    pub root_key_by_recovery: String,
    pub workspace_key: String,
}

/// Resets the password with proof of the recovery key. Only the new password
/// material is taken from `record`; the server keeps the stored recovery blobs.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverRequest {
    pub username: String,
    pub recovery_auth_key: String,
    pub new_auth_key: String,
    pub device_label: String,
    pub record: AccountRecordWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub label: String,
    pub created_at: u64,
    /// Last activity, rounded to the server's touch interval. Equals
    /// `created_at` until the session is first used.
    #[serde(default)]
    pub last_seen_at: u64,
    pub expires_at: u64,
    pub current: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionList {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceKeyBody {
    pub workspace_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

/// Stable error codes of the HTTP API.
pub mod codes {
    pub const BAD_REQUEST: &str = "bad_request";
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const INVALID_CREDENTIALS: &str = "invalid_credentials";
    pub const FORBIDDEN: &str = "forbidden";
    pub const REGISTRATION_CLOSED: &str = "registration_closed";
    pub const NOT_FOUND: &str = "not_found";
    pub const LOCKED: &str = "locked";
    pub const TOO_LARGE: &str = "payload_too_large";
    pub const LENGTH_REQUIRED: &str = "length_required";
    pub const QUOTA: &str = "quota_exceeded";
    pub const UNAVAILABLE: &str = "unavailable";
    pub const RECOVERY_UNAVAILABLE: &str = "recovery_unavailable";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> AccountRecordWire {
        let wrapped = b64_encode(&[1; WRAPPED_KEY_LEN]);
        AccountRecordWire {
            kdf_salt: b64_encode(&[2; SALT_LEN]),
            kdf_params: KdfParamsWire {
                memory_kib: 64,
                iterations: 2,
                parallelism: 1,
            },
            auth_verifier: b64_encode(&[3; KEY_LEN]),
            recovery_verifier: Some(b64_encode(&[4; KEY_LEN])),
            root_key_by_password: wrapped.clone(),
            root_key_by_recovery: wrapped.clone(),
            workspace_key: wrapped,
        }
    }

    #[test]
    fn a_well_formed_record_validates() {
        assert!(record().is_valid());
    }

    #[test]
    fn malformed_fields_are_rejected() {
        let mut bad = record();
        bad.kdf_salt = b64_encode(&[2; SALT_LEN + 1]);
        assert!(!bad.is_valid());
        let mut bad = record();
        bad.workspace_key = "not base64!".into();
        assert!(!bad.is_valid());
        let mut bad = record();
        bad.kdf_params.memory_kib = u32::MAX;
        assert!(!bad.is_valid());
        let mut bad = record();
        bad.auth_verifier = "A".repeat(100_000);
        assert!(!bad.is_valid());
        let mut bad = record();
        bad.recovery_verifier = Some("short".into());
        assert!(!bad.is_valid());
    }

    #[test]
    fn a_record_without_a_recovery_verifier_is_still_valid() {
        let mut legacy = record();
        legacy.recovery_verifier = None;
        assert!(legacy.is_valid());
        let json = serde_json::to_string(&legacy).expect("json");
        assert!(!json.contains("recovery_verifier"));
        let back: AccountRecordWire = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, legacy);
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(serde_json::from_str::<SaltRequest>(r#"{"username":"abc"}"#).is_ok());
        assert!(serde_json::from_str::<SaltRequest>(r#"{"username":"abc","extra":1}"#).is_err());
    }

    #[test]
    fn b64_array_helpers_check_length() {
        let text = b64_encode(&[9; 32]);
        assert_eq!(b64_decode_array::<32>(&text), Some([9; 32]));
        assert_eq!(b64_decode_array::<16>(&text), None);
    }
}
