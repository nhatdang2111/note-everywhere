//! Failed-attempt throttling with escalating lockout, persisted so it survives
//! the object being evicted. Buckets exist per account name and per client
//! address; an unknown account name gets a bucket like a real one, so lockout
//! behaviour never reveals whether an account exists.

use ne_sync_protocol::ids::hex_encode;
use sha2::{Digest, Sha256};
use worker::Result;

use crate::config::Config;
use crate::db::{Db, int, n, s, uint};

/// Longest single lockout.
const MAX_LOCK_SECS: u64 = 3600;
/// Failures are forgotten after this long without a new one.
const FORGET_AFTER_MS: u64 = 24 * 3600 * 1000;
/// Buckets kept; beyond this new names are not tracked (the address bucket
/// still applies), which bounds storage under a spraying attack.
const MAX_BUCKETS: i64 = 2000;

/// Lockout length after `failures` consecutive failures: none for the first
/// `free`, then `base` seconds doubling with each failure, capped at an hour.
pub fn lock_secs(failures: u32, free: u32, base: u64) -> u64 {
    if failures < free {
        return 0;
    }
    let doublings = (failures - free).min(16);
    base.saturating_mul(1u64 << doublings).min(MAX_LOCK_SECS)
}

pub fn account_key(username: &str) -> String {
    let digest = Sha256::digest(username.as_bytes());
    format!("acct:{}", hex_encode(&digest[..8]))
}

pub fn ip_key(ip: &str) -> String {
    format!("ip:{ip}")
}

/// Recovery attempts have their own buckets: someone who forgot the password
/// has just failed several logins and must still be able to recover, and
/// guessing at recovery must not lock the owner out of password logins.
pub fn recovery_account_key(username: &str) -> String {
    let digest = Sha256::digest(username.as_bytes());
    format!("rcv:{}", hex_encode(&digest[..8]))
}

pub fn recovery_ip_key(ip: &str) -> String {
    format!("rip:{ip}")
}

/// Seconds the caller must still wait, if any bucket is locked.
pub fn remaining_lock(db: &Db, keys: &[String], now: u64) -> Result<Option<u64>> {
    let mut longest = 0;
    for key in keys {
        if let Some(row) = db.one(
            "SELECT locked_until FROM throttle WHERE key = ?",
            vec![s(key)],
        )? {
            let until = uint(&row, 0)?;
            if until > now {
                longest = longest.max((until - now).div_ceil(1000));
            }
        }
    }
    Ok((longest > 0).then_some(longest))
}

pub fn record_failure(db: &Db, cfg: &Config, keys: &[String], now: u64) -> Result<()> {
    db.execute(
        "DELETE FROM throttle WHERE updated_at < ?",
        vec![n(now.saturating_sub(FORGET_AFTER_MS))],
    )?;
    let tracked = db
        .one("SELECT COUNT(*) FROM throttle", vec![])?
        .map_or(Ok(0), |row| int(&row, 0))?;
    for key in keys {
        let previous = db
            .one("SELECT failures FROM throttle WHERE key = ?", vec![s(key)])?
            .map(|row| uint(&row, 0))
            .transpose()?;
        if previous.is_none() && tracked >= MAX_BUCKETS {
            continue;
        }
        let failures = u32::try_from(previous.unwrap_or(0) + 1).unwrap_or(u32::MAX);
        let locked_until = now + lock_secs(failures, cfg.free_attempts, cfg.lock_base_secs) * 1000;
        db.execute(
            "INSERT OR REPLACE INTO throttle (key, failures, locked_until, updated_at)
             VALUES (?, ?, ?, ?)",
            vec![s(key), n(u64::from(failures)), n(locked_until), n(now)],
        )?;
    }
    Ok(())
}

pub fn record_success(db: &Db, keys: &[String]) -> Result<()> {
    for key in keys {
        db.execute("DELETE FROM throttle WHERE key = ?", vec![s(key)])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lock_before_the_free_attempts_are_used() {
        assert_eq!(lock_secs(0, 5, 30), 0);
        assert_eq!(lock_secs(4, 5, 30), 0);
    }

    #[test]
    fn lock_doubles_per_failure_and_is_capped() {
        assert_eq!(lock_secs(5, 5, 30), 30);
        assert_eq!(lock_secs(6, 5, 30), 60);
        assert_eq!(lock_secs(7, 5, 30), 120);
        assert_eq!(lock_secs(40, 5, 30), 3600);
        assert_eq!(lock_secs(u32::MAX, 5, u64::MAX), 3600);
    }

    #[test]
    fn recovery_buckets_are_separate_from_login_buckets() {
        assert_ne!(account_key("alice"), recovery_account_key("alice"));
        assert_ne!(ip_key("1.2.3.4"), recovery_ip_key("1.2.3.4"));
        assert!(!recovery_account_key("alice").contains("alice"));
    }

    /// Recovery uses the login policy: free attempts, then doubling to an hour.
    #[test]
    fn recovery_lockout_follows_the_login_escalation() {
        let (free, base) = (5, 30);
        let schedule: Vec<u64> = (1..=9).map(|n| lock_secs(n, free, base)).collect();
        assert_eq!(schedule, [0, 0, 0, 0, 30, 60, 120, 240, 480]);
        assert_eq!(lock_secs(5 + 7, free, base), 3600);
        assert_eq!(lock_secs(5 + 30, free, base), 3600);
    }

    #[test]
    fn account_keys_do_not_expose_the_name() {
        let key = account_key("alice");
        assert!(key.starts_with("acct:") && !key.contains("alice"));
        assert_ne!(key, account_key("bob"));
    }
}
