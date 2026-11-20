//! Strongly typed 32-byte secret keys.
//!
//! Each key role has its own type so a vault key cannot be passed where a
//! workspace key is expected. All of them are wiped on drop, compare in
//! constant time, and print as `<redacted>` in `Debug` output.

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length in bytes of every key in this crate.
pub const KEY_LEN: usize = 32;

/// Common access to the raw bytes of a typed key.
pub trait SecretKey: Sized {
    /// Copies the key out of `bytes`. The caller is responsible for wiping its
    /// own copy of `bytes`.
    fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self;

    /// Borrows the raw key bytes. Never log or serialise these.
    fn as_bytes(&self) -> &[u8; KEY_LEN];
}

macro_rules! secret_key {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Zeroize, ZeroizeOnDrop)]
        pub struct $name([u8; KEY_LEN]);

        impl SecretKey for $name {
            fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self {
                Self(*bytes)
            }

            fn as_bytes(&self) -> &[u8; KEY_LEN] {
                &self.0
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.0.ct_eq(&other.0).into()
            }
        }

        impl Eq for $name {}

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

macro_rules! random_key {
    ($name:ident) => {
        impl $name {
            /// Generates a fresh key from the operating system's CSPRNG.
            pub fn generate() -> Self {
                Self(crate::random::array::<KEY_LEN>())
            }
        }
    };
}

secret_key!(
    /// Account root key: wraps the workspace key. Stored on the server only in
    /// wrapped form.
    RootKey
);
secret_key!(
    /// Encrypts sync envelopes and attachment chunks.
    WorkspaceKey
);
secret_key!(
    /// Encrypts vault items. Never wrapped by the root key.
    VaultKey
);
secret_key!(
    /// A key that wraps another key: derived from a password or a recovery key.
    WrappingKey
);
secret_key!(
    /// The half of the password-derived keys that is sent to the server.
    AuthKey
);

random_key!(RootKey);
random_key!(WorkspaceKey);
random_key!(VaultKey);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_differ() {
        assert_ne!(RootKey::generate(), RootKey::generate());
    }

    #[test]
    fn debug_never_prints_key_bytes() {
        let key = VaultKey::from_bytes(&[0xAB; KEY_LEN]);
        let text = format!("{key:?}");
        assert_eq!(text, "VaultKey(<redacted>)");
        assert!(!text.to_lowercase().contains("ab"));
    }

    #[test]
    fn from_bytes_round_trips() {
        let key = WrappingKey::from_bytes(&[9; KEY_LEN]);
        assert_eq!(key.as_bytes(), &[9; KEY_LEN]);
    }
}
