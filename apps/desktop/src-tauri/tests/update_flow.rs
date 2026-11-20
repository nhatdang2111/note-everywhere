//! The update flow in the core: when it checks, that it never installs by
//! itself, progress, failures, and the build without an updater key.

mod support;

use std::sync::Arc;

use note_everywhere_desktop_lib::app::update::UPDATE_CHECK_INTERVAL;
use note_everywhere_desktop_lib::app::{Deps, UpdateBackend, UpdateFailure, UpdateState};
use serde_json::json;
use support::fakes::FakeUpdates;
use support::{Device, Services};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const NOW: i64 = 1_900_000_000_000;

fn unconfigured() -> Services {
    Services {
        updater_configured: false,
        ..Services::default()
    }
}

#[tokio::test]
async fn a_build_without_a_key_reports_updates_as_unavailable_and_stays_quiet() {
    let device = Device::with_services(unconfigured()).await;
    let status = device.core.update_status().await.expect("status");
    assert_eq!(status.state, UpdateState::Unavailable);
    assert_eq!(status.current_version, device.core.app_version());
    assert!(
        status
            .message
            .as_deref()
            .is_some_and(|m| m.contains("no update signing key")),
        "{status:?}"
    );

    // Neither the launch check nor a manual one reaches the network.
    let launch = device.core.update_check_if_due(NOW).await.expect("launch");
    let manual = device.core.update_check().await.expect("manual");
    assert_eq!(
        (launch.state, manual.state),
        (UpdateState::Unavailable, UpdateState::Unavailable)
    );
    assert_eq!(FakeUpdates::count(&device.updates.checks), 0);
    assert_eq!(
        device
            .core
            .update_download()
            .await
            .expect_err("nothing")
            .code,
        "no_update"
    );
    assert_eq!(
        device
            .core
            .update_install()
            .await
            .expect_err("nothing")
            .code,
        "update_not_ready"
    );
}

#[tokio::test]
async fn a_check_download_install_run_asks_the_person_at_each_step() {
    let device = Device::with_services(Services::default()).await;
    assert_eq!(
        device.core.update_status().await.expect("s").state,
        UpdateState::Idle
    );
    device.updates.offer("0.2.0");
    device.events.clear();

    let found = device.core.update_check().await.expect("check");
    assert_eq!(found.state, UpdateState::Available);
    assert_eq!(found.version.as_deref(), Some("0.2.0"));
    assert_eq!(found.notes.as_deref(), Some("Faster sync."));
    assert!(found.last_checked_at.is_some());
    // Finding an update installs and downloads nothing.
    assert_eq!(FakeUpdates::count(&device.updates.downloads), 0);
    assert_eq!(FakeUpdates::count(&device.updates.installs), 0);
    assert_eq!(
        device
            .core
            .update_install()
            .await
            .expect_err("not yet")
            .code,
        "update_not_ready"
    );

    device.events.clear();
    let ready = device.core.update_download().await.expect("download");
    assert_eq!(ready.state, UpdateState::Ready);
    assert_eq!(FakeUpdates::count(&device.updates.installs), 0);
    let progress: Vec<_> = device
        .events
        .named("update:progress")
        .iter()
        .map(|e| e.payload())
        .collect();
    assert_eq!(
        progress.last(),
        Some(&json!({ "receivedBytes": 30, "totalBytes": 30 }))
    );
    let states: Vec<_> = device
        .events
        .named("update:status")
        .iter()
        .map(|e| e.payload()["state"].clone())
        .collect();
    assert_eq!(states, [json!("downloading"), json!("ready")]);

    // A check while a verified package waits does not discard it.
    assert_eq!(
        device.core.update_check().await.expect("again").state,
        UpdateState::Ready
    );

    device.core.update_install().await.expect("install");
    assert_eq!(FakeUpdates::count(&device.updates.installs), 1);
    assert_eq!(FakeUpdates::count(&device.updates.restarts), 1);
}

#[tokio::test]
async fn nothing_newer_means_up_to_date() {
    let device = Device::with_services(Services::default()).await;
    let status = device.core.update_check().await.expect("check");
    assert_eq!(status.state, UpdateState::UpToDate);
    assert_eq!(status.version, None);
    assert_eq!(
        device.core.update_download().await.expect_err("none").code,
        "no_update"
    );
}

#[tokio::test]
async fn a_failed_check_is_a_status_and_leaves_the_last_check_time_alone() {
    let device = Device::with_services(Services::default()).await;
    *device.updates.check_result.lock().expect("lock") = Err(UpdateFailure::Unreachable);
    let status = device.core.update_check().await.expect("resolves");
    assert_eq!(status.state, UpdateState::Error);
    assert_eq!(status.last_checked_at, None);
    assert_eq!(
        status.message.as_deref(),
        Some("The update server could not be reached.")
    );
    // The failure was not a completed check, so the next launch tries again.
    let again = device.core.update_check_if_due(NOW).await.expect("due");
    assert_eq!(FakeUpdates::count(&device.updates.checks), 2);
    assert_eq!(again.state, UpdateState::Error);
}

#[tokio::test]
async fn a_failed_download_can_be_retried_and_a_bad_signature_is_named() {
    let device = Device::with_services(Services::default()).await;
    device.updates.offer("0.2.0");
    device.core.update_check().await.expect("check");

    *device.updates.download_result.lock().expect("lock") = Err(UpdateFailure::BadSignature);
    let failed = device.core.update_download().await.expect("resolves");
    assert_eq!(failed.state, UpdateState::Available);
    assert!(
        failed
            .message
            .as_deref()
            .is_some_and(|m| m.contains("signature verification")),
        "{failed:?}"
    );
    assert_eq!(
        device
            .core
            .update_install()
            .await
            .expect_err("nothing verified")
            .code,
        "update_not_ready"
    );

    *device.updates.download_result.lock().expect("lock") = Ok(());
    let ok = device.core.update_download().await.expect("retry");
    assert_eq!((ok.state, ok.message), (UpdateState::Ready, None));
}

#[tokio::test]
async fn a_failed_install_goes_back_to_available_and_does_not_restart() {
    let device = Device::with_services(Services::default()).await;
    device.updates.offer("0.2.0");
    device.core.update_check().await.expect("check");
    device.core.update_download().await.expect("download");
    *device.updates.install_result.lock().expect("lock") = Err(UpdateFailure::Other);

    let err = device.core.update_install().await.expect_err("fails");
    assert_eq!(err.code, "update_failed");
    assert_eq!(FakeUpdates::count(&device.updates.restarts), 0);
    assert_eq!(
        device.core.update_status().await.expect("s").state,
        UpdateState::Available
    );
}

#[tokio::test]
async fn the_launch_check_runs_at_most_once_a_day_even_across_restarts() {
    let device = Device::with_services(Services::default()).await;
    device.updates.offer("0.2.0");

    let first = device.core.update_check_if_due(NOW).await.expect("first");
    assert_eq!(first.state, UpdateState::Available);
    assert_eq!(FakeUpdates::count(&device.updates.checks), 1);
    // The completed check is stamped with the real clock.
    let last = first.last_checked_at.expect("stamped");

    let soon = device
        .core
        .update_check_if_due(last + DAY_MS - 1)
        .await
        .expect("soon");
    assert_eq!(soon.state, UpdateState::Available);
    assert_eq!(
        FakeUpdates::count(&device.updates.checks),
        1,
        "still the same day"
    );

    // A new launch reads the time of the last check from disk.
    let (dir, secrets) = (device.dir.clone(), device.secrets.clone());
    device.core.shutdown().await;
    drop(device);
    let relaunched = Device::open_in(dir, secrets).await;
    relaunched.updates.offer("0.2.0");
    let status = relaunched.core.update_status().await.expect("status");
    assert!(status.last_checked_at.is_some(), "{status:?}");
    assert_eq!(status.last_checked_at, Some(last));
    relaunched
        .core
        .update_check_if_due(last + DAY_MS - 1)
        .await
        .expect("same day");
    assert_eq!(FakeUpdates::count(&relaunched.updates.checks), 0);
    relaunched
        .core
        .update_check_if_due(last + DAY_MS)
        .await
        .expect("next day");
    assert_eq!(FakeUpdates::count(&relaunched.updates.checks), 1);
    assert_eq!(UPDATE_CHECK_INTERVAL.as_millis(), DAY_MS as u128);
}

/// A backend that holds a check open so a second one meets it.
struct Slow {
    release: tokio::sync::Notify,
    inner: FakeUpdates,
}

impl UpdateBackend for Slow {
    fn configured(&self) -> bool {
        true
    }

    fn check(
        &self,
    ) -> note_everywhere_desktop_lib::app::BoxFuture<
        '_,
        Result<Option<note_everywhere_desktop_lib::app::UpdateOffer>, UpdateFailure>,
    > {
        Box::pin(async move {
            self.release.notified().await;
            self.inner.check().await
        })
    }

    fn download(
        &self,
        progress: note_everywhere_desktop_lib::app::ProgressFn,
    ) -> note_everywhere_desktop_lib::app::BoxFuture<'_, Result<(), UpdateFailure>> {
        self.inner.download(progress)
    }

    fn install(&self) -> Result<(), UpdateFailure> {
        self.inner.install()
    }

    fn restart(&self) {
        self.inner.restart();
    }
}

#[tokio::test]
async fn a_second_check_while_one_runs_is_refused() {
    let device = Device::with_services(Services::default()).await;
    // Rebuild the core over the slow backend, on the same data.
    let slow = Arc::new(Slow {
        release: tokio::sync::Notify::new(),
        inner: FakeUpdates::new(true),
    });
    let deps = Deps {
        events: device.events.clone(),
        clipboard: device.clipboard.clone(),
        dialogs: device.dialogs.clone(),
        hotkeys: device.hotkeys.clone(),
        notifier: device.notifier.clone(),
        updater: slow.clone(),
        secrets: note_everywhere_desktop_lib::app::SharedBackend(device.secrets.clone()),
    };
    let core = note_everywhere_desktop_lib::app::AppCore::open(
        support::test_config(&device.dir, device.clock.clone()),
        deps,
    )
    .await
    .expect("open");

    let running = {
        let core = core.clone();
        tokio::spawn(async move { core.update_check().await })
    };
    while core.update_status().await.expect("status").state != UpdateState::Checking {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        core.update_check().await.expect_err("busy").code,
        "update_busy"
    );
    slow.release.notify_one();
    let done = running.await.expect("join").expect("check");
    assert_eq!(done.state, UpdateState::UpToDate);
    core.shutdown().await;
}
