//! Injected time source, so timestamps and grace periods are testable.

use std::time::{SystemTime, UNIX_EPOCH};

/// Source of wall-clock time in milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now_ms(&self) -> i64;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        // A clock before 1970 reads as 0 rather than failing.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}
