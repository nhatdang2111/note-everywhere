//! Property tests for sealing. Stream round trips live next to the stream code.

#![cfg(feature = "client")]

use ne_crypto::aead::{self, AeadError, OVERHEAD};
use ne_crypto::envelope::{open_update, seal_update};
use ne_crypto::keys::WorkspaceKey;
use proptest::prelude::*;

proptest! {
    #[test]
    fn seal_open_round_trips(
        key in any::<[u8; 32]>(),
        aad in proptest::collection::vec(any::<u8>(), 0..64),
        plaintext in prop_oneof![
            Just(Vec::new()),
            proptest::collection::vec(any::<u8>(), 0..2048),
        ],
    ) {
        let sealed = aead::seal(&key, &aad, &plaintext).expect("seal");
        prop_assert_eq!(sealed.len(), plaintext.len() + OVERHEAD);
        prop_assert_eq!(aead::open(&key, &aad, &sealed).expect("open"), plaintext);
    }

    #[test]
    fn any_single_byte_change_is_rejected(
        key in any::<[u8; 32]>(),
        plaintext in proptest::collection::vec(any::<u8>(), 0..256),
        position in any::<prop::sample::Index>(),
        flip in 1u8..=255,
    ) {
        let mut sealed = aead::seal(&key, b"aad", &plaintext).expect("seal");
        let index = position.index(sealed.len());
        sealed[index] ^= flip;
        prop_assert_eq!(aead::open(&key, b"aad", &sealed), Err(AeadError));
    }

    #[test]
    fn different_aad_is_rejected(
        key in any::<[u8; 32]>(),
        aad in proptest::collection::vec(any::<u8>(), 0..32),
        other in proptest::collection::vec(any::<u8>(), 0..32),
        plaintext in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        prop_assume!(aad != other);
        let sealed = aead::seal(&key, &aad, &plaintext).expect("seal");
        prop_assert_eq!(aead::open(&key, &other, &sealed), Err(AeadError));
    }

    #[test]
    fn envelopes_round_trip_only_for_their_own_ids(
        ws in "[a-z0-9-]{0,24}",
        doc in "[a-z0-9-]{0,24}",
        dev in "[a-z0-9-]{0,24}",
        other_doc in "[a-z0-9-]{0,24}",
        plaintext in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let key = WorkspaceKey::generate();
        let envelope = seal_update(&key, &ws, &doc, &dev, &plaintext).expect("seal");
        prop_assert_eq!(open_update(&key, &ws, &doc, &dev, &envelope).expect("open"), plaintext);
        prop_assume!(doc != other_doc);
        prop_assert!(open_update(&key, &ws, &other_doc, &dev, &envelope).is_err());
    }
}
