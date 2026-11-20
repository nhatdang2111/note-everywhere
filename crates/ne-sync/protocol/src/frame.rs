//! Binary WebSocket frames, version 1. All integers are big-endian.
//!
//! ```text
//! frame  = version u8 | type u8 | body
//! id     = len u8 | utf-8 bytes            (see `ids::is_valid_id`)
//! blob   = len u32 | bytes
//! ```
//!
//! The byte layouts of each body are documented next to the encoder of the
//! frame, and in `docs/sync-protocol.md`.

use crate::ids::is_valid_id;
use crate::limits::{
    MAX_FRAME_BYTES, MAX_PAGE_ENTRIES, MAX_PARTS, MAX_PUSH_ENTRIES, MAX_SNAPSHOT_PAYLOAD,
    MAX_UPDATE_PAYLOAD,
};

/// Wire format version, the first byte of every frame.
pub const VERSION: u8 = 1;
/// Length of a client-generated update id.
pub const CLIENT_ID_LEN: usize = 16;

const T_PUSH: u8 = 0x01;
const T_PULL: u8 = 0x02;
const T_SNAPSHOT: u8 = 0x03;
const T_COMMIT: u8 = 0x04;
const T_ACK: u8 = 0x81;
const T_UPDATES: u8 = 0x82;
const T_SNAPSHOT_ACK: u8 = 0x83;
const T_ERROR: u8 = 0x8f;

/// Why a frame could not be encoded or decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Truncated,
    TrailingBytes,
    UnsupportedVersion(u8),
    UnknownType(u8),
    InvalidId,
    InvalidValue,
    TooLarge,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => f.write_str("frame is truncated"),
            Self::TrailingBytes => f.write_str("frame has trailing bytes"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported protocol version {v}"),
            Self::UnknownType(t) => write!(f, "unknown frame type {t:#04x}"),
            Self::InvalidId => f.write_str("invalid identifier"),
            Self::InvalidValue => f.write_str("invalid value"),
            Self::TooLarge => f.write_str("frame or field too large"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Random id the client picks for each update; the server stores each id once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub [u8; CLIENT_ID_LEN]);

/// One encrypted update inside a push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushEntry {
    pub client_id: ClientId,
    pub doc_id: String,
    pub payload: Vec<u8>,
}

/// Whether a committed large object is an update or a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LargeKind {
    Update,
    Snapshot { upto: u64 },
}

/// What the log row of a large object holds instead of a payload: where its
/// parts are staged (the group is the row's `client_id`) and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Manifest {
    pub group: [u8; CLIENT_ID_LEN],
    pub parts: u16,
}

impl Manifest {
    pub const LEN: usize = CLIENT_ID_LEN + 2;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&self.group);
        out.extend_from_slice(&self.parts.to_be_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let bytes: &[u8; Self::LEN] = bytes.try_into().map_err(|_| ProtocolError::InvalidValue)?;
        let (group, parts) = bytes.split_at(CLIENT_ID_LEN);
        let parts = u16::from_be_bytes([parts[0], parts[1]]);
        if parts == 0 || usize::from(parts) > MAX_PARTS {
            return Err(ProtocolError::InvalidValue);
        }
        Ok(Self {
            group: group.try_into().map_err(|_| ProtocolError::InvalidValue)?,
            parts,
        })
    }
}

/// Frames the client sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientFrame {
    /// `req_id u32 | count u16 | count x (client_id 16 | doc id | payload blob)`
    Push {
        req_id: u32,
        entries: Vec<PushEntry>,
    },
    /// `req_id u32 | cursor u64`
    Pull { req_id: u32, cursor: u64 },
    /// `req_id u32 | client_id 16 | doc id | upto u64 | payload blob`
    Snapshot {
        req_id: u32,
        client_id: ClientId,
        doc_id: String,
        upto: u64,
        payload: Vec<u8>,
    },
    /// `req_id u32 | client_id 16 | doc id | kind u8 (0 update, 1 snapshot) |
    /// upto u64 | parts u16`. Publishes an object whose `parts` sealed parts
    /// were staged beforehand under the group `client_id`. The server stores
    /// the row only if every part exists. The answer is an `Ack` (update) or a
    /// `SnapshotAck` (snapshot), like the inline frames.
    Commit {
        req_id: u32,
        client_id: ClientId,
        doc_id: String,
        kind: LargeKind,
        parts: u16,
    },
}

/// What a log row holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Update,
    /// Full document state covering every update of the document up to `upto`.
    Snapshot,
    /// An update too big for a row; the payload is a [`Manifest`].
    LargeUpdate,
    /// A snapshot too big for a row; the payload is a [`Manifest`].
    LargeSnapshot,
}

impl EntryKind {
    pub fn is_snapshot(self) -> bool {
        matches!(self, Self::Snapshot | Self::LargeSnapshot)
    }

    pub fn is_large(self) -> bool {
        matches!(self, Self::LargeUpdate | Self::LargeSnapshot)
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Update => 0,
            Self::Snapshot => 1,
            Self::LargeUpdate => 2,
            Self::LargeSnapshot => 3,
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Update,
            1 => Self::Snapshot,
            2 => Self::LargeUpdate,
            3 => Self::LargeSnapshot,
            _ => return None,
        })
    }
}

/// One row of the log as delivered to a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub seq: u64,
    pub kind: EntryKind,
    /// For snapshots, the highest sequence number the state covers; else 0.
    pub upto: u64,
    pub doc_id: String,
    pub device_id: String,
    pub payload: Vec<u8>,
}

/// Result of a snapshot upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// Stored; `dropped` log rows of the document were released.
    Accepted { dropped: u32 },
    /// The server already holds a snapshot covering at least as much.
    Stale,
    /// `upto` is beyond the log head, so the claim cannot be true.
    Rejected,
}

/// Machine-readable error codes. Messages are deliberately generic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Malformed,
    TooLarge,
    /// Storage or quota trouble on the server; retry later with backoff.
    Unavailable,
    /// The session expired or was revoked while connected.
    Unauthorized,
    Unknown(u16),
}

impl ErrorCode {
    fn to_u16(self) -> u16 {
        match self {
            Self::Malformed => 1,
            Self::TooLarge => 2,
            Self::Unavailable => 3,
            Self::Unauthorized => 4,
            Self::Unknown(code) => code,
        }
    }

    fn from_u16(code: u16) -> Self {
        match code {
            1 => Self::Malformed,
            2 => Self::TooLarge,
            3 => Self::Unavailable,
            4 => Self::Unauthorized,
            other => Self::Unknown(other),
        }
    }
}

/// Frames the server sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerFrame {
    /// `req_id u32 | head u64 | count u16 | count x seq u64`. `seq` is 0 for an
    /// entry the server had stored already (a retry).
    Ack {
        req_id: u32,
        head: u64,
        seqs: Vec<u64>,
    },
    /// `req_id u32 | flags u8 (bit0 = done) | next_cursor u64 | head u64 |
    /// count u16 | count x (seq u64 | kind u8 | upto u64 | doc id | device id |
    /// payload blob)`. `req_id` 0 marks a live fan-out frame.
    Updates {
        req_id: u32,
        done: bool,
        next_cursor: u64,
        head: u64,
        entries: Vec<LogEntry>,
    },
    /// `req_id u32 | status u8 (0 accepted, 1 stale, 2 rejected) | dropped u32`
    SnapshotAck {
        req_id: u32,
        outcome: SnapshotOutcome,
    },
    /// `req_id u32 | code u16`
    Error { req_id: u32, code: ErrorCode },
}

struct Writer(Vec<u8>);

impl Writer {
    fn new(kind: u8) -> Self {
        Self(vec![VERSION, kind])
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn id(&mut self, id: &str) -> Result<(), ProtocolError> {
        if !is_valid_id(id) {
            return Err(ProtocolError::InvalidId);
        }
        self.u8(id.len() as u8);
        self.0.extend_from_slice(id.as_bytes());
        Ok(())
    }
    fn blob(&mut self, bytes: &[u8]) -> Result<(), ProtocolError> {
        let len = u32::try_from(bytes.len()).map_err(|_| ProtocolError::TooLarge)?;
        self.u32(len);
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn finish(self) -> Result<Vec<u8>, ProtocolError> {
        if self.0.len() > MAX_FRAME_BYTES {
            return Err(ProtocolError::TooLarge);
        }
        Ok(self.0)
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtocolError> {
        if self.0.len() < n {
            return Err(ProtocolError::Truncated);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ProtocolError::Truncated)
    }
    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn id(&mut self) -> Result<String, ProtocolError> {
        let len = usize::from(self.u8()?);
        let raw = self.take(len)?;
        let text = std::str::from_utf8(raw).map_err(|_| ProtocolError::InvalidId)?;
        if !is_valid_id(text) {
            return Err(ProtocolError::InvalidId);
        }
        Ok(text.to_owned())
    }
    fn blob(&mut self, max: usize) -> Result<Vec<u8>, ProtocolError> {
        let len = usize::try_from(self.u32()?).map_err(|_| ProtocolError::TooLarge)?;
        if len > max {
            return Err(ProtocolError::TooLarge);
        }
        Ok(self.take(len)?.to_vec())
    }
    fn finish(self) -> Result<(), ProtocolError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(ProtocolError::TrailingBytes)
        }
    }
}

fn open(frame: &[u8]) -> Result<(u8, Reader<'_>), ProtocolError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut reader = Reader(frame);
    let version = reader.u8()?;
    if version != VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }
    Ok((reader.u8()?, reader))
}

fn check_update_payload(payload: &[u8]) -> Result<(), ProtocolError> {
    if payload.is_empty() || payload.len() > MAX_UPDATE_PAYLOAD {
        return Err(ProtocolError::TooLarge);
    }
    Ok(())
}

impl ClientFrame {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        match self {
            Self::Push { req_id, entries } => {
                if entries.is_empty() || entries.len() > MAX_PUSH_ENTRIES {
                    return Err(ProtocolError::InvalidValue);
                }
                let mut w = Writer::new(T_PUSH);
                w.u32(*req_id);
                w.u16(entries.len() as u16);
                for entry in entries {
                    check_update_payload(&entry.payload)?;
                    w.0.extend_from_slice(&entry.client_id.0);
                    w.id(&entry.doc_id)?;
                    w.blob(&entry.payload)?;
                }
                w.finish()
            }
            Self::Pull { req_id, cursor } => {
                let mut w = Writer::new(T_PULL);
                w.u32(*req_id);
                w.u64(*cursor);
                w.finish()
            }
            Self::Snapshot {
                req_id,
                client_id,
                doc_id,
                upto,
                payload,
            } => {
                if payload.is_empty() || payload.len() > MAX_SNAPSHOT_PAYLOAD {
                    return Err(ProtocolError::TooLarge);
                }
                let mut w = Writer::new(T_SNAPSHOT);
                w.u32(*req_id);
                w.0.extend_from_slice(&client_id.0);
                w.id(doc_id)?;
                w.u64(*upto);
                w.blob(payload)?;
                w.finish()
            }
            Self::Commit {
                req_id,
                client_id,
                doc_id,
                kind,
                parts,
            } => {
                if *parts == 0 || usize::from(*parts) > MAX_PARTS {
                    return Err(ProtocolError::InvalidValue);
                }
                let mut w = Writer::new(T_COMMIT);
                w.u32(*req_id);
                w.0.extend_from_slice(&client_id.0);
                w.id(doc_id)?;
                let (tag, upto) = match kind {
                    LargeKind::Update => (0, 0),
                    LargeKind::Snapshot { upto } => (1, *upto),
                };
                w.u8(tag);
                w.u64(upto);
                w.u16(*parts);
                w.finish()
            }
        }
    }

    pub fn decode(frame: &[u8]) -> Result<Self, ProtocolError> {
        let (kind, mut r) = open(frame)?;
        let decoded = match kind {
            T_PUSH => {
                let req_id = r.u32()?;
                let count = usize::from(r.u16()?);
                if count == 0 || count > MAX_PUSH_ENTRIES {
                    return Err(ProtocolError::InvalidValue);
                }
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let client_id = ClientId(r.array()?);
                    let doc_id = r.id()?;
                    let payload = r.blob(MAX_UPDATE_PAYLOAD)?;
                    check_update_payload(&payload)?;
                    entries.push(PushEntry {
                        client_id,
                        doc_id,
                        payload,
                    });
                }
                Self::Push { req_id, entries }
            }
            T_PULL => Self::Pull {
                req_id: r.u32()?,
                cursor: r.u64()?,
            },
            T_SNAPSHOT => {
                let req_id = r.u32()?;
                let client_id = ClientId(r.array()?);
                let doc_id = r.id()?;
                let upto = r.u64()?;
                let payload = r.blob(MAX_SNAPSHOT_PAYLOAD)?;
                if payload.is_empty() {
                    return Err(ProtocolError::InvalidValue);
                }
                Self::Snapshot {
                    req_id,
                    client_id,
                    doc_id,
                    upto,
                    payload,
                }
            }
            T_COMMIT => {
                let req_id = r.u32()?;
                let client_id = ClientId(r.array()?);
                let doc_id = r.id()?;
                let tag = r.u8()?;
                let upto = r.u64()?;
                let parts = r.u16()?;
                if parts == 0 || usize::from(parts) > MAX_PARTS {
                    return Err(ProtocolError::InvalidValue);
                }
                let kind = match (tag, upto) {
                    (0, 0) => LargeKind::Update,
                    (1, upto) => LargeKind::Snapshot { upto },
                    _ => return Err(ProtocolError::InvalidValue),
                };
                Self::Commit {
                    req_id,
                    client_id,
                    doc_id,
                    kind,
                    parts,
                }
            }
            other => return Err(ProtocolError::UnknownType(other)),
        };
        r.finish()?;
        Ok(decoded)
    }
}

impl ServerFrame {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        match self {
            Self::Ack { req_id, head, seqs } => {
                let mut w = Writer::new(T_ACK);
                w.u32(*req_id);
                w.u64(*head);
                w.u16(u16::try_from(seqs.len()).map_err(|_| ProtocolError::TooLarge)?);
                seqs.iter().for_each(|seq| w.u64(*seq));
                w.finish()
            }
            Self::Updates {
                req_id,
                done,
                next_cursor,
                head,
                entries,
            } => {
                if entries.len() > MAX_PAGE_ENTRIES {
                    return Err(ProtocolError::TooLarge);
                }
                let mut w = Writer::new(T_UPDATES);
                w.u32(*req_id);
                w.u8(u8::from(*done));
                w.u64(*next_cursor);
                w.u64(*head);
                w.u16(entries.len() as u16);
                for entry in entries {
                    w.u64(entry.seq);
                    w.u8(entry.kind.to_u8());
                    w.u64(entry.upto);
                    w.id(&entry.doc_id)?;
                    w.id(&entry.device_id)?;
                    w.blob(&entry.payload)?;
                }
                w.finish()
            }
            Self::SnapshotAck { req_id, outcome } => {
                let mut w = Writer::new(T_SNAPSHOT_ACK);
                w.u32(*req_id);
                let (status, dropped) = match outcome {
                    SnapshotOutcome::Accepted { dropped } => (0, *dropped),
                    SnapshotOutcome::Stale => (1, 0),
                    SnapshotOutcome::Rejected => (2, 0),
                };
                w.u8(status);
                w.u32(dropped);
                w.finish()
            }
            Self::Error { req_id, code } => {
                let mut w = Writer::new(T_ERROR);
                w.u32(*req_id);
                w.u16(code.to_u16());
                w.finish()
            }
        }
    }

    pub fn decode(frame: &[u8]) -> Result<Self, ProtocolError> {
        let (kind, mut r) = open(frame)?;
        let decoded = match kind {
            T_ACK => {
                let req_id = r.u32()?;
                let head = r.u64()?;
                let count = usize::from(r.u16()?);
                let seqs = (0..count).map(|_| r.u64()).collect::<Result<Vec<_>, _>>()?;
                Self::Ack { req_id, head, seqs }
            }
            T_UPDATES => {
                let req_id = r.u32()?;
                let flags = r.u8()?;
                let next_cursor = r.u64()?;
                let head = r.u64()?;
                let count = usize::from(r.u16()?);
                if flags > 1 || count > MAX_PAGE_ENTRIES {
                    return Err(ProtocolError::InvalidValue);
                }
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let seq = r.u64()?;
                    let kind = EntryKind::from_u8(r.u8()?).ok_or(ProtocolError::InvalidValue)?;
                    let upto = r.u64()?;
                    let doc_id = r.id()?;
                    let device_id = r.id()?;
                    let payload = r.blob(MAX_SNAPSHOT_PAYLOAD)?;
                    entries.push(LogEntry {
                        seq,
                        kind,
                        upto,
                        doc_id,
                        device_id,
                        payload,
                    });
                }
                Self::Updates {
                    req_id,
                    done: flags == 1,
                    next_cursor,
                    head,
                    entries,
                }
            }
            T_SNAPSHOT_ACK => {
                let req_id = r.u32()?;
                let status = r.u8()?;
                let dropped = r.u32()?;
                let outcome = match status {
                    0 => SnapshotOutcome::Accepted { dropped },
                    1 => SnapshotOutcome::Stale,
                    2 => SnapshotOutcome::Rejected,
                    _ => return Err(ProtocolError::InvalidValue),
                };
                Self::SnapshotAck { req_id, outcome }
            }
            T_ERROR => Self::Error {
                req_id: r.u32()?,
                code: ErrorCode::from_u16(r.u16()?),
            },
            other => return Err(ProtocolError::UnknownType(other)),
        };
        r.finish()?;
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: u8) -> PushEntry {
        PushEntry {
            client_id: ClientId([n; CLIENT_ID_LEN]),
            doc_id: format!("doc-{n}"),
            payload: vec![n; 50],
        }
    }

    fn client_frames() -> Vec<ClientFrame> {
        vec![
            ClientFrame::Push {
                req_id: 7,
                entries: vec![entry(1), entry(2)],
            },
            ClientFrame::Pull {
                req_id: 8,
                cursor: u64::MAX,
            },
            ClientFrame::Snapshot {
                req_id: 9,
                client_id: ClientId([3; CLIENT_ID_LEN]),
                doc_id: "doc-9".into(),
                upto: 42,
                payload: vec![9; 300],
            },
            ClientFrame::Commit {
                req_id: 10,
                client_id: ClientId([4; CLIENT_ID_LEN]),
                doc_id: "doc-10".into(),
                kind: LargeKind::Update,
                parts: 37,
            },
            ClientFrame::Commit {
                req_id: 11,
                client_id: ClientId([5; CLIENT_ID_LEN]),
                doc_id: "doc-11".into(),
                kind: LargeKind::Snapshot { upto: 99 },
                parts: 1,
            },
        ]
    }

    fn server_frames() -> Vec<ServerFrame> {
        vec![
            ServerFrame::Ack {
                req_id: 1,
                head: 12,
                seqs: vec![10, 0, 12],
            },
            ServerFrame::Updates {
                req_id: 0,
                done: true,
                next_cursor: 14,
                head: 14,
                entries: vec![
                    LogEntry {
                        seq: 11,
                        kind: EntryKind::Update,
                        upto: 0,
                        doc_id: "d".into(),
                        device_id: "dev".into(),
                        payload: vec![1, 2, 3],
                    },
                    LogEntry {
                        seq: 12,
                        kind: EntryKind::Snapshot,
                        upto: 9,
                        doc_id: "d".into(),
                        device_id: "dev".into(),
                        payload: vec![4; 10],
                    },
                    LogEntry {
                        seq: 13,
                        kind: EntryKind::LargeUpdate,
                        upto: 0,
                        doc_id: "d".into(),
                        device_id: "dev".into(),
                        payload: Manifest {
                            group: [8; CLIENT_ID_LEN],
                            parts: 3,
                        }
                        .encode(),
                    },
                    LogEntry {
                        seq: 14,
                        kind: EntryKind::LargeSnapshot,
                        upto: 12,
                        doc_id: "d".into(),
                        device_id: "dev".into(),
                        payload: vec![0; Manifest::LEN],
                    },
                ],
            },
            ServerFrame::SnapshotAck {
                req_id: 3,
                outcome: SnapshotOutcome::Accepted { dropped: 5 },
            },
            ServerFrame::SnapshotAck {
                req_id: 3,
                outcome: SnapshotOutcome::Stale,
            },
            ServerFrame::SnapshotAck {
                req_id: 3,
                outcome: SnapshotOutcome::Rejected,
            },
            ServerFrame::Error {
                req_id: 4,
                code: ErrorCode::Unavailable,
            },
            ServerFrame::Error {
                req_id: 4,
                code: ErrorCode::Unknown(999),
            },
        ]
    }

    #[test]
    fn client_frames_round_trip() {
        for frame in client_frames() {
            let bytes = frame.encode().expect("encode");
            assert_eq!(ClientFrame::decode(&bytes).expect("decode"), frame);
        }
    }

    #[test]
    fn server_frames_round_trip() {
        for frame in server_frames() {
            let bytes = frame.encode().expect("encode");
            assert_eq!(ServerFrame::decode(&bytes).expect("decode"), frame);
        }
    }

    #[test]
    fn every_strict_prefix_is_rejected_without_panicking() {
        for frame in client_frames() {
            let bytes = frame.encode().expect("encode");
            for len in 0..bytes.len() {
                assert!(ClientFrame::decode(&bytes[..len]).is_err(), "prefix {len}");
            }
        }
        for frame in server_frames() {
            let bytes = frame.encode().expect("encode");
            for len in 0..bytes.len() {
                assert!(ServerFrame::decode(&bytes[..len]).is_err(), "prefix {len}");
            }
        }
    }

    #[test]
    fn trailing_bytes_and_bad_headers_are_rejected() {
        let mut bytes = ClientFrame::Pull {
            req_id: 1,
            cursor: 2,
        }
        .encode()
        .expect("encode");
        bytes.push(0);
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(ProtocolError::TrailingBytes)
        );

        let mut bytes = ClientFrame::Pull {
            req_id: 1,
            cursor: 2,
        }
        .encode()
        .expect("encode");
        bytes[0] = 2;
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(ProtocolError::UnsupportedVersion(2))
        );

        let mut bytes = ClientFrame::Pull {
            req_id: 1,
            cursor: 2,
        }
        .encode()
        .expect("encode");
        bytes[1] = 0x55;
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(ProtocolError::UnknownType(0x55))
        );
        // A server frame is not a client frame.
        let ack = ServerFrame::Ack {
            req_id: 1,
            head: 1,
            seqs: vec![],
        }
        .encode()
        .expect("encode");
        assert!(ClientFrame::decode(&ack).is_err());
    }

    #[test]
    fn limits_apply_when_encoding_and_decoding() {
        let too_big = PushEntry {
            payload: vec![0; MAX_UPDATE_PAYLOAD + 1],
            ..entry(1)
        };
        assert_eq!(
            ClientFrame::Push {
                req_id: 1,
                entries: vec![too_big]
            }
            .encode(),
            Err(ProtocolError::TooLarge)
        );
        assert_eq!(
            ClientFrame::Push {
                req_id: 1,
                entries: vec![]
            }
            .encode(),
            Err(ProtocolError::InvalidValue)
        );
        let many = (0..=MAX_PUSH_ENTRIES).map(|_| entry(1)).collect();
        assert_eq!(
            ClientFrame::Push {
                req_id: 1,
                entries: many
            }
            .encode(),
            Err(ProtocolError::InvalidValue)
        );
        let bad_id = PushEntry {
            doc_id: "no spaces".into(),
            ..entry(1)
        };
        assert_eq!(
            ClientFrame::Push {
                req_id: 1,
                entries: vec![bad_id]
            }
            .encode(),
            Err(ProtocolError::InvalidId)
        );
        assert_eq!(
            ClientFrame::decode(&vec![1; MAX_FRAME_BYTES + 1]),
            Err(ProtocolError::TooLarge)
        );
    }

    #[test]
    fn commit_validates_part_counts_and_kind() {
        let commit = |parts| ClientFrame::Commit {
            req_id: 1,
            client_id: ClientId([1; CLIENT_ID_LEN]),
            doc_id: "d".into(),
            kind: LargeKind::Update,
            parts,
        };
        assert_eq!(commit(0).encode(), Err(ProtocolError::InvalidValue));
        assert_eq!(
            commit(MAX_PARTS as u16 + 1).encode(),
            Err(ProtocolError::InvalidValue)
        );
        assert!(commit(MAX_PARTS as u16).encode().is_ok());
        // An update must not claim a covered position.
        let mut bytes = commit(2).encode().expect("encode");
        let at = bytes.len() - 2 - 8;
        bytes[at..at + 8].copy_from_slice(&5u64.to_be_bytes());
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(ProtocolError::InvalidValue)
        );
        // Unknown kind tag.
        let mut bytes = commit(2).encode().expect("encode");
        let at = bytes.len() - 2 - 8 - 1;
        bytes[at] = 2;
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(ProtocolError::InvalidValue)
        );
    }

    #[test]
    fn manifests_round_trip_and_reject_bad_sizes() {
        let manifest = Manifest {
            group: [3; CLIENT_ID_LEN],
            parts: 40,
        };
        assert_eq!(Manifest::decode(&manifest.encode()), Ok(manifest));
        assert!(Manifest::decode(&[0; Manifest::LEN - 1]).is_err());
        assert!(Manifest::decode(&[0; Manifest::LEN + 1]).is_err());
        // parts = 0
        assert!(
            Manifest::decode(
                &[1; CLIENT_ID_LEN]
                    .iter()
                    .chain(&[0, 0])
                    .copied()
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
        // parts above the limit
        let mut over = manifest.encode();
        let n = (MAX_PARTS as u16 + 1).to_be_bytes();
        over[CLIENT_ID_LEN..].copy_from_slice(&n);
        assert!(Manifest::decode(&over).is_err());
    }

    #[test]
    fn entry_kinds_map_to_wire_values() {
        for value in 0..=3u8 {
            let kind = EntryKind::from_u8(value).expect("kind");
            assert_eq!(kind.to_u8(), value);
        }
        assert_eq!(EntryKind::from_u8(4), None);
        assert!(EntryKind::LargeSnapshot.is_snapshot() && EntryKind::LargeSnapshot.is_large());
        assert!(!EntryKind::Update.is_snapshot() && !EntryKind::Update.is_large());
    }

    #[test]
    fn declared_length_beyond_the_frame_is_rejected() {
        let mut bytes = ClientFrame::Push {
            req_id: 1,
            entries: vec![entry(1)],
        }
        .encode()
        .expect("encode");
        // The payload length field sits after version, type, req_id, count, client id, doc id.
        let at = 2 + 4 + 2 + CLIENT_ID_LEN + 1 + "doc-1".len();
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(ClientFrame::decode(&bytes), Err(ProtocolError::TooLarge));
    }
}
