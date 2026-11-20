//! Attachments: content-addressed files beside the database, with reference
//! counts kept in SQLite.
//!
//! A blob is stored once, under the SHA-256 of its content, as
//! `attachments/<first two hex digits>/<hash>`. It is written to a temporary
//! file, synced and renamed into place, so a crash leaves either the whole
//! file or none of it. The database row is added after the file; a crash in
//! between leaves an unreferenced file that [`sweep`] removes later.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ne_core::page::ATTACHMENT_HASH_LEN;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};

const TMP_DIR: &str = "tmp";

/// What is known about a stored attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    /// Lowercase hex SHA-256 of the content.
    pub hash: String,
    /// The name the file had when it was first added.
    pub file_name: String,
    /// Size in bytes.
    pub size: u64,
    /// How many pages show it.
    pub ref_count: u64,
    /// When it was first added, in milliseconds since the Unix epoch.
    pub created_at: i64,
}

/// What a garbage collection removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcReport {
    /// Unreferenced attachments removed.
    pub removed: usize,
    /// Bytes freed by them.
    pub bytes_freed: u64,
    /// Files on disk without a database row (left by an interrupted add)
    /// removed as well.
    pub orphan_files: usize,
}

/// True for a well-formed attachment hash: 64 lowercase hex digits. Hashes
/// become file names, so anything else is refused.
pub fn is_valid_hash(hash: &str) -> bool {
    hash.len() == ATTACHMENT_HASH_LEN
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn sync_dir(path: &Path) {
    // Syncing a directory entry is best effort: it hardens the rename against
    // power loss on systems that allow opening a directory.
    #[cfg(unix)]
    if let Ok(dir) = File::open(path) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The attachment directory of one workspace.
pub(crate) struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Opens the store, creating its directory and clearing temporary files
    /// an earlier crash left behind.
    pub(crate) fn open(root: PathBuf) -> Result<Self> {
        let tmp = root.join(TMP_DIR);
        fs::create_dir_all(&tmp).map_err(|e| Error::io(&tmp, &e))?;
        if let Ok(entries) = fs::read_dir(&tmp) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(Self { root })
    }

    fn path_of(&self, hash: &str) -> Result<PathBuf> {
        if !is_valid_hash(hash) {
            return Err(Error::InvalidInput(format!(
                "{hash:?} is not an attachment hash"
            )));
        }
        Ok(self.root.join(&hash[..2]).join(hash))
    }

    /// Stores `bytes` and returns their hash. Storing the same content again
    /// writes nothing.
    pub(crate) fn put_bytes(&self, bytes: &[u8]) -> Result<String> {
        let hash = hex(&Sha256::digest(bytes));
        let target = self.path_of(&hash)?;
        if fs::metadata(&target).is_ok_and(|m| m.len() == bytes.len() as u64) {
            return Ok(hash);
        }
        let tmp = self.root.join(TMP_DIR).join(Uuid::new_v4().to_string());
        let write = || -> std::io::Result<()> {
            let mut file = File::create(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()
        };
        write().map_err(|e| Error::io(&tmp, &e))?;
        self.install(&tmp, &target)?;
        Ok(hash)
    }

    /// Stores the content of a file without loading it into memory. Returns
    /// the hash and the size.
    pub(crate) fn put_file(&self, source: &Path) -> Result<(String, u64)> {
        let tmp = self.root.join(TMP_DIR).join(Uuid::new_v4().to_string());
        let copy = || -> std::io::Result<(String, u64)> {
            let mut input = File::open(source)?;
            let mut output = File::create(&tmp)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 64 * 1024];
            let mut size = 0u64;
            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                output.write_all(&buffer[..read])?;
                size += read as u64;
            }
            output.sync_all()?;
            Ok((hex(&hasher.finalize()), size))
        };
        let (hash, size) = copy().map_err(|e| {
            let _ = fs::remove_file(&tmp);
            Error::io(source, &e)
        })?;
        let target = self.path_of(&hash)?;
        if fs::metadata(&target).is_ok_and(|m| m.len() == size) {
            let _ = fs::remove_file(&tmp);
        } else {
            self.install(&tmp, &target)?;
        }
        Ok((hash, size))
    }

    fn install(&self, tmp: &Path, target: &Path) -> Result<()> {
        let parent = target.parent().unwrap_or(&self.root);
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, &e))?;
        fs::rename(tmp, target).map_err(|e| {
            let _ = fs::remove_file(tmp);
            Error::io(target, &e)
        })?;
        sync_dir(parent);
        Ok(())
    }

    /// Reads a stored blob, checking it still has the recorded size.
    pub(crate) fn read(&self, hash: &str, expected_size: u64) -> Result<Vec<u8>> {
        let path = self.path_of(hash)?;
        let bytes = fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(format!("attachment file {hash}")),
            _ => Error::io(&path, &e),
        })?;
        if bytes.len() as u64 != expected_size {
            return Err(Error::Corrupt(format!(
                "attachment {hash} has {} bytes, expected {expected_size}",
                bytes.len()
            )));
        }
        Ok(bytes)
    }

    /// Path of a stored blob, for callers that stream it.
    pub(crate) fn existing_path(&self, hash: &str) -> Result<PathBuf> {
        let path = self.path_of(hash)?;
        if path.is_file() {
            Ok(path)
        } else {
            Err(Error::NotFound(format!("attachment file {hash}")))
        }
    }

    pub(crate) fn remove(&self, hash: &str) -> Result<u64> {
        let path = self.path_of(hash)?;
        let size = fs::metadata(&path).map_or(0, |m| m.len());
        match fs::remove_file(&path) {
            Ok(()) => Ok(size),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(Error::io(&path, &e)),
        }
    }

    /// Removes files whose hash is not in `known` and that are older than
    /// `min_age`. Returns how many.
    pub(crate) fn sweep(&self, known: &HashSet<String>, min_age: Duration) -> Result<usize> {
        let mut removed = 0;
        let Ok(shards) = fs::read_dir(&self.root) else {
            return Ok(0);
        };
        for shard in shards.flatten() {
            if shard.file_name() == TMP_DIR || !shard.path().is_dir() {
                continue;
            }
            let Ok(files) = fs::read_dir(shard.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                let old_enough = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .is_some_and(|age| age >= min_age);
                if is_valid_hash(&name) && !known.contains(&name) && old_enough {
                    fs::remove_file(file.path()).map_err(|e| Error::io(file.path(), &e))?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

// ---------------------------------------------------------------------------
// Database rows
// ---------------------------------------------------------------------------

pub(crate) fn register(
    conn: &Connection,
    hash: &str,
    file_name: &str,
    size: u64,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO attachments (hash, file_name, size, created_at, ref_count)
         VALUES (?1, ?2, ?3, ?4, (SELECT COUNT(*) FROM attachment_refs WHERE hash = ?1))",
        params![
            hash,
            file_name,
            i64::try_from(size).unwrap_or(i64::MAX),
            now
        ],
    )?;
    Ok(())
}

pub(crate) fn info(conn: &Connection, hash: &str) -> Result<Option<AttachmentInfo>> {
    Ok(conn
        .query_row(
            "SELECT hash, file_name, size, ref_count, created_at FROM attachments WHERE hash = ?1",
            [hash],
            |row| {
                Ok(AttachmentInfo {
                    hash: row.get(0)?,
                    file_name: row.get(1)?,
                    size: row.get::<_, i64>(2)?.try_into().unwrap_or_default(),
                    ref_count: row.get::<_, i64>(3)?.try_into().unwrap_or_default(),
                    created_at: row.get(4)?,
                })
            },
        )
        .optional()?)
}

fn recount(conn: &Connection, hashes: &HashSet<String>) -> Result<()> {
    let mut statement = conn.prepare_cached(
        "UPDATE attachments
         SET ref_count = (SELECT COUNT(*) FROM attachment_refs r WHERE r.hash = attachments.hash)
         WHERE hash = ?1",
    )?;
    for hash in hashes {
        statement.execute([hash])?;
    }
    Ok(())
}

/// Records that `page_id` shows exactly `hashes` and updates the counts of
/// every attachment gained or lost.
pub(crate) fn set_page_refs(conn: &Connection, page_id: &str, hashes: &[String]) -> Result<()> {
    let mut old: HashSet<String> = HashSet::new();
    {
        let mut statement =
            conn.prepare_cached("SELECT hash FROM attachment_refs WHERE page_id = ?1")?;
        for hash in statement.query_map([page_id], |row| row.get::<_, String>(0))? {
            old.insert(hash?);
        }
    }
    let new: HashSet<String> = hashes.iter().cloned().collect();
    for gone in old.difference(&new) {
        conn.execute(
            "DELETE FROM attachment_refs WHERE page_id = ?1 AND hash = ?2",
            params![page_id, gone],
        )?;
    }
    for added in new.difference(&old) {
        conn.execute(
            "INSERT INTO attachment_refs (page_id, hash) VALUES (?1, ?2)",
            params![page_id, added],
        )?;
    }
    let touched: HashSet<String> = old.symmetric_difference(&new).cloned().collect();
    recount(conn, &touched)
}

/// Drops every reference of a page (it was deleted).
pub(crate) fn clear_page_refs(conn: &Connection, page_id: &str) -> Result<()> {
    set_page_refs(conn, page_id, &[])
}

/// Removes the rows of attachments nobody shows that were added at or before
/// `cutoff`. Returns their hashes; the caller deletes the files.
pub(crate) fn drop_unreferenced(conn: &Connection, cutoff: i64) -> Result<Vec<(String, u64)>> {
    let mut statement = conn
        .prepare("SELECT hash, size FROM attachments WHERE ref_count = 0 AND created_at <= ?1")?;
    let rows: Vec<(String, i64)> = statement
        .query_map([cutoff], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (hash, _) in &rows {
        conn.execute("DELETE FROM attachments WHERE hash = ?1", [hash])?;
    }
    Ok(rows
        .into_iter()
        .map(|(hash, size)| (hash, size.try_into().unwrap_or_default()))
        .collect())
}

pub(crate) fn known_hashes(conn: &Connection) -> Result<HashSet<String>> {
    let mut statement = conn.prepare("SELECT hash FROM attachments")?;
    let hashes = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(hashes)
}
