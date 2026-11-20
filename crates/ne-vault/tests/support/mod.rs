//! Shared helpers for the integration tests. All data is synthetic.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use ne_crypto::account::{NewVault, VaultRecord, create_vault};
use ne_crypto::password::{KdfParams, KdfPolicy};
use ne_crypto::recovery::RecoveryKey;
use ne_vault::{Clock, ItemBody, ItemDraft, LockPolicy, Vault, VaultDoc};

pub const PASSWORD: &[u8] = b"synthetic vault password 1";

pub const TINY_KDF: KdfParams = KdfParams {
    memory_kib: 8,
    iterations: 1,
    parallelism: 1,
};

/// A clock the test moves by hand.
#[derive(Debug, Default)]
pub struct TestClock(AtomicI64);

impl TestClock {
    pub fn at(ms: i64) -> Arc<Self> {
        Arc::new(Self(AtomicI64::new(ms)))
    }

    pub fn advance_secs(&self, secs: i64) {
        self.0.fetch_add(secs * 1000, Ordering::SeqCst);
    }

    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A freshly created vault account: the record the server would hold plus the
/// recovery key.
pub struct Account {
    pub record: VaultRecord,
    pub recovery: RecoveryKey,
}

pub fn account() -> Account {
    let NewVault {
        record,
        recovery_key,
        ..
    } = create_vault(PASSWORD, TINY_KDF, &KdfPolicy::LENIENT).expect("create vault");
    Account {
        record,
        recovery: recovery_key,
    }
}

pub fn vault_with(
    account: &Account,
    doc: VaultDoc,
    policy: LockPolicy,
    clock: Arc<TestClock>,
) -> Vault {
    Vault::new(
        doc,
        account.record.clone(),
        KdfPolicy::LENIENT,
        policy,
        clock,
    )
}

/// A new unlocked replica with an empty document.
pub fn unlocked_replica(account: &Account, clock: Arc<TestClock>) -> Vault {
    let mut vault = vault_with(account, VaultDoc::new(), LockPolicy::default(), clock);
    vault.unlock_with_password(PASSWORD).expect("unlock");
    vault
}

pub fn login(title: &str, user: &str, password: &str) -> ItemDraft {
    ItemDraft {
        title: title.into(),
        tags: vec![],
        body: ItemBody::Login {
            username: user.into(),
            password: password.into(),
            urls: vec![],
            notes: String::new(),
        },
    }
}

pub fn note(title: &str, text: &str) -> ItemDraft {
    ItemDraft {
        title: title.into(),
        tags: vec![],
        body: ItemBody::SecureNote { text: text.into() },
    }
}

pub fn api_key(title: &str, value: &str) -> ItemDraft {
    ItemDraft {
        title: title.into(),
        tags: vec!["ci".into()],
        body: ItemBody::ApiKey {
            value: value.into(),
            notes: String::new(),
        },
    }
}

/// Sends everything `from` has to `to`.
pub fn sync(from: &Vault, to: &mut Vault) {
    let updates = from
        .export_updates_since(&to.version())
        .expect("export updates");
    to.import(&updates).expect("import");
}

/// Two-way exchange.
pub fn sync_both(a: &mut Vault, b: &mut Vault) {
    let to_b = a.export_updates_since(&b.version()).expect("export a");
    let to_a = b.export_updates_since(&a.version()).expect("export b");
    b.import(&to_b).expect("import into b");
    a.import(&to_a).expect("import into a");
}

/// Titles of all items, in list order.
pub fn titles(vault: &Vault) -> Vec<String> {
    vault
        .list()
        .expect("list")
        .into_iter()
        .map(|v| v.title.clone())
        .collect()
}
