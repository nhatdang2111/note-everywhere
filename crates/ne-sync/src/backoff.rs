//! Exponential backoff with jitter for reconnects and retries.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(500),
            max: Duration::from_secs(60),
        }
    }
}

impl Backoff {
    /// The wait before retry number `attempt` (0 for the first retry).
    /// `jitter` is a uniform value in `[0, 1)`; the result lies between half
    /// of the exponential delay and all of it, which keeps devices from
    /// retrying in lockstep without ever waiting less than half the delay.
    pub fn delay(&self, attempt: u32, jitter: f64) -> Duration {
        let factor = 1u32.checked_shl(attempt.min(30)).unwrap_or(u32::MAX);
        let ceiling = self.base.saturating_mul(factor).min(self.max);
        ceiling.mul_f64(0.5 + 0.5 * jitter.clamp(0.0, 1.0))
    }

    /// [`delay`](Self::delay) with a fresh random jitter.
    pub fn next(&self, attempt: u32) -> Duration {
        self.delay(attempt, crate::random::unit_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: Backoff = Backoff {
        base: Duration::from_millis(500),
        max: Duration::from_secs(60),
    };

    #[test]
    fn delay_doubles_until_the_cap() {
        let full = |attempt| B.delay(attempt, 1.0);
        assert_eq!(full(0), Duration::from_millis(500));
        assert_eq!(full(1), Duration::from_secs(1));
        assert_eq!(full(2), Duration::from_secs(2));
        assert_eq!(full(7), Duration::from_secs(60));
        assert_eq!(full(1000), Duration::from_secs(60));
    }

    #[test]
    fn jitter_spreads_between_half_and_full() {
        assert_eq!(B.delay(3, 0.0), Duration::from_secs(2));
        assert_eq!(B.delay(3, 1.0), Duration::from_secs(4));
        assert_eq!(B.delay(3, 0.5), Duration::from_secs(3));
        // Out-of-range jitter is clamped rather than producing a wild delay.
        assert_eq!(B.delay(3, 9.0), B.delay(3, 1.0));
        assert_eq!(B.delay(3, -1.0), B.delay(3, 0.0));
    }

    #[test]
    fn random_delays_stay_within_bounds() {
        for attempt in 0..12 {
            let d = B.next(attempt);
            let ceiling = B.delay(attempt, 1.0);
            assert!(d >= ceiling / 2 && d <= ceiling, "attempt {attempt}: {d:?}");
        }
    }
}
