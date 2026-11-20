//! Attachment upload and download.
//!
//! An attachment is encrypted with `ne-crypto::stream` into a temporary file
//! while its SHA-256 is computed; that hash is the object's address on the
//! server. Both directions stream through files, so memory stays flat for any
//! size up to the server's cap. The unit of progress is the whole object:
//! upload first asks whether the object exists and skips the transfer if so,
//! so re-running an interrupted upload picks up where it stopped.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use ne_crypto::keys::{SecretKey, WorkspaceKey};
use ne_crypto::stream::{decrypt_stream, encrypt_stream};
use ne_sync_protocol::ids::{hex_encode, is_valid_id};
use ne_sync_protocol::limits::MAX_BLOB_CEILING;
use reqwest::{Body, Method};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::backoff::Backoff;
use crate::client::SyncClient;
use crate::error::{Result, SyncError};
use crate::transport;

const UPLOAD_ATTEMPTS: u32 = 4;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(900);

/// Address of an uploaded attachment: keep it next to the document that
/// references the attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    /// Lowercase hex SHA-256 of the ciphertext.
    pub hash: String,
    /// Size of the ciphertext in bytes.
    pub size: u64,
}

/// Binds a stream to one workspace and attachment so the server cannot hand
/// back a different attachment's bytes.
fn attachment_aad(workspace_id: &str, attachment_id: &str) -> Vec<u8> {
    let mut aad = b"note-everywhere/v1/attachment".to_vec();
    for part in [workspace_id, attachment_id] {
        aad.extend_from_slice(&(part.len() as u16).to_be_bytes());
        aad.extend_from_slice(part.as_bytes());
    }
    aad
}

struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
    len: u64,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn check_hash(hash: &str) -> Result<()> {
    ne_sync_protocol::ids::hex_decode::<32>(hash)
        .map(drop)
        .ok_or_else(|| {
            SyncError::Input("attachment hash must be 64 lowercase hex characters".into())
        })
}

impl SyncClient {
    fn workspace_id(&self) -> Result<String> {
        Ok(self
            .transport
            .session()
            .ok_or(SyncError::Unauthorized)?
            .workspace_id)
    }

    /// Encrypts `plaintext` and stores it on the server, returning its address.
    /// Nothing is transferred if the same ciphertext is already stored.
    pub async fn upload_attachment<R: Read + Send + 'static>(
        &self,
        key: &WorkspaceKey,
        attachment_id: &str,
        plaintext: R,
    ) -> Result<BlobRef> {
        if !is_valid_id(attachment_id) {
            return Err(SyncError::Input("invalid attachment id".into()));
        }
        let aad = attachment_aad(&self.workspace_id()?, attachment_id);
        let key = WorkspaceKey::from_bytes(key.as_bytes());
        let (spool, blob) = tokio::task::spawn_blocking(move || -> Result<_> {
            let file = tempfile::NamedTempFile::new()?;
            let mut sink = HashingWriter {
                inner: BufWriter::new(file),
                hasher: Sha256::new(),
                len: 0,
            };
            encrypt_stream(&key, &aad, BufReader::new(plaintext), &mut sink).map_err(|err| {
                match err {
                    ne_crypto::stream::StreamError::Io(err) => SyncError::Io(err),
                    ne_crypto::stream::StreamError::Crypto => SyncError::Crypto,
                }
            })?;
            let HashingWriter { inner, hasher, len } = sink;
            let file = inner
                .into_inner()
                .map_err(|err| SyncError::Io(err.into_error()))?;
            let hash = hex_encode(&hasher.finalize());
            Ok((file, BlobRef { hash, size: len }))
        })
        .await
        .map_err(|_| SyncError::Input("background task failed".into()))??;

        if self.attachment_size(&blob.hash).await?.is_some() {
            return Ok(blob);
        }
        let backoff = Backoff::default();
        for attempt in 0..UPLOAD_ATTEMPTS {
            match self.put_blob_file(spool.path(), &blob).await {
                Ok(()) => return Ok(blob),
                Err(err) if err.is_retryable() && attempt + 1 < UPLOAD_ATTEMPTS => {
                    tokio::time::sleep(backoff.next(attempt)).await;
                }
                Err(err) => return Err(err),
            }
        }
        Err(SyncError::Unavailable)
    }

    /// Streams an already encrypted file to the server under `blob.hash`. The
    /// server verifies that the bytes hash to that address.
    pub async fn put_blob_file(&self, path: &Path, blob: &BlobRef) -> Result<()> {
        check_hash(&blob.hash)?;
        let file = tokio::fs::File::open(path).await?;
        let request = self
            .transport
            .authed(Method::PUT, &format!("/v1/blobs/{}", blob.hash))?
            .timeout(TRANSFER_TIMEOUT)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header(reqwest::header::CONTENT_LENGTH, blob.size)
            .body(Body::wrap_stream(ReaderStream::new(file)));
        transport::unit(request).await
    }

    /// Size of the stored object, or `None` if the server has no such object.
    pub async fn attachment_size(&self, hash: &str) -> Result<Option<u64>> {
        check_hash(hash)?;
        let request = self
            .transport
            .authed(Method::HEAD, &format!("/v1/blobs/{hash}"))?;
        match transport::check(request.send().await?).await {
            Ok(response) => Ok(response
                .headers()
                .get("x-blob-size")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())),
            Err(SyncError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn delete_attachment(&self, hash: &str) -> Result<()> {
        check_hash(hash)?;
        transport::unit(
            self.transport
                .authed(Method::DELETE, &format!("/v1/blobs/{hash}"))?,
        )
        .await
    }

    /// Downloads, verifies and decrypts an attachment into `dest`. The file
    /// appears at `dest` only after both the hash and the authentication of
    /// every chunk checked out; otherwise nothing is left behind and the error
    /// is [`SyncError::Tampered`].
    pub async fn download_attachment(
        &self,
        key: &WorkspaceKey,
        attachment_id: &str,
        blob: &BlobRef,
        dest: &Path,
    ) -> Result<()> {
        check_hash(&blob.hash)?;
        if !is_valid_id(attachment_id) {
            return Err(SyncError::Input("invalid attachment id".into()));
        }
        let aad = attachment_aad(&self.workspace_id()?, attachment_id);
        let cipher_file = tempfile::NamedTempFile::new()?;
        self.fetch_blob(&blob.hash, cipher_file.reopen()?).await?;

        let key = WorkspaceKey::from_bytes(key.as_bytes());
        let cipher_path = cipher_file.path().to_owned();
        let dest = dest.to_owned();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let dir = dest
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let out = tempfile::NamedTempFile::new_in(dir)?;
            let mut writer = BufWriter::new(out);
            decrypt_stream(
                &key,
                &aad,
                BufReader::new(std::fs::File::open(&cipher_path)?),
                &mut writer,
            )
            .map_err(|err| match err {
                ne_crypto::stream::StreamError::Io(err) => SyncError::Io(err),
                ne_crypto::stream::StreamError::Crypto => SyncError::Tampered,
            })?;
            let out = writer
                .into_inner()
                .map_err(|err| SyncError::Io(err.into_error()))?;
            out.persist(&dest).map_err(|err| SyncError::Io(err.error))?;
            Ok(())
        })
        .await
        .map_err(|_| SyncError::Input("background task failed".into()))??;
        drop(cipher_file);
        Ok(())
    }

    /// Streams the object into `file`, checking its hash against its address.
    async fn fetch_blob(&self, hash: &str, file: std::fs::File) -> Result<()> {
        let request = self
            .transport
            .authed(Method::GET, &format!("/v1/blobs/{hash}"))?
            .timeout(TRANSFER_TIMEOUT);
        let response = transport::check(request.send().await?).await?;
        let mut out = tokio::io::BufWriter::new(tokio::fs::File::from_std(file));
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            total += chunk.len() as u64;
            if total > MAX_BLOB_CEILING {
                return Err(SyncError::Tampered);
            }
            hasher.update(&chunk);
            out.write_all(&chunk).await?;
        }
        out.flush().await?;
        if hex_encode(&hasher.finalize()) != hash {
            return Err(SyncError::Tampered);
        }
        Ok(())
    }
}
