//! Convergence of replicas and what the serialised document exposes.

mod support;

use ne_vault::{Field, ItemId, LockPolicy, Vault, VaultDoc};
use support::*;

const T0: i64 = 1_700_000_000_000;

/// Three replicas of one vault, all unlocked.
fn three(account: &Account) -> (Vault, Vault, Vault) {
    let clock = TestClock::at(T0);
    (
        unlocked_replica(account, clock.clone()),
        unlocked_replica(account, clock.clone()),
        unlocked_replica(account, clock),
    )
}

/// Everything in a replica that matters for equality: id, title, password.
fn state(vault: &Vault) -> Vec<(ItemId, String, String)> {
    let mut out: Vec<_> = vault
        .list()
        .expect("list")
        .into_iter()
        .map(|v| {
            let secret = vault
                .reveal(&v.id, Field::Password)
                .or_else(|_| vault.reveal(&v.id, Field::Text))
                .or_else(|_| vault.reveal(&v.id, Field::Value))
                .expect("reveal");
            (v.id, v.title.clone(), secret.to_string())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn concurrent_edits_to_different_items_both_survive() {
    let account = account();
    let (mut a, mut b, _) = three(&account);
    let base = a.add(login("shared", "u", "base")).expect("add");
    sync(&a, &mut b);

    a.update(&base, login("shared", "u", "from-a"))
        .expect("a edits");
    let only_b = b.add(note("b only", "from-b")).expect("b adds");
    let only_a = a.add(api_key("a only", "tok")).expect("a adds");
    sync_both(&mut a, &mut b);

    assert_eq!(state(&a), state(&b));
    assert_eq!(a.list().expect("list").len(), 3);
    assert!(a.get(&only_a).is_ok() && a.get(&only_b).is_ok());
    assert_eq!(
        &*b.reveal(&base, Field::Password).expect("reveal"),
        "from-a"
    );
}

#[test]
fn concurrent_edits_to_the_same_item_converge_to_one_whole_version() {
    let account = account();
    let (mut a, mut b, _) = three(&account);
    let id = a.add(login("original", "u", "base")).expect("add");
    sync(&a, &mut b);

    a.update(&id, login("title from a", "user-a", "password-a"))
        .expect("a");
    b.update(&id, login("title from b", "user-b", "password-b"))
        .expect("b");
    sync_both(&mut a, &mut b);

    assert_eq!(state(&a), state(&b));
    let view = a.get(&id).expect("get");
    let password = a.reveal(&id, Field::Password).expect("reveal");
    // Whichever side won, title, username and password all come from it.
    let pair = (
        view.title.as_str(),
        view.username.clone().unwrap_or_default(),
        password.to_string(),
    );
    assert!(
        pair == ("title from a", "user-a".into(), "password-a".into())
            || pair == ("title from b", "user-b".into(), "password-b".into()),
        "mixed version: {pair:?}"
    );
    assert!(a.unreadable_items().is_empty() && b.unreadable_items().is_empty());
}

#[test]
fn delete_wins_over_concurrent_edit_and_stale_replicas_do_not_resurrect() {
    let account = account();
    let (mut a, mut b, mut c) = three(&account);
    let id = a.add(login("doomed", "u", "pw")).expect("add");
    sync(&a, &mut b);
    sync(&a, &mut c);

    a.delete(&id).expect("delete on a");
    b.update(&id, login("edited", "u", "newer"))
        .expect("edit on b");
    sync_both(&mut a, &mut b);
    assert!(a.list().expect("list").is_empty());
    assert!(
        b.list().expect("list").is_empty(),
        "the edit lost to the deletion"
    );

    // c never saw the deletion and still shows the item; syncing both ways
    // removes it instead of bringing it back.
    assert_eq!(c.list().expect("list").len(), 1);
    sync_both(&mut c, &mut a);
    sync_both(&mut c, &mut b);
    assert!(c.list().expect("list").is_empty());
    sync_both(&mut a, &mut b);
    assert!(a.list().expect("list").is_empty() && b.list().expect("list").is_empty());
}

#[test]
fn three_replicas_converge_in_every_exchange_order() {
    let account = account();
    // Operations performed offline on each replica, then all orders of merging.
    let orders: [[(usize, usize); 6]; 3] = [
        [(0, 1), (1, 2), (2, 0), (0, 1), (1, 2), (2, 0)],
        [(2, 1), (1, 0), (0, 2), (2, 1), (1, 0), (0, 2)],
        [(1, 0), (2, 0), (0, 1), (0, 2), (1, 2), (2, 1)],
    ];
    for order in orders {
        let (a, b, c) = three(&account);
        let mut replicas = [a, b, c];
        let shared = replicas[0].add(login("shared", "u", "v0")).expect("add");
        let snapshot_updates = replicas[0]
            .export_updates_since(&replicas[1].version())
            .expect("export");
        replicas[1].import(&snapshot_updates).expect("import");
        replicas[2].import(&snapshot_updates).expect("import");

        replicas[0]
            .update(&shared, login("shared", "u", "v-a"))
            .expect("a");
        replicas[1]
            .update(&shared, login("shared", "u", "v-b"))
            .expect("b");
        replicas[2].delete(&shared).expect("c deletes");
        replicas[0].add(note("n0", "t0")).expect("a adds");
        replicas[1].add(note("n1", "t1")).expect("b adds");
        replicas[2].add(note("n2", "t2")).expect("c adds");

        for (from, to) in order {
            let updates = replicas[from]
                .export_updates_since(&replicas[to].version())
                .expect("export");
            replicas[to].import(&updates).expect("import");
        }
        // A last full exchange so every order ends fully merged.
        for from in 0..3 {
            for to in 0..3 {
                if from != to {
                    let updates = replicas[from]
                        .export_updates_since(&replicas[to].version())
                        .expect("export");
                    replicas[to].import(&updates).expect("import");
                }
            }
        }
        let reference = state(&replicas[0]);
        assert_eq!(
            reference.len(),
            3,
            "the shared item is deleted, three notes remain"
        );
        assert!(
            replicas.iter().all(|r| state(r) == reference),
            "order {order:?}"
        );
    }
}

#[test]
fn snapshot_loads_into_a_locked_vault_that_unlocks_with_the_same_password() {
    let account = account();
    let mut a = unlocked_replica(&account, TestClock::at(T0));
    let id = a.add(login("Mail", "u", "pw")).expect("add");
    let doc = VaultDoc::from_snapshot(&a.export_snapshot().expect("snapshot")).expect("load");
    let mut fresh = vault_with(&account, doc, LockPolicy::default(), TestClock::at(T0));
    assert_eq!(fresh.cached_item_count(), 0);
    fresh.unlock_with_password(PASSWORD).expect("unlock");
    assert_eq!(&*fresh.reveal(&id, Field::Password).expect("reveal"), "pw");
}

#[test]
fn import_works_while_locked_and_unlocked_cache_follows_it() {
    let account = account();
    let (mut a, mut b, _) = three(&account);
    a.add(login("from a", "u", "pw")).expect("add");

    b.lock();
    sync(&a, &mut b);
    assert!(b.list().is_err(), "still locked after import");
    b.unlock_with_password(PASSWORD).expect("unlock");
    assert_eq!(titles(&b), ["from a"]);

    a.add(login("second", "u", "pw")).expect("add");
    sync(&a, &mut b);
    assert_eq!(
        titles(&b),
        ["from a", "second"],
        "cache rebuilt on import while unlocked"
    );
}

#[test]
fn bad_sync_bytes_are_rejected_without_changing_state() {
    let mut a = unlocked_replica(&account(), TestClock::at(T0));
    a.add(login("Mail", "u", "pw")).expect("add");
    assert!(a.import(b"definitely not a loro update").is_err());
    assert!(a.export_updates_since(b"garbage").is_err());
    assert_eq!(titles(&a), ["Mail"]);
}

#[test]
fn serialised_document_contains_no_plaintext() {
    let account = account();
    let mut a = unlocked_replica(&account, TestClock::at(T0));
    let secrets = [
        "Zebra-Bank-Title",
        "sam.synthetic@example.test",
        "hunter2-synthetic-pw",
        "https://zebra-bank.example.test/login",
        "secret-tag-label",
        "synthetic note body about unicorns",
        "tok_live_synthetic_000111",
    ];
    let mut draft = login(secrets[0], secrets[1], secrets[2]);
    draft.tags = vec![secrets[4].into()];
    if let ne_vault::ItemBody::Login { urls, .. } = &mut draft.body {
        urls.push(secrets[3].into());
    }
    a.add(draft).expect("add login");
    a.add(note("Another note title", secrets[5]))
        .expect("add note");
    a.add(api_key("Key title", secrets[6])).expect("add key");
    let doomed = a
        .add(login("Deleted title", "deleted-user", "deleted-pw"))
        .expect("add");
    a.delete(&doomed).expect("delete");

    let mut blobs = vec![a.export_snapshot().expect("snapshot")];
    blobs.push(
        a.export_updates_since(&VaultDoc::new().version())
            .expect("updates"),
    );
    let extra = [
        "Another note title",
        "Key title",
        "Deleted title",
        "deleted-user",
        "deleted-pw",
    ];
    for blob in &blobs {
        for needle in secrets.iter().chain(extra.iter()) {
            assert!(
                !blob.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "plaintext {needle:?} found in serialised vault"
            );
        }
    }
    // What is visible: only ids and tombstones.
    let doc = VaultDoc::from_snapshot(&blobs[0]).expect("load");
    assert_eq!(doc.item_ids().len(), 3);
    assert_eq!(doc.deleted_ids(), [doomed]);
}
