//! Idle and sleep signals for the vault's auto-lock.
//!
//! [`ActivityMonitor`] is polled by the app on a timer (every few seconds) and
//! returns the [`LockEvent`]s that `ne_vault::Vault::handle_event` consumes.
//!
//! # What is detected
//!
//! * **Idle**: seconds since the last keyboard or mouse input, read through the
//!   `user-idle2` crate (IOKit on macOS, `GetLastInputInfo` on Windows; the
//!   unsafe calls live in that crate, none here). Reported once when the idle
//!   time crosses the configured threshold, and again only after the user has
//!   been active in between.
//! * **System sleep**: inferred by comparing how far the monotonic clock and the
//!   wall clock advanced between two polls, plus a plain "this poll came much
//!   later than expected" check. See [`SleepDetector`] for the limits.
//!
//! # What is not detected
//!
//! **Screen lock.** Seeing it needs OS notifications (macOS distributed
//! notifications, Windows session-change messages), which cannot be done
//! without `unsafe` or an Objective-C bridge. Nothing here reports
//! [`LockEvent::ScreenLocked`]; the desktop shell should feed it from its own
//! window or session events when available. Until then a locked screen is only
//! covered by the idle timeout (a locked screen produces no input) and by
//! system sleep.
//!
//! The app-backgrounded event likewise comes from the window layer, not here.

use std::time::{Duration, Instant, SystemTime};

use ne_vault::LockEvent;

/// Reading the idle time failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActivityError {
    /// The platform did not give an idle time (no display session, denied).
    #[error("cannot read the idle time: {0}")]
    IdleUnavailable(String),
}

/// Where the idle time comes from.
pub trait IdleSource {
    /// Time since the last user input.
    fn idle_time(&self) -> Result<Duration, ActivityError>;
}

/// The operating system's idle time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemIdle;

impl IdleSource for SystemIdle {
    fn idle_time(&self) -> Result<Duration, ActivityError> {
        user_idle2::UserIdle::get_time()
            .map(|idle| idle.duration())
            .map_err(|err| ActivityError::IdleUnavailable(err.to_string()))
    }
}

/// Detects that the computer was suspended between two observations.
///
/// Two signals, either one counts:
///
/// * the wall clock advanced `threshold` more than the monotonic clock. This
///   works where the monotonic clock stops during sleep (macOS), and also
///   fires when the wall clock is stepped forward by a clock adjustment;
/// * the observation arrived `threshold` later than `expected_interval`. This
///   covers systems whose monotonic clock keeps running through sleep, but it
///   also fires when the app was merely frozen (a debugger, a stalled UI).
///
/// Both false positives lock the vault, which is the safe side. A sleep shorter
/// than `threshold` is missed, as is a sleep whose wake-up is never observed
/// because the process was killed. The wall clock moving backwards is ignored.
#[derive(Debug, Clone)]
pub struct SleepDetector {
    last_mono: Instant,
    last_wall: SystemTime,
    expected_interval: Duration,
    threshold: Duration,
}

impl SleepDetector {
    /// Starts watching from the given moment.
    pub fn new(
        mono: Instant,
        wall: SystemTime,
        expected_interval: Duration,
        threshold: Duration,
    ) -> Self {
        Self {
            last_mono: mono,
            last_wall: wall,
            expected_interval,
            threshold,
        }
    }

    /// Records a new observation; true when a suspension happened since the
    /// previous one.
    pub fn observe(&mut self, mono: Instant, wall: SystemTime) -> bool {
        let mono_delta = mono.saturating_duration_since(self.last_mono);
        let wall_delta = wall.duration_since(self.last_wall).unwrap_or_default();
        self.last_mono = mono;
        self.last_wall = wall;
        wall_delta >= mono_delta.saturating_add(self.threshold)
            || mono_delta >= self.expected_interval.saturating_add(self.threshold)
    }
}

/// Timing settings of an [`ActivityMonitor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorConfig {
    /// How often the app polls.
    pub poll_interval: Duration,
    /// How much slack separates a sleep from ordinary scheduling jitter.
    pub sleep_threshold: Duration,
    /// Report [`LockEvent::IdleElapsed`] once idle time reaches this; usually
    /// the vault's idle timeout. `None` reports no idle events.
    pub idle_report_after: Option<Duration>,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            sleep_threshold: Duration::from_secs(15),
            idle_report_after: Some(Duration::from_secs(5 * 60)),
        }
    }
}

/// The result of one poll.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PollReport {
    /// Events for `Vault::handle_event`, in the order they were noticed.
    pub events: Vec<LockEvent>,
    /// Set when the idle time could not be read. Sleep detection still ran.
    /// The caller decides whether an unreadable idle time should lock.
    pub idle_error: Option<ActivityError>,
}

/// Polling source of lock events.
#[derive(Debug)]
pub struct ActivityMonitor<S: IdleSource = SystemIdle> {
    source: S,
    detector: SleepDetector,
    idle_report_after: Option<Duration>,
    idle_reported: bool,
}

impl ActivityMonitor<SystemIdle> {
    /// A monitor reading the real idle time, starting now.
    pub fn new(config: MonitorConfig) -> Self {
        Self::with_source(SystemIdle, config, Instant::now(), SystemTime::now())
    }
}

impl<S: IdleSource> ActivityMonitor<S> {
    /// A monitor with an explicit idle source and starting moment.
    pub fn with_source(source: S, config: MonitorConfig, mono: Instant, wall: SystemTime) -> Self {
        Self {
            source,
            detector: SleepDetector::new(mono, wall, config.poll_interval, config.sleep_threshold),
            idle_report_after: config.idle_report_after,
            idle_reported: false,
        }
    }

    /// Changes the idle threshold, for example after the user edits the
    /// auto-lock timeout.
    pub fn set_idle_report_after(&mut self, after: Option<Duration>) {
        self.idle_report_after = after;
    }

    /// Polls using the current clocks.
    pub fn poll(&mut self) -> PollReport {
        self.poll_at(Instant::now(), SystemTime::now())
    }

    /// Polls at explicit clock readings (for tests).
    pub fn poll_at(&mut self, mono: Instant, wall: SystemTime) -> PollReport {
        let mut report = PollReport::default();
        if self.detector.observe(mono, wall) {
            report.events.push(LockEvent::SystemSlept);
        }
        match self.source.idle_time() {
            Ok(idle) => {
                let crossed = self.idle_report_after.is_some_and(|after| idle >= after);
                if crossed && !self.idle_reported {
                    report.events.push(LockEvent::IdleElapsed(idle));
                }
                self.idle_reported = crossed;
            }
            Err(err) => report.idle_error = Some(err),
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::UNIX_EPOCH;

    use super::*;

    const SEC: fn(u64) -> Duration = Duration::from_secs;

    struct FakeIdle(Cell<Result<Duration, ()>>);

    impl IdleSource for FakeIdle {
        fn idle_time(&self) -> Result<Duration, ActivityError> {
            self.0
                .get()
                .map_err(|()| ActivityError::IdleUnavailable("no display".into()))
        }
    }

    fn detector(mono: Instant) -> SleepDetector {
        SleepDetector::new(mono, UNIX_EPOCH + SEC(1_000_000), SEC(5), SEC(15))
    }

    #[test]
    fn sleep_detection_table() {
        let t0 = Instant::now();
        let w0 = UNIX_EPOCH + SEC(1_000_000);
        // (monotonic advance, wall advance, expected)
        let table = [
            (5, 5, false),      // on time
            (6, 5, false),      // jitter
            (5, 10, false),     // small clock nudge
            (5, 20, true),      // wall ran ahead: monotonic clock paused in sleep
            (5, 3600, true),    // long sleep, monotonic paused
            (3600, 3600, true), // both clocks kept running; poll came very late
            (22, 22, true),     // late poll beyond interval + threshold
            (19, 19, false),    // late but inside interval + threshold
            (5, 0, false),      // wall clock stepped backwards is ignored
        ];
        for (mono, wall, expected) in table {
            let mut d = detector(t0);
            assert_eq!(
                d.observe(t0 + SEC(mono), w0 + SEC(wall)),
                expected,
                "{mono}/{wall}"
            );
        }
    }

    #[test]
    fn detector_compares_consecutive_observations() {
        let t0 = Instant::now();
        let w0 = UNIX_EPOCH + SEC(1_000_000);
        let mut d = detector(t0);
        assert!(!d.observe(t0 + SEC(5), w0 + SEC(5)));
        assert!(
            d.observe(t0 + SEC(10), w0 + SEC(5 + 600)),
            "sleep in the second interval"
        );
        assert!(
            !d.observe(t0 + SEC(15), w0 + SEC(5 + 600 + 5)),
            "back to normal"
        );
    }

    fn monitor(
        idle: Result<Duration, ()>,
        after: Option<Duration>,
    ) -> (ActivityMonitor<FakeIdle>, Instant) {
        let t0 = Instant::now();
        let config = MonitorConfig {
            idle_report_after: after,
            ..MonitorConfig::default()
        };
        let monitor = ActivityMonitor::with_source(
            FakeIdle(Cell::new(idle)),
            config,
            t0,
            UNIX_EPOCH + SEC(1_000_000),
        );
        (monitor, t0)
    }

    fn poll(m: &mut ActivityMonitor<FakeIdle>, t0: Instant, at: u64) -> PollReport {
        m.poll_at(t0 + SEC(at), UNIX_EPOCH + SEC(1_000_000 + at))
    }

    #[test]
    fn idle_is_reported_once_per_idle_period() {
        let (mut m, t0) = monitor(Ok(SEC(10)), Some(SEC(300)));
        assert!(poll(&mut m, t0, 5).events.is_empty());
        m.source.0.set(Ok(SEC(300)));
        assert_eq!(
            poll(&mut m, t0, 10).events,
            [LockEvent::IdleElapsed(SEC(300))]
        );
        m.source.0.set(Ok(SEC(305)));
        assert!(
            poll(&mut m, t0, 15).events.is_empty(),
            "not repeated while still idle"
        );
        m.source.0.set(Ok(SEC(2)));
        assert!(poll(&mut m, t0, 20).events.is_empty(), "user came back");
        m.source.0.set(Ok(SEC(301)));
        assert_eq!(
            poll(&mut m, t0, 25).events,
            [LockEvent::IdleElapsed(SEC(301))]
        );
    }

    #[test]
    fn disabled_threshold_reports_nothing_and_can_be_changed() {
        let (mut m, t0) = monitor(Ok(SEC(9999)), None);
        assert!(poll(&mut m, t0, 5).events.is_empty());
        m.set_idle_report_after(Some(SEC(60)));
        assert_eq!(
            poll(&mut m, t0, 10).events,
            [LockEvent::IdleElapsed(SEC(9999))]
        );
    }

    #[test]
    fn sleep_and_idle_can_arrive_together_and_errors_do_not_hide_sleep() {
        let (mut m, t0) = monitor(Ok(SEC(900)), Some(SEC(300)));
        let report = m.poll_at(t0 + SEC(5), UNIX_EPOCH + SEC(1_000_000 + 900));
        assert_eq!(
            report.events,
            [LockEvent::SystemSlept, LockEvent::IdleElapsed(SEC(900))]
        );

        let (mut m, t0) = monitor(Err(()), Some(SEC(300)));
        let report = m.poll_at(t0 + SEC(5), UNIX_EPOCH + SEC(1_000_000 + 900));
        assert_eq!(report.events, [LockEvent::SystemSlept]);
        assert!(report.idle_error.is_some());
    }

    /// Reads the real idle time; needs a desktop session (fails on headless
    /// machines).
    #[test]
    #[ignore = "needs a logged-in desktop session"]
    fn real_idle_time_is_readable() {
        let idle = SystemIdle.idle_time().expect("idle time");
        assert!(idle < Duration::from_secs(60 * 60 * 24 * 365));
        let mut monitor = ActivityMonitor::new(MonitorConfig::default());
        assert!(monitor.poll().idle_error.is_none());
    }
}
