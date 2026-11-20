//! The reminders document and the due-time rules.

use ne_core::database::DateValue;
use ne_core::reminders::{DEFAULT_REMINDER_MINUTES, RemindersDoc};

fn doc(peer: u64) -> RemindersDoc {
    let doc = RemindersDoc::new().expect("doc");
    doc.set_peer_id(peer).expect("peer");
    doc
}

#[test]
fn a_reminder_is_stored_listed_and_changed() {
    let doc = doc(1);
    let id = doc
        .add("1@2", None, 5_000, 420, "  gọi lại  ", 100)
        .expect("add");
    let r = doc.get(&id).expect("get");
    assert_eq!(r.note, "gọi lại");
    assert_eq!((r.at, r.utc_offset_minutes, r.done), (5_000, 420, false));
    doc.snooze(&id, 9_000).expect("snooze");
    assert_eq!(doc.get(&id).expect("get").snoozed_until, Some(9_000));
    doc.set_done(&id, true).expect("done");
    let r = doc.get(&id).expect("get");
    assert!(
        r.done && r.snoozed_until.is_none(),
        "done clears the snooze"
    );
    doc.reschedule(&id, 20_000).expect("reschedule");
    let r = doc.get(&id).expect("get");
    assert!(!r.done && r.at == 20_000, "a new time reopens the reminder");
    doc.set_note(&id, "new").expect("note");
    assert_eq!(doc.list().len(), 1);
    doc.delete(&id).expect("delete");
    assert!(doc.list().is_empty());
    assert!(doc.add("", None, 0, 0, "", 0).is_err());
    assert!(doc.add("1@2", None, 0, 24 * 60, "", 0).is_err());
    assert!(doc.add("1@2", None, 0, 0, &"x".repeat(501), 0).is_err());
}

#[test]
fn due_time_follows_the_date_property_and_a_snooze_wins_when_later() {
    let doc = doc(1);
    let id = doc
        .add("1@2", Some("prop"), 1_000, 420, "", 0)
        .expect("add");
    let r = doc.get(&id).expect("get");
    // No date in the cell: the stored moment.
    assert_eq!(r.due_at(None), 1_000);
    // A date without a time is 09:00 in the reminder's offset (UTC+7 = 02:00 UTC).
    let date = DateValue::new("2026-11-21");
    let expected =
        ne_core::database::to_epoch_ms("2026-11-21", 420, DEFAULT_REMINDER_MINUTES).expect("date");
    assert_eq!(r.due_at(Some(&date)), expected);
    let timed = DateValue::new("2026-11-21T18:30");
    assert_eq!(
        r.due_at(Some(&timed)),
        ne_core::database::to_epoch_ms("2026-11-21T18:30", 420, 0).expect("date")
    );
    doc.snooze(&id, expected + 60_000).expect("snooze");
    let r = doc.get(&id).expect("get");
    assert_eq!(r.due_at(Some(&date)), expected + 60_000);
    // A snooze earlier than the due moment changes nothing.
    doc.snooze(&id, 5).expect("snooze early");
    assert_eq!(doc.get(&id).expect("get").due_at(Some(&date)), expected);
}

#[test]
fn done_on_one_device_and_a_note_on_another_both_survive() {
    let a = doc(1);
    let id = a.add("1@2", None, 1_000, 0, "", 0).expect("add");
    let b = RemindersDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("b");
    b.set_peer_id(2).expect("peer");
    let (_, done) = a.edit(|d| d.set_done(&id, true)).expect("done");
    let (_, note) = b.edit(|d| d.set_note(&id, "edited")).expect("note");
    a.import(&note).expect("into a");
    b.import(&done).expect("into b");
    for d in [&a, &b] {
        let r = d.get(&id).expect("get");
        assert!(r.done);
        assert_eq!(r.note, "edited");
    }
}
