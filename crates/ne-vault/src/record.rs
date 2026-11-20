//! Sealing and opening of one item.
//!
//! The stored form of an item is a single binary value:
//!
//! ```text
//! | format version (1) | update counter (8, big endian) | ne-crypto sealed bytes |
//! ```
//!
//! The sealed bytes cover the whole [`ItemContent`] (title and tags included).
//! The associated data is a fixed domain label, the format version and the item
//! id, so a record cannot be moved to another id or replayed under another
//! format. The counter is visible metadata and is not authenticated; it only
//! orders edits for display and diagnostics, never decides what is trusted.

use ne_crypto::aead;
use ne_crypto::keys::{SecretKey, VaultKey};
use zeroize::Zeroizing;

use crate::error::{Result, VaultError};
use crate::item::{ItemContent, ItemId};

/// The only record format version this build writes and reads.
pub const FORMAT_VERSION: u8 = 1;
const AAD_DOMAIN: &[u8] = b"note-everywhere/vault-item";
const HEADER_LEN: usize = 1 + 8;

/// A sealed item as stored in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SealedRecord {
    pub counter: u64,
    pub sealed: Vec<u8>,
}

fn associated_data(id: &ItemId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_DOMAIN.len() + 1 + 16);
    aad.extend_from_slice(AAD_DOMAIN);
    aad.push(FORMAT_VERSION);
    aad.extend_from_slice(id.as_bytes());
    aad
}

impl SealedRecord {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.sealed.len());
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&self.counter.to_be_bytes());
        out.extend_from_slice(&self.sealed);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let (&version, rest) = bytes.split_first().ok_or(VaultError::Corrupt)?;
        if version != FORMAT_VERSION {
            return Err(VaultError::UnsupportedVersion(version));
        }
        if rest.len() < 8 + aead::OVERHEAD {
            return Err(VaultError::Corrupt);
        }
        let (counter, sealed) = rest.split_at(8);
        let counter = u64::from_be_bytes(counter.try_into().map_err(|_| VaultError::Corrupt)?);
        Ok(Self {
            counter,
            sealed: sealed.to_vec(),
        })
    }
}

/// Seals `content` for item `id`.
pub(crate) fn seal_content(
    key: &VaultKey,
    id: &ItemId,
    counter: u64,
    content: &ItemContent,
) -> Result<SealedRecord> {
    let mut plaintext = Zeroizing::new(Vec::with_capacity(content.size_hint()));
    serde_json::to_writer(&mut *plaintext, content).map_err(|_| VaultError::Crypto)?;
    let sealed = aead::seal(key.as_bytes(), &associated_data(id), &plaintext)
        .map_err(|_| VaultError::Crypto)?;
    Ok(SealedRecord { counter, sealed })
}

/// Opens a record. Every failure (wrong key, wrong id, tampering, undecodable
/// plaintext) is the same opaque [`VaultError::Corrupt`].
pub(crate) fn open_content(
    key: &VaultKey,
    id: &ItemId,
    record: &SealedRecord,
) -> Result<ItemContent> {
    let plaintext = aead::open(key.as_bytes(), &associated_data(id), &record.sealed)
        .map(Zeroizing::new)
        .map_err(|_| VaultError::Corrupt)?;
    serde_json::from_slice(&plaintext).map_err(|_| VaultError::Corrupt)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::item::ItemBody;

    fn content(title: &str, secret: &str) -> ItemContent {
        ItemContent {
            title: title.into(),
            tags: vec!["t".into()],
            body: ItemBody::ApiKey {
                value: secret.into(),
                notes: String::new(),
            },
            created_ms: 1,
            updated_ms: 2,
        }
    }

    #[test]
    fn record_binds_key_and_item_id() {
        let key = VaultKey::generate();
        let id = ItemId::generate();
        let record = seal_content(&key, &id, 3, &content("T", "S")).expect("seal");
        assert_eq!(open_content(&key, &id, &record).expect("open").title, "T");
        assert_eq!(
            open_content(&VaultKey::generate(), &id, &record).err(),
            Some(VaultError::Corrupt)
        );
        assert_eq!(
            open_content(&key, &ItemId::generate(), &record).err(),
            Some(VaultError::Corrupt)
        );
    }

    #[test]
    fn encoding_round_trips_and_rejects_bad_input() {
        let key = VaultKey::generate();
        let id = ItemId::generate();
        let record = seal_content(&key, &id, 9, &content("T", "S")).expect("seal");
        let bytes = record.encode();
        assert_eq!(SealedRecord::decode(&bytes).expect("decode"), record);
        assert_eq!(SealedRecord::decode(&[]).err(), Some(VaultError::Corrupt));
        assert_eq!(
            SealedRecord::decode(&bytes[..10]).err(),
            Some(VaultError::Corrupt)
        );
        let mut future = bytes.clone();
        future[0] = 2;
        assert_eq!(
            SealedRecord::decode(&future).err(),
            Some(VaultError::UnsupportedVersion(2))
        );
        let mut flipped = SealedRecord::decode(&bytes).expect("decode");
        flipped.sealed[30] ^= 1;
        assert_eq!(
            open_content(&key, &id, &flipped).err(),
            Some(VaultError::Corrupt)
        );
    }

    proptest! {
        #[test]
        fn seal_then_open_returns_the_content(
            title in "\\PC{1,40}",
            secret in "\\PC{0,200}",
            counter in any::<u64>(),
        ) {
            let key = VaultKey::generate();
            let id = ItemId::generate();
            let sealed = seal_content(&key, &id, counter, &content(&title, &secret)).expect("seal");
            let decoded = SealedRecord::decode(&sealed.encode()).expect("decode");
            prop_assert_eq!(decoded.counter, counter);
            let opened = open_content(&key, &id, &decoded).expect("open");
            prop_assert_eq!(&opened.title, &title);
            prop_assert_eq!(opened.body.field(crate::item::Field::Value), Some(secret.as_str()));
        }

        #[test]
        fn sealed_bytes_never_contain_the_plaintext(secret in "[a-zA-Z0-9]{12,60}") {
            let key = VaultKey::generate();
            let id = ItemId::generate();
            let sealed = seal_content(&key, &id, 1, &content("title", &secret)).expect("seal");
            let hay = sealed.encode();
            prop_assert!(!hay.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
    }
}
