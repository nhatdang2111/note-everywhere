//! Large objects: updates and snapshots above the row limit.
//!
//! The plaintext (already carrying the usual kind byte) is cut into chunks
//! and each chunk is sealed on its own, with a small header naming the group,
//! the index and the part count. Sealing parts separately keeps memory flat,
//! and because a part is valid whichever attempt produced it, an upload that
//! was interrupted resumes with only the missing parts. The header sits inside
//! the seal, so the server cannot swap, reorder, drop or mix parts of
//! different objects without the receiver noticing.
//!
//! Parts travel over HTTP (one request each) into a staging table; a `Commit`
//! frame then publishes the object atomically. See `docs/sync-protocol.md`.

use ne_crypto::envelope::{open_update, seal_update};
use ne_crypto::keys::WorkspaceKey;
use ne_sync_protocol::frame::CLIENT_ID_LEN;
use ne_sync_protocol::ids::hex_encode;
use ne_sync_protocol::limits::{MAX_LARGE_PLAINTEXT, MAX_PARTS, PART_CHUNK};
use reqwest::Method;
use serde::Deserialize;

use crate::engine::EngineConfig;
use crate::error::{Result, SyncError};
use crate::transport::{self, SessionState, Transport};

/// First byte of a sealed part's plaintext.
const PLAIN_PART: u8 = 2;
/// Tag, group, index, count.
const HEADER_LEN: usize = 1 + CLIENT_ID_LEN + 2 + 2;

pub(crate) type Group = [u8; CLIENT_ID_LEN];

/// Number of parts `len` plaintext bytes need.
pub(crate) fn part_count(len: usize) -> usize {
    len.div_ceil(PART_CHUNK).max(1)
}

/// True when `plain` must travel as parts rather than inline.
pub(crate) fn fits_parts(len: usize) -> bool {
    len <= MAX_LARGE_PLAINTEXT && part_count(len) <= MAX_PARTS
}

/// Seals part `index` of `plain`, which must be cut with [`part_count`].
pub(crate) fn seal_part(
    key: &WorkspaceKey,
    workspace: &str,
    doc: &str,
    device: &str,
    group: &Group,
    plain: &[u8],
    index: usize,
) -> Result<Vec<u8>> {
    let count = part_count(plain.len());
    let start = index * PART_CHUNK;
    let chunk = plain.get(start..(start + PART_CHUNK).min(plain.len()));
    let (Some(chunk), Ok(index), Ok(count)) = (chunk, u16::try_from(index), u16::try_from(count))
    else {
        return Err(SyncError::TooLarge);
    };
    let mut body = Vec::with_capacity(HEADER_LEN + chunk.len());
    body.push(PLAIN_PART);
    body.extend_from_slice(group);
    body.extend_from_slice(&index.to_be_bytes());
    body.extend_from_slice(&count.to_be_bytes());
    body.extend_from_slice(chunk);
    seal_update(key, workspace, doc, device, &body).map_err(|_| SyncError::Crypto)
}

/// What a received part says about itself.
pub(crate) struct OpenedPart {
    pub group: Group,
    pub index: u16,
    pub count: u16,
    pub chunk: Vec<u8>,
}

/// Authenticates and unpacks a part. `None` means it does not belong to this
/// workspace, document and device, or was altered.
pub(crate) fn open_part(
    key: &WorkspaceKey,
    workspace: &str,
    doc: &str,
    device: &str,
    sealed: &[u8],
) -> Option<OpenedPart> {
    let body = open_update(key, workspace, doc, device, sealed).ok()?;
    let (tag, rest) = body.split_first()?;
    if *tag != PLAIN_PART || rest.len() < HEADER_LEN - 1 {
        return None;
    }
    let (group, rest) = rest.split_at(CLIENT_ID_LEN);
    let (index, rest) = rest.split_at(2);
    let (count, chunk) = rest.split_at(2);
    Some(OpenedPart {
        group: group.try_into().ok()?,
        index: u16::from_be_bytes(index.try_into().ok()?),
        count: u16::from_be_bytes(count.try_into().ok()?),
        chunk: chunk.to_vec(),
    })
}

#[derive(Deserialize)]
struct Have {
    have: Vec<usize>,
}

/// The HTTP side of staging: upload, resume and download of parts.
pub(crate) struct PartClient {
    transport: Transport,
}

fn path(group: &Group) -> String {
    format!("/v1/parts/{}", hex_encode(group))
}

impl PartClient {
    pub(crate) fn new(cfg: &EngineConfig) -> Result<Self> {
        let transport = Transport::new(cfg.server.clone(), cfg.user_agent.clone())?;
        transport.set_session(Some(SessionState {
            token: cfg.token.clone(),
            session_id: String::new(),
            workspace_id: cfg.workspace_id.clone(),
            expires_at: 0,
        }));
        Ok(Self { transport })
    }

    /// Indices already staged for the group.
    pub(crate) async fn have(&self, group: &Group) -> Result<Vec<usize>> {
        let request = self.transport.authed(Method::GET, &path(group))?;
        Ok(transport::json::<Have>(request).await?.have)
    }

    pub(crate) async fn put(&self, group: &Group, index: usize, sealed: Vec<u8>) -> Result<()> {
        let request = self
            .transport
            .authed(Method::PUT, &format!("{}/{index}", path(group)))?
            .body(sealed);
        transport::unit(request).await
    }

    /// One part, or `None` if the server no longer has it.
    pub(crate) async fn get(&self, group: &Group, index: usize) -> Result<Option<Vec<u8>>> {
        let request = self
            .transport
            .authed(Method::GET, &format!("{}/{index}", path(group)))?;
        match transport::check(request.send().await?).await {
            Ok(response) => Ok(Some(response.bytes().await?.to_vec())),
            Err(SyncError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use ne_crypto::keys::SecretKey;
    use ne_sync_protocol::limits::MAX_PART_PAYLOAD;

    use super::*;

    fn key() -> WorkspaceKey {
        WorkspaceKey::from_bytes(&[5; 32])
    }

    fn assemble(plain: &[u8], group: &Group) -> Vec<u8> {
        let count = part_count(plain.len());
        let mut out = Vec::new();
        for index in 0..count {
            let sealed = seal_part(&key(), "ws", "doc", "dev", group, plain, index).expect("seal");
            assert!(sealed.len() <= MAX_PART_PAYLOAD);
            let opened = open_part(&key(), "ws", "doc", "dev", &sealed).expect("open");
            assert_eq!(opened.group, *group);
            assert_eq!(usize::from(opened.index), index);
            assert_eq!(usize::from(opened.count), count);
            out.extend_from_slice(&opened.chunk);
        }
        out
    }

    #[test]
    fn chunks_reassemble_to_the_original() {
        let group = [9; CLIENT_ID_LEN];
        for len in [
            1,
            PART_CHUNK - 1,
            PART_CHUNK,
            PART_CHUNK + 1,
            3 * PART_CHUNK + 17,
        ] {
            let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            assert_eq!(assemble(&plain, &group), plain, "len {len}");
        }
    }

    #[test]
    fn counts_and_limits() {
        assert_eq!(part_count(1), 1);
        assert_eq!(part_count(PART_CHUNK), 1);
        assert_eq!(part_count(PART_CHUNK + 1), 2);
        assert!(fits_parts(MAX_LARGE_PLAINTEXT));
        assert!(!fits_parts(MAX_LARGE_PLAINTEXT + 1));
        assert!(part_count(MAX_LARGE_PLAINTEXT) <= MAX_PARTS);
    }

    #[test]
    fn a_part_opens_only_where_it_was_sealed() {
        let group = [1; CLIENT_ID_LEN];
        let sealed = seal_part(&key(), "ws", "doc", "dev", &group, b"hello", 0).expect("seal");
        assert!(open_part(&key(), "ws", "other", "dev", &sealed).is_none());
        assert!(open_part(&key(), "ws", "doc", "other", &sealed).is_none());
        assert!(open_part(&key(), "other", "doc", "dev", &sealed).is_none());
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open_part(&key(), "ws", "doc", "dev", &tampered).is_none());
        // An inline update is not a part.
        let inline = seal_update(&key(), "ws", "doc", "dev", &[0, 1, 2, 3]).expect("seal");
        assert!(open_part(&key(), "ws", "doc", "dev", &inline).is_none());
    }

    #[test]
    fn sealing_an_index_past_the_end_fails() {
        let group = [1; CLIENT_ID_LEN];
        assert!(seal_part(&key(), "ws", "doc", "dev", &group, b"x", 1).is_err());
    }
}
