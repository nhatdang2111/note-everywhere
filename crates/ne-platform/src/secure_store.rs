//! Storage in the operating system's secure store: the login keychain on macOS
//! and Credential Manager on Windows (through the `keyring` crate).
//!
//! # What may be stored
//!
//! Only two things: the session token and the account root key. The API is
//! shaped so that nothing else fits: the root key goes in as a
//! [`ne_crypto::keys::RootKey`], the token as `&str`, and there is no method
//! taking raw bytes or any other key type.
//!
//! **The vault key must never be stored.** It is meant to exist only while the
//! user has typed the second password, so that someone at an unlocked machine
//! still cannot read the vault. A `VaultKey` does not type-check here:
//!
//! ```compile_fail
//! use ne_crypto::keys::VaultKey;
//! use ne_platform::secure_store::CredentialStore;
//!
//! let store = CredentialStore::new("service", "profile").unwrap();
//! let vault_key = VaultKey::generate();
//! store.store_root_key(&vault_key).unwrap();
//! ```
//!
//! # Handling of values
//!
//! Loaded values come back in [`Zeroizing`] containers or key types that wipe
//! themselves on drop; byte buffers used on the way are wiped too. Errors never
//! contain the stored value.

use ne_crypto::keys::{KEY_LEN, RootKey, SecretKey};
use zeroize::Zeroizing;

/// Default service name under which the app's entries are filed; it is the
/// bundle identifier of the desktop app.
pub const DEFAULT_SERVICE: &str = "dev.noteeverywhere.app";

/// Longest session token accepted, in bytes. Windows limits a credential blob
/// to 2560 bytes.
pub const MAX_TOKEN_BYTES: usize = 2048;
const MAX_NAME_BYTES: usize = 256;

/// Why an operation on the secure store failed.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    /// No secure store exists on this system.
    #[error("no secure store is available on this system")]
    Unavailable,
    /// The store refused access (locked keychain, user declined a prompt).
    #[error("access to the secure store was denied")]
    AccessDenied,
    /// The stored value does not have the expected shape.
    #[error("the stored value is damaged")]
    Corrupt,
    /// The profile name or token is not acceptable.
    #[error("invalid value: {0}")]
    Invalid(&'static str),
    /// Any other failure reported by the operating system.
    #[error("secure store failure: {0}")]
    Platform(String),
}

/// Raw access to a store of named secrets. [`KeyringBackend`] is the real one;
/// the trait lets the naming and validation rules be tested without touching
/// the user's keychain.
pub trait SecretBackend {
    /// Creates or replaces the secret filed under (`service`, `user`).
    fn set(&self, service: &str, user: &str, secret: &[u8]) -> Result<(), CredentialError>;
    /// Reads a secret; `None` when there is none.
    fn get(&self, service: &str, user: &str)
    -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError>;
    /// Removes a secret; returns whether one existed.
    fn delete(&self, service: &str, user: &str) -> Result<bool, CredentialError>;
}

/// The operating system's secure store.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringBackend;

fn map_error(err: keyring::Error) -> CredentialError {
    match err {
        keyring::Error::NoDefaultStore => CredentialError::Unavailable,
        keyring::Error::NoStorageAccess(_) => CredentialError::AccessDenied,
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            CredentialError::Corrupt
        }
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => {
            CredentialError::Invalid("rejected by the secure store")
        }
        other => CredentialError::Platform(other.to_string()),
    }
}

fn entry(service: &str, user: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(service, user).map_err(map_error)
}

impl SecretBackend for KeyringBackend {
    fn set(&self, service: &str, user: &str, secret: &[u8]) -> Result<(), CredentialError> {
        entry(service, user)?.set_secret(secret).map_err(map_error)
    }

    fn get(
        &self,
        service: &str,
        user: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError> {
        match entry(service, user)?.get_secret() {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(map_error(err)),
        }
    }

    fn delete(&self, service: &str, user: &str) -> Result<bool, CredentialError> {
        match entry(service, user)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(err) => Err(map_error(err)),
        }
    }
}

/// The session token and account root key of one profile (one signed-in
/// account), kept in the OS secure store.
#[derive(Debug, Clone)]
pub struct CredentialStore<B: SecretBackend = KeyringBackend> {
    backend: B,
    service: String,
    profile: String,
}

impl CredentialStore<KeyringBackend> {
    /// A store backed by the OS secure store. `profile` names the account (for
    /// example its login name); different profiles do not collide.
    pub fn new(service: &str, profile: &str) -> Result<Self, CredentialError> {
        Self::with_backend(KeyringBackend, service, profile)
    }
}

impl<B: SecretBackend> CredentialStore<B> {
    /// Like [`CredentialStore::new`] with an explicit backend.
    pub fn with_backend(backend: B, service: &str, profile: &str) -> Result<Self, CredentialError> {
        if service.is_empty() || service.len() > MAX_NAME_BYTES {
            return Err(CredentialError::Invalid(
                "the service name must be 1 to 256 bytes",
            ));
        }
        if profile.is_empty() || profile.len() > MAX_NAME_BYTES {
            return Err(CredentialError::Invalid(
                "the profile name must be 1 to 256 bytes",
            ));
        }
        Ok(Self {
            backend,
            service: service.to_owned(),
            profile: profile.to_owned(),
        })
    }

    fn user(&self, slot: &str) -> String {
        format!("{}/{slot}", self.profile)
    }

    /// Stores (replacing) the session token.
    pub fn store_session_token(&self, token: &str) -> Result<(), CredentialError> {
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(CredentialError::Invalid(
                "the token must be 1 to 2048 bytes",
            ));
        }
        self.backend
            .set(&self.service, &self.user("session-token"), token.as_bytes())
    }

    /// The stored session token, if any.
    pub fn load_session_token(&self) -> Result<Option<Zeroizing<String>>, CredentialError> {
        let Some(bytes) = self
            .backend
            .get(&self.service, &self.user("session-token"))?
        else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| CredentialError::Corrupt)?;
        Ok(Some(Zeroizing::new(text.to_owned())))
    }

    /// Removes the session token; returns whether there was one.
    pub fn delete_session_token(&self) -> Result<bool, CredentialError> {
        self.backend
            .delete(&self.service, &self.user("session-token"))
    }

    /// Stores (replacing) the account root key.
    pub fn store_root_key(&self, key: &RootKey) -> Result<(), CredentialError> {
        self.backend
            .set(&self.service, &self.user("root-key"), key.as_bytes())
    }

    /// The stored account root key, if any.
    pub fn load_root_key(&self) -> Result<Option<RootKey>, CredentialError> {
        let Some(bytes) = self.backend.get(&self.service, &self.user("root-key"))? else {
            return Ok(None);
        };
        let raw: Zeroizing<[u8; KEY_LEN]> = Zeroizing::new(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| CredentialError::Corrupt)?,
        );
        Ok(Some(RootKey::from_bytes(&raw)))
    }

    /// Removes the root key; returns whether there was one.
    pub fn delete_root_key(&self) -> Result<bool, CredentialError> {
        self.backend.delete(&self.service, &self.user("root-key"))
    }

    /// Removes everything stored for this profile (sign-out). Attempts both
    /// deletions even if the first fails and reports the first error.
    pub fn clear(&self) -> Result<(), CredentialError> {
        let token = self.delete_session_token();
        let key = self.delete_root_key();
        token.and(key).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// A plain in-memory map standing in for the OS store, to test our own
    /// rules (naming, validation, shapes) without touching the real keychain.
    #[derive(Default)]
    struct MapBackend(RefCell<HashMap<(String, String), Vec<u8>>>);

    impl SecretBackend for MapBackend {
        fn set(&self, service: &str, user: &str, secret: &[u8]) -> Result<(), CredentialError> {
            self.0
                .borrow_mut()
                .insert((service.into(), user.into()), secret.to_vec());
            Ok(())
        }

        fn get(
            &self,
            service: &str,
            user: &str,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError> {
            Ok(self
                .0
                .borrow()
                .get(&(service.to_owned(), user.to_owned()))
                .cloned()
                .map(Zeroizing::new))
        }

        fn delete(&self, service: &str, user: &str) -> Result<bool, CredentialError> {
            Ok(self
                .0
                .borrow_mut()
                .remove(&(service.to_owned(), user.to_owned()))
                .is_some())
        }
    }

    fn store(profile: &str) -> CredentialStore<MapBackend> {
        CredentialStore::with_backend(MapBackend::default(), "svc", profile).expect("valid")
    }

    #[test]
    fn token_round_trip_and_delete() {
        let store = store("ana");
        assert!(store.load_session_token().expect("load").is_none());
        store.store_session_token("tok-synthetic").expect("store");
        let loaded = store.load_session_token().expect("load").expect("some");
        assert_eq!(&*loaded, "tok-synthetic");
        store.store_session_token("tok-2").expect("replace");
        let loaded = store.load_session_token().expect("load").expect("some");
        assert_eq!(&*loaded, "tok-2");
        assert!(store.delete_session_token().expect("delete"));
        assert!(!store.delete_session_token().expect("delete again"));
    }

    #[test]
    fn root_key_round_trip_and_slots_are_separate() {
        let store = store("ana");
        let key = RootKey::generate();
        store.store_root_key(&key).expect("store");
        store.store_session_token("t").expect("store token");
        assert!(store.load_root_key().expect("load").expect("some") == key);
        assert!(store.delete_root_key().expect("delete"));
        assert!(store.load_root_key().expect("load").is_none());
        assert!(store.load_session_token().expect("load").is_some());
        store.clear().expect("clear");
        assert!(store.load_session_token().expect("load").is_none());
    }

    #[test]
    fn profiles_do_not_collide() {
        let backend = MapBackend::default();
        backend.set("svc", "ana/session-token", b"a").expect("set");
        let ana = CredentialStore::with_backend(backend, "svc", "ana").expect("valid");
        let loaded = ana.load_session_token().expect("load").expect("some");
        assert_eq!(&*loaded, "a");
        assert!(
            ana.backend
                .get("svc", "bob/session-token")
                .expect("get")
                .is_none()
        );
    }

    #[test]
    fn damaged_values_and_bad_input_are_typed_errors() {
        let backend = MapBackend::default();
        backend
            .set("svc", "ana/root-key", b"too short")
            .expect("set");
        backend
            .set("svc", "ana/session-token", &[0xff, 0xfe])
            .expect("set");
        let store = CredentialStore::with_backend(backend, "svc", "ana").expect("valid");
        assert!(matches!(
            store.load_root_key(),
            Err(CredentialError::Corrupt)
        ));
        assert!(matches!(
            store.load_session_token(),
            Err(CredentialError::Corrupt)
        ));
        assert!(matches!(
            store.store_session_token(""),
            Err(CredentialError::Invalid(_))
        ));
        let long = "x".repeat(MAX_TOKEN_BYTES + 1);
        assert!(matches!(
            store.store_session_token(&long),
            Err(CredentialError::Invalid(_))
        ));
        assert!(CredentialStore::with_backend(MapBackend::default(), "svc", "").is_err());
        assert!(CredentialStore::with_backend(MapBackend::default(), "", "p").is_err());
    }

    /// Talks to the real OS secure store, which may show a permission prompt,
    /// so it only runs on request: `cargo test -p ne-platform -- --ignored`.
    #[test]
    #[ignore = "uses the real OS secure store and may prompt the user"]
    fn real_keychain_round_trip() {
        let profile = format!("ne-platform-test-{}", std::process::id());
        let store = CredentialStore::new("dev.noteeverywhere.test", &profile).expect("store");
        let key = RootKey::generate();
        store
            .store_session_token("synthetic-token")
            .expect("store token");
        store.store_root_key(&key).expect("store key");
        let token = store.load_session_token().expect("load").expect("token");
        assert_eq!(&*token, "synthetic-token");
        assert!(store.load_root_key().expect("load").expect("key") == key);
        store.clear().expect("clear");
        assert!(store.load_session_token().expect("load").is_none());
        assert!(store.load_root_key().expect("load").is_none());
    }
}
