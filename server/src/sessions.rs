//! Session tokens: 256 random bits handed to the client once; only their
//! SHA-256 is stored, so a leaked database cannot be replayed as logins.

use ne_sync_protocol::ids::hex_encode;
use ne_sync_protocol::wire::{SessionInfo, b64_encode};
use sha2::{Digest, Sha256};
use worker::Result;

use crate::db::{Db, Row, b, n, s, text, uint};
use crate::random;

/// Sessions kept at once; the oldest are dropped beyond this.
const MAX_SESSIONS: u64 = 50;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub expires_at: u64,
}

pub struct NewSession {
    pub token: String,
    pub id: String,
    pub expires_at: u64,
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn create(db: &Db, label: &str, ttl_ms: u64, now: u64) -> Result<NewSession> {
    let token = b64_encode(&random::bytes::<32>()?);
    let id = hex_encode(&random::bytes::<8>()?);
    let expires_at = now + ttl_ms;
    db.execute("DELETE FROM sessions WHERE expires_at <= ?", vec![n(now)])?;
    db.execute(
        "INSERT INTO sessions (id, token_hash, label, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
        vec![s(&id), b(&token_hash(&token)), s(label), n(now), n(expires_at)],
    )?;
    db.execute(
        "DELETE FROM sessions WHERE id IN
           (SELECT id FROM sessions ORDER BY created_at DESC LIMIT -1 OFFSET ?)",
        vec![n(MAX_SESSIONS)],
    )?;
    Ok(NewSession {
        token,
        id,
        expires_at,
    })
}

/// Looks a token up; expired or unknown tokens yield `None`.
pub fn authenticate(db: &Db, token: &str, now: u64) -> Result<Option<Session>> {
    let row = db.one(
        "SELECT id, expires_at FROM sessions WHERE token_hash = ? AND expires_at > ?",
        vec![b(&token_hash(token)), n(now)],
    )?;
    row.map(|row| {
        Ok(Session {
            id: text(&row, 0)?,
            expires_at: uint(&row, 1)?,
        })
    })
    .transpose()
}

pub fn list(db: &Db, current: &str, now: u64) -> Result<Vec<SessionInfo>> {
    let rows = db.query(
        "SELECT id, label, created_at, expires_at FROM sessions
         WHERE expires_at > ? ORDER BY created_at",
        vec![n(now)],
    )?;
    rows.iter()
        .map(|row: &Row| {
            let id = text(row, 0)?;
            Ok(SessionInfo {
                current: id == current,
                id,
                label: text(row, 1)?,
                created_at: uint(row, 2)?,
                expires_at: uint(row, 3)?,
            })
        })
        .collect()
}

/// Revokes one session; returns whether it existed.
pub fn revoke(db: &Db, id: &str) -> Result<bool> {
    let rows = db.query(
        "DELETE FROM sessions WHERE id = ? RETURNING id",
        vec![s(id)],
    )?;
    Ok(!rows.is_empty())
}

/// Revokes every session but `keep`; returns the ids that were revoked.
pub fn revoke_all_except(db: &Db, keep: &str) -> Result<Vec<String>> {
    let rows = db.query(
        "DELETE FROM sessions WHERE id <> ? RETURNING id",
        vec![s(keep)],
    )?;
    rows.iter().map(|row| text(row, 0)).collect()
}
