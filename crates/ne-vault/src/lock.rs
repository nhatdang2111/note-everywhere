//! Auto-lock and re-authentication policy.
//!
//! This module is pure: it decides, the `Vault` acts. Events come from the
//! caller (see `ne-platform`'s `ActivityMonitor`); nothing here reads a clock
//! or an OS signal.

use std::fmt;
use std::time::Duration;

/// Whether the vault key is in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    /// No key and no decrypted data in memory.
    Locked,
    /// The key and decrypted items are in memory.
    Unlocked,
}

/// Something that happened on the device, reported by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockEvent {
    /// The user has not touched keyboard or mouse for this long. May be
    /// reported repeatedly with growing durations.
    IdleElapsed(Duration),
    /// The computer went to sleep.
    SystemSlept,
    /// The screen was locked.
    ScreenLocked,
    /// The app lost focus or was minimised.
    AppBackgrounded,
}

/// An action that exposes a secret and may demand recent authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveAction {
    /// Showing a field on screen.
    Reveal,
    /// Putting a field on the clipboard.
    Copy,
    /// Writing plaintext items to a file.
    Export,
}

impl fmt::Display for SensitiveAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reveal => "reveal a secret",
            Self::Copy => "copy a secret",
            Self::Export => "export the vault",
        })
    }
}

/// When the vault locks itself and when it asks for the password again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockPolicy {
    /// Lock after this much user inactivity; `None` disables idle locking.
    pub idle_timeout: Option<Duration>,
    /// Lock when the computer sleeps.
    pub lock_on_sleep: bool,
    /// Lock when the screen locks.
    pub lock_on_screen_lock: bool,
    /// Lock when the app is backgrounded.
    pub lock_on_background: bool,
    /// How long after the last password entry a sensitive action is allowed
    /// without asking again. Zero asks every time.
    pub reauth_grace: Duration,
    /// How long a copied secret may stay on the clipboard.
    pub clipboard_clear_after: Duration,
}

impl Default for LockPolicy {
    fn default() -> Self {
        Self {
            idle_timeout: Some(Duration::from_secs(5 * 60)),
            lock_on_sleep: true,
            lock_on_screen_lock: true,
            lock_on_background: false,
            reauth_grace: Duration::from_secs(60),
            clipboard_clear_after: Duration::from_secs(30),
        }
    }
}

impl LockPolicy {
    /// Whether `event` must lock the vault under this policy.
    pub fn should_lock(&self, event: LockEvent) -> bool {
        match event {
            LockEvent::IdleElapsed(idle) => self.idle_timeout.is_some_and(|limit| idle >= limit),
            LockEvent::SystemSlept => self.lock_on_sleep,
            LockEvent::ScreenLocked => self.lock_on_screen_lock,
            LockEvent::AppBackgrounded => self.lock_on_background,
        }
    }

    /// Whether the password must be entered again. A clock that moved backwards
    /// counts as expired.
    pub fn reauth_required(&self, authenticated_at_ms: i64, now_ms: i64) -> bool {
        match u64::try_from(now_ms.saturating_sub(authenticated_at_ms)) {
            Ok(elapsed_ms) => u128::from(elapsed_ms) >= self.reauth_grace.as_millis(),
            Err(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn auto_lock_decisions() {
        let default = LockPolicy::default();
        let never_idle = LockPolicy {
            idle_timeout: None,
            ..default
        };
        let strict = LockPolicy {
            lock_on_background: true,
            lock_on_sleep: false,
            lock_on_screen_lock: false,
            idle_timeout: Some(SEC(10)),
            ..default
        };
        // (policy, event, expected)
        let table = [
            (default, LockEvent::IdleElapsed(SEC(299)), false),
            (default, LockEvent::IdleElapsed(SEC(300)), true),
            (default, LockEvent::IdleElapsed(SEC(9999)), true),
            (default, LockEvent::SystemSlept, true),
            (default, LockEvent::ScreenLocked, true),
            (default, LockEvent::AppBackgrounded, false),
            (never_idle, LockEvent::IdleElapsed(SEC(99999)), false),
            (never_idle, LockEvent::SystemSlept, true),
            (strict, LockEvent::IdleElapsed(SEC(9)), false),
            (strict, LockEvent::IdleElapsed(SEC(10)), true),
            (strict, LockEvent::SystemSlept, false),
            (strict, LockEvent::ScreenLocked, false),
            (strict, LockEvent::AppBackgrounded, true),
        ];
        for (policy, event, expected) in table {
            assert_eq!(policy.should_lock(event), expected, "{event:?}");
        }
    }

    #[test]
    fn reauth_grace_boundaries() {
        let policy = LockPolicy {
            reauth_grace: SEC(60),
            ..LockPolicy::default()
        };
        assert!(!policy.reauth_required(1_000, 1_000));
        assert!(!policy.reauth_required(1_000, 60_999));
        assert!(policy.reauth_required(1_000, 61_000));
        assert!(policy.reauth_required(1_000, 999), "clock went backwards");
        let always = LockPolicy {
            reauth_grace: Duration::ZERO,
            ..policy
        };
        assert!(always.reauth_required(5, 5));
    }
}
