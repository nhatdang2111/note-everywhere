//! Attachments and what the server can see. Skipped without `NE_SYNC_TEST_URL`.
//!
//! The size and quota checks expect the server to run with the small limits
//! from `server/README.md` (1 MiB per object, 3 MiB in total); override them
//! with `NE_SYNC_TEST_BLOB_MAX` and `NE_SYNC_TEST_BLOB_QUOTA` if yours differ.

mod common;

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::Arc;

use ne_crypto::account::{begin_login, create_vault};
use ne_crypto::keys::SecretKey;
use ne_crypto::password::KdfPolicy;
use ne_sync::{BlobRef, MemoryStore, SyncError};
use ne_sync_protocol::ids::hex_encode;
use ne_sync_protocol::wire::b64_encode;
use sha2::{Digest, Sha256};

use common::{
    PASSWORD, fresh, raw_http, snapshot_of, start_engine, update_of, wait_for, wait_synced,
};

const MARKER: &str = "NE-PLAINTEXT-MARKER-5c1f0b77d2e94a38";

fn env_bytes(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn sample(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i.wrapping_mul(31) % 251) as u8).collect()
}

#[tokio::test]
async fn attachments_round_trip_and_are_checked_on_the_way_back() {
    let h = fresh(require_server!()).await;
    let (client, signup) = h.sign_up().await;
    let key = &signup.unlocked.secrets.workspace_key;
    let dir = tempfile::tempdir().expect("temp dir");

    // 200 KB spans several 64 KiB chunks.
    let plain = sample(200_000);
    let blob = client
        .upload_attachment(key, "att-1", Cursor::new(plain.clone()))
        .await
        .expect("upload");
    assert_eq!(
        client.attachment_size(&blob.hash).await.expect("head"),
        Some(blob.size)
    );
    assert!(
        blob.size > plain.len() as u64,
        "ciphertext carries a header and tags"
    );

    let out = dir.path().join("restored.bin");
    client
        .download_attachment(key, "att-1", &blob, &out)
        .await
        .expect("download");
    assert_eq!(std::fs::read(&out).expect("read"), plain);

    // Uploading a stored object again moves no data and succeeds.
    let spool = dir.path().join("spool");
    let http = raw_http();
    let bearer = client.session().expect("session").token;
    let object_url = h
        .url
        .join(&format!("/v1/blobs/{}", blob.hash))
        .expect("url");
    let genuine = http
        .get(object_url.clone())
        .bearer_auth(bearer.expose())
        .send()
        .await
        .expect("get");
    let mut cipher = genuine.bytes().await.expect("bytes").to_vec();
    std::fs::write(&spool, &cipher).expect("write");
    client
        .put_blob_file(&spool, &blob)
        .await
        .expect("idempotent put");

    // A stored object under the right address but with one flipped bit is rejected on download.
    let middle = cipher.len() / 2;
    cipher[middle] ^= 1;
    let forged = BlobRef {
        hash: hex_encode(&Sha256::digest(&cipher)),
        size: cipher.len() as u64,
    };
    std::fs::write(&spool, &cipher).expect("write");
    client
        .put_blob_file(&spool, &forged)
        .await
        .expect("the server cannot tell");
    let tampered_out = dir.path().join("tampered.bin");
    let result = client
        .download_attachment(key, "att-1", &forged, &tampered_out)
        .await;
    assert!(matches!(result, Err(SyncError::Tampered)), "got {result:?}");
    assert!(!tampered_out.exists(), "nothing is left behind");

    // The same bytes cannot be passed off as another attachment.
    let swapped = client
        .download_attachment(key, "att-2", &blob, &dir.path().join("swapped.bin"))
        .await;
    assert!(matches!(swapped, Err(SyncError::Tampered)));

    // The server refuses bytes that do not match the address they are stored under.
    let lie = BlobRef {
        hash: hex_encode(&Sha256::digest(b"something else")),
        size: cipher.len() as u64,
    };
    assert!(client.put_blob_file(&spool, &lie).await.is_err());
    assert_eq!(client.attachment_size(&lie.hash).await.expect("head"), None);

    // Delete is final and idempotent.
    client.delete_attachment(&blob.hash).await.expect("delete");
    client
        .delete_attachment(&blob.hash)
        .await
        .expect("delete again");
    assert_eq!(
        client.attachment_size(&blob.hash).await.expect("head"),
        None
    );
    let gone = client
        .download_attachment(key, "att-1", &blob, &dir.path().join("gone.bin"))
        .await;
    assert!(matches!(gone, Err(SyncError::NotFound)));
}

#[tokio::test]
async fn attachment_size_cap_and_quota_are_enforced() {
    let h = fresh(require_server!()).await;
    let (client, signup) = h.sign_up().await;
    let key = &signup.unlocked.secrets.workspace_key;
    let max = env_bytes("NE_SYNC_TEST_BLOB_MAX", 1024 * 1024);
    let quota = env_bytes("NE_SYNC_TEST_BLOB_QUOTA", 3 * 1024 * 1024);

    let too_big = client
        .upload_attachment(key, "big", Cursor::new(sample(max + 1)))
        .await;
    assert!(
        matches!(too_big, Err(SyncError::TooLarge)),
        "got {too_big:?}"
    );

    let chunk = max - 1024;
    let mut stored = 0usize;
    let mut refused = None;
    for i in 0..=(quota / chunk + 1) {
        let id = format!("att-{i}");
        match client
            .upload_attachment(key, &id, Cursor::new(sample(chunk)))
            .await
        {
            Ok(_) => stored += chunk,
            Err(err) => {
                refused = Some(err);
                break;
            }
        }
    }
    assert!(
        matches!(refused, Some(SyncError::QuotaExceeded)),
        "got {refused:?}"
    );
    assert!(
        stored <= quota && stored + chunk > quota - 4096,
        "stopped at the quota, stored {stored}"
    );
}

#[tokio::test]
async fn the_server_stores_no_plaintext_password_or_unwrapped_key() {
    let h = fresh(require_server!()).await;
    let (client_a, signup) = h.sign_up().await;
    let (client_b, unlocked_b) = h.login("device b").await;
    let (store_a, store_b) = (Arc::new(MemoryStore::new()), Arc::new(MemoryStore::new()));
    let a = start_engine(
        &client_a,
        &signup.unlocked.secrets,
        "device-a",
        &store_a,
        None,
    );
    let b = start_engine(&client_b, &unlocked_b.secrets, "device-b", &store_b, None);
    wait_synced(&a).await;
    wait_synced(&b).await;

    // Notes, a snapshot, an attachment and a vault, all carrying the marker.
    let note = format!("{MARKER} private note {}", "x".repeat(2000)).into_bytes();
    for n in 0..5u32 {
        let mut update = update_of(n);
        update.extend_from_slice(&note);
        a.submit("doc-main", &update).expect("submit");
    }
    wait_for(
        "B to receive the notes",
        std::time::Duration::from_secs(20),
        || async { store_b.applied().len() == 5 },
    )
    .await;
    wait_synced(&a).await;
    let state = [snapshot_of(&BTreeSet::from([1u32, 2, 3])), note.clone()].concat();
    a.upload_snapshot("doc-main", a.snapshot_point().expect("point"), &state)
        .await
        .expect("snapshot");
    let key = &signup.unlocked.secrets.workspace_key;
    client_a
        .upload_attachment(
            key,
            "att-1",
            Cursor::new([MARKER.as_bytes(), &sample(70_000)].concat()),
        )
        .await
        .expect("upload");
    let vault =
        create_vault(MARKER.as_bytes(), common::TEST_KDF, &KdfPolicy::LENIENT).expect("vault");
    client_a
        .put_vault_record(&vault.record)
        .await
        .expect("vault record");
    client_a
        .change_password(PASSWORD, &format!("{MARKER} new password"))
        .await
        .expect("change");

    let record = client_a.account_record().await.expect("record");
    let auth_key = begin_login(
        format!("{MARKER} new password").as_bytes(),
        &record.kdf_salt,
        record.kdf_params,
        &KdfPolicy::LENIENT,
    )
    .expect("derive")
    .auth_key();
    let dump = h.dump().await;
    let contains = |needle: &[u8]| dump.windows(needle.len()).any(|w| w == needle);

    // The dump is real: it holds what the server must know.
    assert!(contains(b"doc-main"), "document ids are visible metadata");
    assert!(contains(b"alice@example.test"), "the username is stored");
    assert!(
        contains(b64_encode(record.root_key_by_password.as_bytes()).as_bytes()),
        "wrapped keys are stored"
    );
    assert!(dump.len() > 70_000, "attachment bytes are in the dump");

    // And it holds nothing it must not.
    assert!(
        !contains(MARKER.as_bytes()),
        "note, attachment or password text leaked"
    );
    assert!(!contains(PASSWORD.as_bytes()), "old password leaked");
    for (name, secret) in [
        (
            "workspace key",
            signup.unlocked.secrets.workspace_key.as_bytes(),
        ),
        ("root key", signup.unlocked.secrets.root_key.as_bytes()),
        ("auth key", auth_key.as_bytes()),
        ("vault key", vault.vault_key.as_bytes()),
    ] {
        assert!(!contains(secret), "{name} stored in the clear");
        assert!(
            !contains(b64_encode(secret).as_bytes()),
            "{name} stored as base64"
        );
        assert!(
            !contains(hex_encode(secret).as_bytes()),
            "{name} stored as hex"
        );
    }
    drop((a, b));
}
