//! Settings read from the Worker environment (vars and secrets).

use ne_sync_protocol::limits::{DEFAULT_BLOB_MAX, MAX_BLOB_CEILING};
use worker::Env;

const DEFAULT_QUOTA: u64 = 8 * 1024 * 1024 * 1024;
const DEFAULT_SESSION_DAYS: u64 = 30;
const DEFAULT_FREE_ATTEMPTS: u32 = 5;
const DEFAULT_LOCK_BASE_SECS: u64 = 30;
const DEFAULT_TOUCH_SECS: u64 = 600;

#[derive(Debug, Clone)]
pub struct Config {
    /// One-time code that authorises creating the first account.
    pub setup_code: Option<String>,
    /// Largest attachment object, never above the free-plan body limit.
    pub blob_max: u64,
    /// Total bytes of attachments the index allows.
    pub blob_quota: u64,
    pub session_ttl_ms: u64,
    /// Failed attempts tolerated before a lockout starts.
    pub free_attempts: u32,
    /// First lockout length; doubles with each further failure.
    pub lock_base_secs: u64,
    /// A session's last activity is written at most this often.
    pub touch_ms: u64,
    /// Enables the `/v1/_test/*` routes. Local development only.
    pub test_inspect: bool,
}

fn read(env: &Env, name: &str) -> Option<String> {
    let value = env
        .secret(name)
        .map(|v| v.to_string())
        .or_else(|_| env.var(name).map(|v| v.to_string()))
        .ok()?;
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn number<T: std::str::FromStr>(env: &Env, name: &str) -> Option<T> {
    read(env, name)?.parse().ok()
}

impl Config {
    pub fn from_env(env: &Env) -> Self {
        Self {
            setup_code: read(env, "SETUP_CODE"),
            blob_max: number(env, "BLOB_MAX_BYTES")
                .unwrap_or(DEFAULT_BLOB_MAX)
                .clamp(1, MAX_BLOB_CEILING),
            blob_quota: number(env, "BLOB_QUOTA_BYTES").unwrap_or(DEFAULT_QUOTA),
            session_ttl_ms: number::<u64>(env, "SESSION_TTL_DAYS")
                .unwrap_or(DEFAULT_SESSION_DAYS)
                .clamp(1, 365)
                * 86_400_000,
            free_attempts: number::<u32>(env, "LOCKOUT_FREE_ATTEMPTS")
                .unwrap_or(DEFAULT_FREE_ATTEMPTS)
                .clamp(1, 100),
            lock_base_secs: number::<u64>(env, "LOCKOUT_BASE_SECONDS")
                .unwrap_or(DEFAULT_LOCK_BASE_SECS)
                .clamp(1, 3600),
            touch_ms: number::<u64>(env, "SESSION_TOUCH_SECONDS")
                .unwrap_or(DEFAULT_TOUCH_SECS)
                .min(86_400)
                * 1000,
            test_inspect: read(env, "TEST_INSPECT").is_some_and(|v| v == "1"),
        }
    }
}
