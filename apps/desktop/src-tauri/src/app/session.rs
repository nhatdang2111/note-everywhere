//! Workspace creation, accounts and sessions.
//!
//! What is stored where: the session token and the account root key live in
//! the OS secure store; everything non-secret (server address, user name,
//! session ids, the wrapped account record) lives in the workspace's settings
//! table. Opening the app unwraps the workspace key from the wrapped record
//! with the stored root key, so no password is needed to resume syncing.
//!
//! Signing out keeps the local workspace and turns it into a local-only one:
//! the outbox, the sync position and the attachment index are dropped and a
//! later sign-in uploads everything again (importing known updates is a no-op
//! on the receiving side).

use ne_core::NodeId;
use ne_core::tree::NodeKind;
use ne_crypto::account::{AccountError, AccountRecord};
use ne_crypto::key_wrap::{WrapContext, unwrap_key};
use ne_crypto::keys::{RootKey, SecretKey, WorkspaceKey};
use ne_crypto::recovery::RecoveryKey;
use ne_platform::secure_store::CredentialStore;
use ne_store::{DocRef, Workspace};
use ne_sync::{
    ClientConfig, SessionState as ClientSession, SessionToken, SyncClient, SyncError, Unlocked,
};
use url::Url;

use super::deps::SharedBackend;
use super::sync_store::{BLOB_INDEX_DOC, VAULT_DOC, encode_cursor};
use super::types::{
    DeviceSession, RecoverAccountArgs, SessionState, SessionStatus, SignInArgs, SignUpArgs,
};
use super::{AppCore, AppError, AppResult, lock};

const PROFILE: &str = "default";
const SESSION_KEYS: [&str; 7] = [
    "session.server_url",
    "session.username",
    "session.id",
    "session.workspace_id",
    "session.expires_at",
    "session.device_label",
    "session.record",
];

fn parse_server_url(text: &str) -> AppResult<Url> {
    let url = Url::parse(text.trim()).map_err(|_| {
        AppError::invalid("Enter the server address, for example https://sync.example.workers.dev.")
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::invalid(
            "The server address must start with http:// or https://.",
        ));
    }
    Ok(url)
}

fn require_text(value: &str, what: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        Err(AppError::invalid(format!("{what} must not be empty.")))
    } else {
        Ok(())
    }
}

/// Every page node, trashed ones included.
fn all_pages(ws: &Workspace) -> ne_store::Result<Vec<NodeId>> {
    let mut pages = Vec::new();
    let mut stack = vec![None];
    while let Some(parent) = stack.pop() {
        for info in ws.tree().children(parent, true)? {
            stack.push(Some(info.id));
            if info.kind == NodeKind::Page {
                pages.push(info.id);
            }
        }
    }
    Ok(pages)
}

/// Every document the server keeps for this workspace except the vault: the
/// tree, every page node, then the documents that travel under a node-shaped
/// id without being a page of the tree (databases, the template library, the
/// reminders, synced blocks).
pub(crate) fn workspace_documents(ws: &Workspace) -> ne_store::Result<Vec<DocRef>> {
    let mut docs = vec![DocRef::Tree];
    docs.extend(all_pages(ws)?.into_iter().map(DocRef::Page));
    docs.extend(ws.synced_documents()?);
    Ok(docs)
}

/// The full state of a `DocRef::Page` document, whatever it holds.
pub(crate) fn page_family_snapshot(ws: &mut Workspace, id: NodeId) -> ne_store::Result<Vec<u8>> {
    if ws
        .tree()
        .get(id)
        .is_ok_and(|info| info.kind == NodeKind::Page)
    {
        ws.page_snapshot(id)
    } else {
        ws.document_snapshot(id)
    }
}

impl AppCore {
    pub(crate) fn credentials(&self) -> AppResult<CredentialStore<SharedBackend>> {
        Ok(CredentialStore::with_backend(
            self.inner.deps.secrets.clone(),
            &self.inner.cfg.secret_service,
            PROFILE,
        )?)
    }

    fn make_client(&self, url: Url, label: &str) -> AppResult<SyncClient> {
        let cfg = &self.inner.cfg;
        let mut config = ClientConfig::new(url, label);
        config.kdf_params = cfg.kdf_params;
        config.kdf_policy = cfg.kdf_policy;
        config.user_agent_extra = Some(format!("desktop/{}", env!("CARGO_PKG_VERSION")));
        Ok(SyncClient::new(config)?)
    }

    pub fn session_status_now(&self) -> SessionStatus {
        let data = lock(&self.inner.session);
        SessionStatus {
            state: data.state,
            username: data.username.clone(),
            server_url: data.server_url.clone(),
        }
    }

    pub async fn session_status(&self) -> AppResult<SessionStatus> {
        Ok(self.session_status_now())
    }

    fn set_session(
        &self,
        state: SessionState,
        username: Option<String>,
        server_url: Option<String>,
    ) {
        {
            let mut data = lock(&self.inner.session);
            data.state = state;
            data.username = username;
            data.server_url = server_url;
        }
        self.emit(super::CoreEvent::SessionChanged(self.session_status_now()));
        self.publish_sync_status();
    }

    fn state(&self) -> SessionState {
        lock(&self.inner.session).state
    }

    // ---- workspace ----

    fn create_workspace(&self) -> AppResult<()> {
        let ws = Workspace::create(
            self.inner.cfg.workspace_dir(),
            self.inner.cfg.store_config(),
        )?;
        *lock(&self.inner.ws) = Some(ws);
        self.persist_settings()?;
        lock(&self.inner.session).state = SessionState::Local;
        Ok(())
    }

    pub async fn workspace_create_local(&self) -> AppResult<()> {
        let _gate = self.inner.session_gate.lock().await;
        if self.state() != SessionState::Empty {
            return Err(AppError::new(
                "workspace_exists",
                "A workspace already exists on this device.",
            ));
        }
        self.create_workspace()?;
        self.set_session(SessionState::Local, None, None);
        Ok(())
    }

    // ---- sign up, sign in ----

    pub async fn sign_up(&self, args: SignUpArgs) -> AppResult<String> {
        let _gate = self.inner.session_gate.lock().await;
        if matches!(
            self.state(),
            SessionState::SignedIn | SessionState::NeedsSignIn
        ) {
            return Err(AppError::new(
                "already_signed_in",
                "This device already has an account.",
            ));
        }
        let url = parse_server_url(&args.server_url)?;
        require_text(&args.username, "The username")?;
        require_text(&args.password, "The password")?;
        let client = self.make_client(url, &args.device_label)?;
        let created = client
            .sign_up(&args.setup_code, &args.username, &args.password)
            .await?;
        let recovery = created.recovery_key.to_display_string().to_string();
        self.finish_sign_in(
            client,
            created.unlocked,
            &args.server_url,
            &args.username,
            &args.device_label,
        )
        .await?;
        Ok(recovery)
    }

    /// A device that already belongs to an account can only sign in to that
    /// same account again.
    fn check_account_matches(&self, url: &Url, username: &str) -> AppResult<()> {
        if self.state() != SessionState::NeedsSignIn {
            return Ok(());
        }
        let known = self.session_status_now();
        let same = known
            .server_url
            .as_deref()
            .is_some_and(|u| parse_server_url(u).is_ok_and(|k| &k == url))
            && known.username.as_deref().map(str::to_lowercase)
                == Some(username.trim().to_lowercase());
        if same {
            Ok(())
        } else {
            Err(AppError::new(
                "account_mismatch",
                "This device belongs to another account. Sign in with that account.",
            ))
        }
    }

    pub async fn sign_in(&self, args: SignInArgs) -> AppResult<()> {
        let _gate = self.inner.session_gate.lock().await;
        if self.state() == SessionState::SignedIn {
            return Err(AppError::new(
                "already_signed_in",
                "This device is already signed in.",
            ));
        }
        let url = parse_server_url(&args.server_url)?;
        require_text(&args.username, "The username")?;
        require_text(&args.password, "The password")?;
        self.check_account_matches(&url, &args.username)?;
        let client = self.make_client(url, &args.device_label)?;
        let unlocked = client.login(&args.username, &args.password).await?;
        self.finish_sign_in(
            client,
            unlocked,
            &args.server_url,
            &args.username,
            &args.device_label,
        )
        .await
    }

    /// Resets a forgotten password with the account recovery key and signs
    /// this device in. The local data stays as it is; every other device is
    /// signed out and needs the new password. The recovery key itself stays
    /// valid (it is not rotated).
    pub async fn recover_account(&self, args: RecoverAccountArgs) -> AppResult<()> {
        let _gate = self.inner.session_gate.lock().await;
        if self.state() == SessionState::SignedIn {
            return Err(AppError::new(
                "already_signed_in",
                "This device is already signed in.",
            ));
        }
        let url = parse_server_url(&args.server_url)?;
        require_text(&args.username, "The username")?;
        require_text(&args.new_password, "The new password")?;
        RecoveryKey::parse(&args.recovery_key).map_err(|_| {
            AppError::new(
                "invalid_recovery_key",
                "That recovery key is not valid. Check it for typing mistakes.",
            )
        })?;
        self.check_account_matches(&url, &args.username)?;
        let client = self.make_client(url, &args.device_label)?;
        let unlocked = client
            .recover_account(&args.username, &args.recovery_key, &args.new_password)
            .await
            .map_err(|err| match err {
                err if err.is_recovery_unavailable() => AppError::new(
                    "recovery_unavailable",
                    "This account was created before password recovery was supported. \
                     Sign in with the password, or create a new account.",
                ),
                // A key that opens nothing, or a proof the server refused.
                SyncError::Account(AccountError::WrongSecret) | SyncError::InvalidCredentials => {
                    AppError::new(
                        "wrong_recovery_key",
                        "That recovery key does not match this account.",
                    )
                }
                other => other.into(),
            })?;
        self.finish_sign_in(
            client,
            unlocked,
            &args.server_url,
            &args.username,
            &args.device_label,
        )
        .await
    }

    async fn finish_sign_in(
        &self,
        client: SyncClient,
        unlocked: Unlocked,
        server_url: &str,
        username: &str,
        label: &str,
    ) -> AppResult<()> {
        let result = self
            .adopt_session(&client, &unlocked, server_url, username, label)
            .await;
        if result.is_err() {
            // Do not leave an unused session open on the server.
            let _ = client.logout().await;
        }
        let key = result?;
        self.start_runtime(client, key)?;
        let status = self.session_status_now();
        self.set_session(SessionState::SignedIn, status.username, status.server_url);
        self.schedule_maintenance();
        Ok(())
    }

    async fn adopt_session(
        &self,
        client: &SyncClient,
        unlocked: &Unlocked,
        server_url: &str,
        username: &str,
        label: &str,
    ) -> AppResult<WorkspaceKey> {
        let record = client.account_record().await?;
        let server_vault = client.vault_record().await?;
        let was = self.state();
        // A vault is tied to its own key, so two different ones cannot merge.
        if let (Some(local), Some(remote)) = (self.local_vault_record()?, server_vault.as_ref())
            && &local != remote
        {
            return Err(AppError::new(
                "vault_conflict",
                "This device already has a vault that differs from the one in your account.",
            ));
        }
        if was == SessionState::Empty {
            self.create_workspace()?;
        }
        let creds = self.credentials()?;
        creds.store_session_token(unlocked.session.token.expose())?;
        creds.store_root_key(&unlocked.secrets.root_key)?;
        let session = &unlocked.session;
        let record_json = serde_json::to_string(&record)
            .map_err(|_| AppError::internal("The account record could not be stored."))?;
        let username = username.trim().to_lowercase();
        let entries = [
            ("session.server_url", server_url.trim().to_owned()),
            ("session.username", username.clone()),
            ("session.id", session.session_id.clone()),
            ("session.workspace_id", session.workspace_id.clone()),
            ("session.expires_at", session.expires_at.to_string()),
            ("session.device_label", label.to_owned()),
            ("session.record", record_json),
        ];
        self.store(|ws| {
            for (key, value) in &entries {
                ws.set_setting(key, value)?;
            }
            Ok(())
        })?;
        if was == SessionState::Local {
            self.queue_full_upload()?;
        }
        match (server_vault, self.local_vault_record()?) {
            (Some(remote), None) => self.adopt_vault_record(&remote)?,
            (None, Some(_)) => self.mark_vault_record_pending()?,
            _ => {}
        }
        {
            let mut data = lock(&self.inner.session);
            data.username = Some(username);
            data.server_url = Some(server_url.trim().to_owned());
        }
        Ok(WorkspaceKey::from_bytes(
            unlocked.secrets.workspace_key.as_bytes(),
        ))
    }

    /// Replaces the outbox with everything this device holds, for a workspace
    /// that has so far lived only here. The cursor goes back to zero and every
    /// document is queued explicitly: nothing relies on the store to do it.
    fn queue_full_upload(&self) -> AppResult<()> {
        self.with_ws(|ws| {
            loop {
                let queued = ws.outbox_peek(500)?;
                if queued.is_empty() {
                    break;
                }
                let ids: Vec<String> = queued.into_iter().map(|i| i.client_id).collect();
                ws.outbox_acknowledge(&ids)?;
            }
            ws.set_sync_cursor(&encode_cursor(0))?;
            for doc in workspace_documents(ws)? {
                let snapshot = match &doc {
                    DocRef::Tree => ws.tree().export_snapshot().map_err(ne_store::Error::from)?,
                    DocRef::Page(id) => page_family_snapshot(ws, *id)?,
                    DocRef::Vault(_) => continue,
                };
                ws.outbox_enqueue(&doc, &snapshot)?;
            }
            if ws.vault_doc(VAULT_DOC)?.is_some() {
                let snapshot = super::vault_persist::load_doc(ws)?
                    .export_snapshot()
                    .map_err(|_| AppError::internal("The vault could not be read."))?;
                ws.outbox_enqueue(&DocRef::Vault(VAULT_DOC.to_owned()), &snapshot)?;
            }
            Ok(())
        })
    }

    // ---- sign out, needs sign-in ----

    pub async fn sign_out(&self) -> AppResult<()> {
        let _gate = self.inner.session_gate.lock().await;
        if !matches!(
            self.state(),
            SessionState::SignedIn | SessionState::NeedsSignIn
        ) {
            return Err(AppError::new(
                "not_signed_in",
                "This device has no account to sign out of.",
            ));
        }
        if let Some(runtime) = self.runtime() {
            // Best effort: the local session is dropped even if the server is out of reach.
            let _ = runtime.client.logout().await;
        }
        self.stop_runtime().await;
        let cleared = self
            .credentials()
            .and_then(|c| c.clear().map_err(AppError::from));
        self.store(|ws| {
            for key in SESSION_KEYS {
                ws.remove_setting(key)?;
            }
            ws.remove_setting("vault.record_pending")?;
            loop {
                let queued = ws.outbox_peek(500)?;
                if queued.is_empty() {
                    break;
                }
                let ids: Vec<String> = queued.into_iter().map(|i| i.client_id).collect();
                ws.outbox_acknowledge(&ids)?;
            }
            ws.set_sync_cursor(&encode_cursor(0))?;
            ws.delete_vault_doc(BLOB_INDEX_DOC)
        })?;
        *lock(&self.inner.last_synced_at) = None;
        self.set_session(SessionState::Local, None, None);
        cleared
    }

    /// The stored session was refused: keep the data, drop the credentials,
    /// and ask for the password again.
    pub(crate) async fn enter_needs_sign_in(&self) {
        let _gate = self.inner.session_gate.lock().await;
        if self.state() != SessionState::SignedIn {
            return;
        }
        self.stop_runtime().await;
        if let Ok(creds) = self.credentials() {
            let _ = creds.clear();
        }
        let status = self.session_status_now();
        self.set_session(
            SessionState::NeedsSignIn,
            status.username,
            status.server_url,
        );
    }

    /// Any call that comes back `unauthorized` means the session is gone.
    async fn checked<T>(&self, result: Result<T, ne_sync::SyncError>) -> AppResult<T> {
        if matches!(result, Err(ne_sync::SyncError::Unauthorized)) {
            let core = self.clone();
            self.inner
                .rt
                .spawn(async move { core.enter_needs_sign_in().await });
        }
        Ok(result?)
    }

    fn client(&self) -> AppResult<SyncClient> {
        self.runtime()
            .map(|r| r.client.clone())
            .ok_or_else(|| AppError::new("not_signed_in", "This device is not signed in."))
    }

    pub async fn change_password(&self, current: &str, new: &str) -> AppResult<()> {
        require_text(current, "The current password")?;
        require_text(new, "The new password")?;
        let client = self.client()?;
        self.checked(client.change_password(current, new).await)
            .await?;
        // The record carries the new salt and wrapped keys; keep the local copy current.
        let record = self.checked(client.account_record().await).await?;
        let json = serde_json::to_string(&record)
            .map_err(|_| AppError::internal("The account record could not be stored."))?;
        self.store(|ws| ws.set_setting("session.record", &json))
    }

    pub async fn sessions_list(&self) -> AppResult<Vec<DeviceSession>> {
        let client = self.client()?;
        let sessions = self.checked(client.list_sessions().await).await?;
        Ok(sessions
            .into_iter()
            .map(|s| DeviceSession {
                id: s.id,
                device_label: s.label,
                created_at: s.created_at,
                last_seen_at: s.last_seen_at,
                current: s.current,
            })
            .collect())
    }

    pub async fn sessions_revoke(&self, id: &str) -> AppResult<()> {
        let client = self.client()?;
        self.checked(client.revoke_session(id).await).await
    }

    // ---- reopening ----

    /// Restores settings, the vault and the session from the opened workspace.
    pub(crate) async fn restore(&self) -> AppResult<()> {
        self.load_settings()?;
        self.load_vault()?;
        let (url, user) = self.store(|ws| {
            Ok((
                ws.setting("session.server_url")?,
                ws.setting("session.username")?,
            ))
        })?;
        let (Some(url), Some(user)) = (url, user) else {
            lock(&self.inner.session).state = SessionState::Local;
            return Ok(());
        };
        {
            let mut data = lock(&self.inner.session);
            data.state = SessionState::NeedsSignIn;
            data.username = Some(user);
            data.server_url = Some(url);
        }
        if self.try_resume().is_ok() {
            lock(&self.inner.session).state = SessionState::SignedIn;
            self.publish_sync_status();
        }
        Ok(())
    }

    fn try_resume(&self) -> AppResult<()> {
        let creds = self.credentials()?;
        let token = creds
            .load_session_token()?
            .ok_or_else(|| AppError::internal("no stored session"))?;
        let root: RootKey = creds
            .load_root_key()?
            .ok_or_else(|| AppError::internal("no stored key"))?;
        let get = |key: &str| -> AppResult<String> {
            self.store(|ws| ws.setting(key))?
                .ok_or_else(|| AppError::internal("incomplete stored session"))
        };
        let record: AccountRecord = serde_json::from_str(&get("session.record")?)
            .map_err(|_| AppError::internal("damaged stored session"))?;
        let key: WorkspaceKey = unwrap_key(&root, WrapContext::WorkspaceKey, &record.workspace_key)
            .map_err(|_| AppError::internal("the stored key does not match the account"))?;
        let url = parse_server_url(&get("session.server_url")?)?;
        let client = self.make_client(url, &get("session.device_label")?)?;
        client.resume_session(ClientSession {
            token: SessionToken::new(token.to_string()),
            session_id: get("session.id")?,
            workspace_id: get("session.workspace_id")?,
            expires_at: get("session.expires_at")?.parse().unwrap_or(0),
        });
        self.start_runtime(client, key)
    }
}
