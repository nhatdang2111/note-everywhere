//! Account and session operations against the sync server.
//!
//! Callers pass passwords and get unlocked keys back; the client composes
//! `ne-crypto::account` so the auth key, the wrapped records and the unwrapped
//! keys never need to be handled by the caller. Key derivation is CPU-heavy
//! and runs on the blocking pool.

use std::sync::Arc;

use ne_crypto::account::{
    self, AccountRecord, AccountSecrets, RecoveryRecord, SALT_LEN, VaultRecord,
};
use ne_crypto::key_wrap::WrappedKey;
use ne_crypto::keys::SecretKey;
use ne_crypto::password::{KdfParams, KdfPolicy};
use ne_crypto::recovery::{RecoveryKey, RecoveryKind};
use ne_sync_protocol::ids::{is_valid_label, normalize_username};
use ne_sync_protocol::wire::{
    ChangePasswordRequest, LoginRequest, RecoverRequest, RecoveryLookupRequest,
    RecoveryLookupResponse, RegisterRequest, SaltRequest, SaltResponse, SessionInfo, SessionList,
    SessionResponse, WorkspaceKeyBody, b64_decode_array, b64_encode,
};
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{Result, SyncError};
use crate::transport::{self, SessionState, SessionToken, Transport};

#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Base address of the deployed Worker, for example `https://sync.example.workers.dev`.
    pub server: Url,
    /// Shown in the session list so a device can be recognised and revoked.
    pub device_label: String,
    /// Argon2 cost used when creating an account or changing the password.
    pub kdf_params: KdfParams,
    /// Bounds enforced on parameters that arrive from the server.
    pub kdf_policy: KdfPolicy,
    /// Appended to the User-Agent, for example the app version.
    pub user_agent_extra: Option<String>,
}

impl ClientConfig {
    pub fn new(server: Url, device_label: impl Into<String>) -> Self {
        Self {
            server,
            device_label: device_label.into(),
            kdf_params: KdfParams::DEFAULT,
            kdf_policy: KdfPolicy::PRODUCTION,
            user_agent_extra: None,
        }
    }
}

/// A signed-in account: its unlocked keys and the session that goes with them.
#[derive(Debug)]
pub struct Unlocked {
    pub secrets: AccountSecrets,
    pub session: SessionState,
}

/// Result of [`SyncClient::sign_up`].
#[derive(Debug)]
pub struct SignUp {
    pub unlocked: Unlocked,
    /// Show once and require the user to confirm it is saved; the server
    /// cannot reset a password.
    pub recovery_key: RecoveryKey,
}

#[derive(Clone)]
pub struct SyncClient {
    pub(crate) cfg: Arc<ClientConfig>,
    pub(crate) transport: Arc<Transport>,
}

/// Converts between `ne-crypto` records and their JSON wire shape, which are
/// the same JSON by construction.
fn convert<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U> {
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| SyncError::Input("background task failed".into()))
}

impl SyncClient {
    pub fn new(cfg: ClientConfig) -> Result<Self> {
        if !is_valid_label(&cfg.device_label) {
            return Err(SyncError::Input(
                "device label must be 1 to 64 printable characters".into(),
            ));
        }
        let agent = transport::user_agent(cfg.user_agent_extra.as_deref());
        let transport = Transport::new(cfg.server.clone(), agent)?;
        Ok(Self {
            cfg: Arc::new(cfg),
            transport: Arc::new(transport),
        })
    }

    /// The current session, if signed in.
    pub fn session(&self) -> Option<SessionState> {
        self.transport.session()
    }

    /// Resumes a session saved earlier (for example from the OS keychain).
    pub fn resume_session(&self, session: SessionState) {
        self.transport.set_session(Some(session));
    }

    pub async fn health(&self) -> Result<()> {
        transport::unit(self.transport.request(Method::GET, "/v1/health")?).await
    }

    fn adopt(&self, response: &SessionResponse) -> Result<SessionState> {
        let session = SessionState {
            token: SessionToken::new(response.token.clone()),
            session_id: response.session_id.clone(),
            workspace_id: response.workspace_id.clone(),
            expires_at: response.expires_at,
        };
        self.transport.set_session(Some(session.clone()));
        Ok(session)
    }

    /// Creates the account on a fresh server with its one-time setup code and
    /// signs in.
    pub async fn sign_up(
        &self,
        setup_code: &str,
        username: &str,
        password: &str,
    ) -> Result<SignUp> {
        let username = normalize_username(username).ok_or_else(|| {
            SyncError::Input("username must be 3 to 64 of a-z 0-9 . _ @ + -".into())
        })?;
        let (params, policy, secret) = (
            self.cfg.kdf_params,
            self.cfg.kdf_policy,
            password.to_owned(),
        );
        let created =
            blocking(move || account::create_account(secret.as_bytes(), params, &policy)).await??;
        let body = RegisterRequest {
            setup_code: setup_code.to_owned(),
            username,
            device_label: self.cfg.device_label.clone(),
            auth_key: b64_encode(created.auth_key.as_bytes()),
            recovery_auth_key: b64_encode(
                created
                    .recovery_key
                    .auth_key(RecoveryKind::Account)
                    .as_bytes(),
            ),
            record: convert(&created.record)?,
        };
        let request = self
            .transport
            .request(Method::POST, "/v1/account/register")?
            .json(&body);
        let response: SessionResponse = transport::json(request).await?;
        let session = self.adopt(&response)?;
        Ok(SignUp {
            unlocked: Unlocked {
                secrets: created.secrets,
                session,
            },
            recovery_key: created.recovery_key,
        })
    }

    /// Signs in on this device: fetches the salt, proves the password with the
    /// auth key, and unwraps the keys locally.
    pub async fn login(&self, username: &str, password: &str) -> Result<Unlocked> {
        let username = normalize_username(username).ok_or_else(|| {
            SyncError::Input("username must be 3 to 64 of a-z 0-9 . _ @ + -".into())
        })?;
        let salt_request = self
            .transport
            .request(Method::POST, "/v1/account/salt")?
            .json(&SaltRequest {
                username: username.clone(),
            });
        let salt: SaltResponse = transport::json(salt_request).await?;
        let salt_bytes: [u8; SALT_LEN] =
            b64_decode_array(&salt.kdf_salt).ok_or(SyncError::BadResponse)?;
        let params: KdfParams = convert(&salt.kdf_params)?;
        let (policy, secret) = (self.cfg.kdf_policy, password.to_owned());
        let attempt =
            blocking(move || account::begin_login(secret.as_bytes(), &salt_bytes, params, &policy))
                .await??;

        let body = LoginRequest {
            username,
            auth_key: b64_encode(attempt.auth_key().as_bytes()),
            device_label: self.cfg.device_label.clone(),
        };
        let request = self
            .transport
            .request(Method::POST, "/v1/session/login")?
            .json(&body);
        let response: SessionResponse = transport::json(request).await?;
        let record: AccountRecord = convert(&response.record)?;
        let secrets = attempt
            .unlock(&record)
            .map_err(|_| SyncError::InvalidCredentials)?;
        let session = self.adopt(&response)?;
        Ok(Unlocked { secrets, session })
    }

    /// Resets a forgotten password with the account recovery key and signs in.
    ///
    /// The server hands out the recovery-wrapped keys for the name (made-up
    /// ones for an unknown name); the key is checked locally first, so a wrong
    /// key or name fails as [`account::AccountError::WrongSecret`] without
    /// anything secret leaving the device. Then the proof (an auth secret
    /// derived separately from the wrapping key) and the new password material
    /// go up in one request. The server ends every other session. The recovery
    /// key itself stays valid: it is not rotated. An account created before
    /// recovery verification existed answers with `Server { status: 409, code:
    /// "recovery_unavailable" }`.
    pub async fn recover_account(
        &self,
        username: &str,
        recovery_key: &str,
        new_password: &str,
    ) -> Result<Unlocked> {
        let username = normalize_username(username).ok_or_else(|| {
            SyncError::Input("username must be 3 to 64 of a-z 0-9 . _ @ + -".into())
        })?;
        let key =
            RecoveryKey::parse(recovery_key).map_err(|err| SyncError::Input(err.to_string()))?;
        let lookup = self
            .transport
            .request(Method::POST, "/v1/account/recovery")?
            .json(&RecoveryLookupRequest {
                username: username.clone(),
            });
        let lookup: RecoveryLookupResponse = transport::json(lookup).await?;
        let stored: RecoveryRecord = convert(&lookup)?;
        let (params, policy, secret) = (
            self.cfg.kdf_params,
            self.cfg.kdf_policy,
            new_password.to_owned(),
        );
        let recovered = blocking(move || {
            account::recover_account(&key, &stored, secret.as_bytes(), params, &policy)
        })
        .await??;
        let body = RecoverRequest {
            username,
            recovery_auth_key: b64_encode(recovered.recovery_auth_key.as_bytes()),
            new_auth_key: b64_encode(recovered.auth_key.as_bytes()),
            device_label: self.cfg.device_label.clone(),
            record: convert(&recovered.record)?,
        };
        let request = self
            .transport
            .request(Method::POST, "/v1/account/recover")?
            .json(&body);
        let response: SessionResponse = transport::json(request).await?;
        let session = self.adopt(&response)?;
        Ok(Unlocked {
            secrets: recovered.secrets,
            session,
        })
    }

    /// Ends this session on the server and forgets it locally. The local
    /// session is dropped even if the server cannot be reached.
    pub async fn logout(&self) -> Result<()> {
        let result =
            transport::unit(self.transport.authed(Method::POST, "/v1/session/logout")?).await;
        self.transport.set_session(None);
        match result {
            Err(SyncError::Unauthorized) => Ok(()),
            other => other,
        }
    }

    /// Changes the password: re-wraps the root key under the new password and
    /// replaces the verifier. The server then ends every other session.
    pub async fn change_password(&self, current_password: &str, new_password: &str) -> Result<()> {
        let record = self.account_record().await?;
        let (params, policy) = (self.cfg.kdf_params, self.cfg.kdf_policy);
        let (current, new) = (current_password.to_owned(), new_password.to_owned());
        let old_record = record.clone();
        let (change, current_auth_key) = blocking(move || {
            let attempt = account::begin_login(
                current.as_bytes(),
                &old_record.kdf_salt,
                old_record.kdf_params,
                &policy,
            )?;
            let current_auth_key = attempt.auth_key();
            let secrets = attempt.unlock(&old_record)?;
            let change =
                account::change_password(&secrets, &old_record, new.as_bytes(), params, &policy)?;
            Ok::<_, account::AccountError>((change, current_auth_key))
        })
        .await?
        .map_err(|err| match err {
            account::AccountError::WrongSecret => SyncError::InvalidCredentials,
            other => other.into(),
        })?;
        let body = ChangePasswordRequest {
            current_auth_key: b64_encode(current_auth_key.as_bytes()),
            new_auth_key: b64_encode(change.auth_key.as_bytes()),
            record: convert(&change.record)?,
        };
        let request = self
            .transport
            .authed(Method::POST, "/v1/account/password")?
            .json(&body);
        transport::unit(request).await
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let list: SessionList =
            transport::json(self.transport.authed(Method::GET, "/v1/sessions")?).await?;
        Ok(list.sessions)
    }

    /// Ends another (or this) session; its WebSocket connections close too.
    pub async fn revoke_session(&self, session_id: &str) -> Result<()> {
        let path = format!("/v1/sessions/{session_id}");
        transport::unit(self.transport.authed(Method::DELETE, &path)?).await
    }

    pub async fn account_record(&self) -> Result<AccountRecord> {
        let wire: ne_sync_protocol::wire::AccountRecordWire =
            transport::json(self.transport.authed(Method::GET, "/v1/account/record")?).await?;
        convert(&wire)
    }

    pub async fn put_vault_record(&self, record: &VaultRecord) -> Result<()> {
        let body: ne_sync_protocol::wire::VaultRecordWire = convert(record)?;
        transport::unit(self.transport.authed(Method::PUT, "/v1/vault")?.json(&body)).await
    }

    /// The stored vault record, or `None` if no vault was ever set up.
    pub async fn vault_record(&self) -> Result<Option<VaultRecord>> {
        let request = self.transport.authed(Method::GET, "/v1/vault")?;
        match transport::json::<ne_sync_protocol::wire::VaultRecordWire>(request).await {
            Ok(wire) => Ok(Some(convert(&wire)?)),
            Err(SyncError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn put_workspace_key(&self, wrapped: &WrappedKey) -> Result<()> {
        let body = WorkspaceKeyBody {
            workspace_key: convert(wrapped)?,
        };
        transport::unit(
            self.transport
                .authed(Method::PUT, "/v1/workspace-key")?
                .json(&body),
        )
        .await
    }

    pub async fn workspace_key(&self) -> Result<WrappedKey> {
        let body: WorkspaceKeyBody =
            transport::json(self.transport.authed(Method::GET, "/v1/workspace-key")?).await?;
        convert(&body.workspace_key)
    }
}
