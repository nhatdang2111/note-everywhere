//! Stand-ins for the two OS services that cannot be exercised for real in a
//! test run: the system notification centre and the updater (whose real
//! plugin is covered separately, against a local server).

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use note_everywhere_desktop_lib::app::{
    BoxFuture, Notifier, NotifyPermission, ProgressFn, UpdateBackend, UpdateFailure, UpdateOffer,
};

/// Records what the core asks of the notification centre.
pub struct RecordingNotifier {
    state: Mutex<NotifyPermission>,
    /// What the person answers when asked.
    answer: NotifyPermission,
    pub shown: Mutex<Vec<(String, String)>>,
    pub requests: AtomicUsize,
}

impl RecordingNotifier {
    pub fn new(initial: NotifyPermission, answer: NotifyPermission) -> Self {
        Self {
            state: Mutex::new(initial),
            answer,
            shown: Mutex::new(Vec::new()),
            requests: AtomicUsize::new(0),
        }
    }

    pub fn shown(&self) -> Vec<(String, String)> {
        self.shown.lock().expect("lock").clone()
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Notifier for RecordingNotifier {
    fn permission(&self) -> NotifyPermission {
        *self.state.lock().expect("lock")
    }

    fn request_permission(&self) -> NotifyPermission {
        self.requests.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().expect("lock") = self.answer;
        self.answer
    }

    fn show(&self, title: &str, body: &str) -> Result<(), String> {
        self.shown
            .lock()
            .expect("lock")
            .push((title.to_owned(), body.to_owned()));
        Ok(())
    }
}

/// An updater whose answers the test scripts.
pub struct FakeUpdates {
    configured: bool,
    pub check_result: Mutex<Result<Option<UpdateOffer>, UpdateFailure>>,
    pub download_result: Mutex<Result<(), UpdateFailure>>,
    pub install_result: Mutex<Result<(), UpdateFailure>>,
    /// Progress reports a download makes: (received, total).
    pub chunks: Mutex<Vec<(u64, Option<u64>)>>,
    pub checks: AtomicUsize,
    pub downloads: AtomicUsize,
    pub installs: AtomicUsize,
    pub restarts: AtomicUsize,
}

impl FakeUpdates {
    pub fn new(configured: bool) -> Self {
        Self {
            configured,
            check_result: Mutex::new(Ok(None)),
            download_result: Mutex::new(Ok(())),
            install_result: Mutex::new(Ok(())),
            chunks: Mutex::new(vec![(10, Some(30)), (30, Some(30))]),
            checks: AtomicUsize::new(0),
            downloads: AtomicUsize::new(0),
            installs: AtomicUsize::new(0),
            restarts: AtomicUsize::new(0),
        }
    }

    pub fn count(counter: &AtomicUsize) -> usize {
        counter.load(Ordering::SeqCst)
    }

    pub fn offer(&self, version: &str) {
        *self.check_result.lock().expect("lock") = Ok(Some(UpdateOffer {
            version: version.to_owned(),
            notes: Some("Faster sync.".to_owned()),
        }));
    }
}

impl UpdateBackend for FakeUpdates {
    fn configured(&self) -> bool {
        self.configured
    }

    fn check(&self) -> BoxFuture<'_, Result<Option<UpdateOffer>, UpdateFailure>> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        let result = self.check_result.lock().expect("lock").clone();
        Box::pin(async move { result })
    }

    fn download(&self, progress: ProgressFn) -> BoxFuture<'_, Result<(), UpdateFailure>> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        for (received, total) in self.chunks.lock().expect("lock").iter() {
            progress(*received, *total);
        }
        let result = *self.download_result.lock().expect("lock");
        Box::pin(async move { result })
    }

    fn install(&self) -> Result<(), UpdateFailure> {
        self.installs.fetch_add(1, Ordering::SeqCst);
        *self.install_result.lock().expect("lock")
    }

    fn restart(&self) {
        self.restarts.fetch_add(1, Ordering::SeqCst);
    }
}
