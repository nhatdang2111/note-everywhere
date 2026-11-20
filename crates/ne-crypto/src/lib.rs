//! Password key derivation, key wrapping and authenticated encryption.
//!
//! This crate is the only place in Note Everywhere that touches
//! cryptography, and it only composes audited RustCrypto primitives
//! (XChaCha20-Poly1305, Argon2id, HKDF-SHA256, SHA-256).
//!
//! # Key hierarchy
//!
//! ```text
//! account password --Argon2id+HKDF--> auth key (to server) + wrapping key (local)
//! wrapping key  --wraps--> account root key <--wraps-- account recovery key
//! root key      --wraps--> workspace key  (encrypts sync envelopes and attachments)
//! vault password --Argon2id+HKDF--> wrapping key --wraps--> vault key <--wraps-- vault recovery key
//! ```
//!
//! The vault key is deliberately not wrapped by the root key.
//!
//! # Features
//!
//! * `client` (default): everything that needs OS randomness. Disable default
//!   features to get only [`auth`], which is what the sync Worker needs and
//!   which has no OS-specific dependencies.

pub mod auth;

#[cfg(feature = "client")]
pub mod account;
#[cfg(feature = "client")]
pub mod aead;
#[cfg(feature = "client")]
pub mod envelope;
#[cfg(feature = "client")]
pub mod key_wrap;
#[cfg(feature = "client")]
pub mod keys;
#[cfg(feature = "client")]
pub mod password;
#[cfg(feature = "client")]
pub mod recovery;
#[cfg(feature = "client")]
pub mod stream;

#[cfg(feature = "client")]
mod random;
#[cfg(feature = "client")]
mod serde_b64;
