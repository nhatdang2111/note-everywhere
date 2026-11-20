//! Native notifications for due reminders: when the permission is asked for,
//! what is shown, and what the `reminder:due` event says about it.

mod support;

use std::time::Duration;

use note_everywhere_desktop_lib::app::NotifyPermission;
use note_everywhere_desktop_lib::app::reminder_api::ReminderAddArgs;
use note_everywhere_desktop_lib::app::types::{CreateNodeArgs, NodeKind};
use serde_json::json;
use support::{Device, Services};

const MINUTE: i64 = 60_000;
const BASE: i64 = 1_900_000_000_000;

async fn page(device: &Device, title: &str) -> String {
    device
        .core
        .tree_create(CreateNodeArgs {
            kind: NodeKind::Page,
            parent_id: None,
            index: None,
            title: Some(title.into()),
        })
        .await
        .expect("page")
}

async fn remind(device: &Device, page: &str, minutes: i64, note: Option<&str>) {
    device
        .core
        .reminder_add(ReminderAddArgs {
            page_id: page.to_owned(),
            property_id: None,
            at: Some(BASE + minutes * MINUTE),
            utc_offset_minutes: 420,
            note: note.map(str::to_owned),
        })
        .await
        .expect("add");
}

async fn wait_for_requests(device: &Device, expected: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while device.notifier.requests() < expected {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the permission request never arrived"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn undetermined(answer: NotifyPermission) -> Services {
    Services {
        notifications: NotifyPermission::Undetermined,
        notification_answer: answer,
        ..Services::default()
    }
}

#[tokio::test]
async fn permission_is_asked_when_the_first_reminder_is_created_and_not_before() {
    let device = Device::with_services(undetermined(NotifyPermission::Granted)).await;
    device.core.start_reminder_scheduler();
    device.core.workspace_create_local().await.expect("create");
    let page = page(&device, "Gọi điện").await;
    // Opening the app, creating a workspace and a page asked nothing.
    assert_eq!(device.notifier.requests(), 0);

    remind(&device, &page, 5, None).await;
    wait_for_requests(&device, 1).await;
    remind(&device, &page, 15, None).await;
    // Answered once, so the second reminder does not ask again.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(device.notifier.requests(), 1);
}

#[tokio::test]
async fn a_person_who_already_refused_is_not_asked_again() {
    let device = Device::with_services(Services {
        notifications: NotifyPermission::Denied,
        ..Services::default()
    })
    .await;
    device.core.workspace_create_local().await.expect("create");
    let page = page(&device, "Gọi điện").await;
    remind(&device, &page, 5, None).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(device.notifier.requests(), 0);
}

#[tokio::test]
async fn a_due_reminder_shows_a_notification_and_the_event_says_so() {
    let device = Device::local().await;
    let page = page(&device, "Gọi điện").await;
    remind(&device, &page, 5, Some("gọi bác sĩ")).await;
    device.events.clear();

    let fired = device
        .core
        .reminders_check(BASE + 6 * MINUTE)
        .expect("check");
    assert_eq!(fired.len(), 1);
    assert_eq!(
        device.notifier.shown(),
        [("Gọi điện".to_owned(), "gọi bác sĩ".to_owned())]
    );
    let events = device.events.named("reminder:due");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload()["nativeShown"], json!(true));
    // Once per device: looking again shows nothing more.
    device
        .core
        .reminders_check(BASE + 7 * MINUTE)
        .expect("check");
    assert_eq!(device.notifier.shown().len(), 1);
}

#[tokio::test]
async fn a_reminder_without_a_note_or_a_title_still_reads_sensibly() {
    let device = Device::local().await;
    let page = page(&device, "").await;
    remind(&device, &page, 5, None).await;
    device
        .core
        .reminders_check(BASE + 6 * MINUTE)
        .expect("check");
    assert_eq!(
        device.notifier.shown(),
        [("Reminder".to_owned(), "A reminder is due.".to_owned())]
    );
}

#[tokio::test]
async fn many_due_reminders_become_one_notification() {
    let device = Device::local().await;
    let page = page(&device, "Việc").await;
    for minute in 1..=5 {
        remind(&device, &page, minute, None).await;
    }
    device.events.clear();
    let fired = device
        .core
        .reminders_check(BASE + 6 * MINUTE)
        .expect("check");
    assert_eq!(fired.len(), 5);
    assert_eq!(
        device.notifier.shown(),
        [("Reminders".to_owned(), "5 reminders are due.".to_owned())]
    );
    assert_eq!(
        device.events.named("reminder:due")[0].payload()["nativeShown"],
        json!(true)
    );
}

#[tokio::test]
async fn without_permission_the_event_is_all_there_is() {
    for permission in [NotifyPermission::Denied, NotifyPermission::Undetermined] {
        let device = Device::with_services(Services {
            notifications: permission,
            notification_answer: permission,
            ..Services::default()
        })
        .await;
        device.core.workspace_create_local().await.expect("create");
        let page = page(&device, "Gọi điện").await;
        remind(&device, &page, 5, Some("gọi bác sĩ")).await;
        device.events.clear();
        let fired = device
            .core
            .reminders_check(BASE + 6 * MINUTE)
            .expect("check");
        assert_eq!(fired.len(), 1);
        assert!(device.notifier.shown().is_empty());
        let events = device.events.named("reminder:due");
        assert_eq!(events.len(), 1);
        let payload = events[0].payload();
        assert_eq!(payload["nativeShown"], json!(false));
        assert_eq!(
            payload["reminders"][0]["reminder"]["note"],
            json!("gọi bác sĩ")
        );
    }
}
