//! Account, login and key-record endpoints.
//!
//! Single tenant: at most one account ever exists. The server stores what the
//! client uploads (salt, KDF parameters, verifier, wrapped keys) and never
//! sees a password or an unwrapped key.

use ne_crypto::auth;
use ne_sync_protocol::ids::{hex_encode, is_valid_label, normalize_username};
use ne_sync_protocol::wire::{
    AccountRecordWire, ChangePasswordRequest, KdfParamsWire, LoginRequest, RecoverRequest,
    RecoveryLookupRequest, RecoveryLookupResponse, RegisterRequest, SaltRequest, SaltResponse,
    SessionList, SessionResponse, VaultRecordWire, WRAPPED_KEY_LEN, WorkspaceKeyBody,
    b64_decode_array, b64_encode, is_wrapped_key,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use worker::{Request, Response};

use crate::db::{META_ACCOUNT, META_SEED, META_USERNAME, META_VAULT, META_WORKSPACE};
use crate::http::{ApiError, ApiResult, json, read_json};
use crate::workspace_do::{Workspace, ok_empty};
use crate::{random, sessions, throttle};

/// Shortest setup code the server accepts for registration.
const MIN_SETUP_CODE_LEN: usize = 12;
/// Argon2 settings advertised for names that do not exist. They match the
/// client default, so a real account created with defaults looks identical.
const DEFAULT_KDF: KdfParamsWire = KdfParamsWire {
    memory_kib: 256 * 1024,
    iterations: 3,
    parallelism: 4,
};
const SETUP_BUCKET: &str = "setup:global";
/// Version byte every sealed blob starts with, so a made-up blob has the
/// same first byte as a real one.
const SEALED_VERSION: u8 = 1;

fn digest_eq(a: &str, b: &str) -> bool {
    Sha256::digest(a.as_bytes())
        .ct_eq(&Sha256::digest(b.as_bytes()))
        .into()
}

struct Account {
    username: String,
    record: AccountRecordWire,
}

impl Workspace {
    fn load_account(&self) -> ApiResult<Option<Account>> {
        let (Some(username), Some(record)) =
            (self.db.meta(META_USERNAME)?, self.db.meta(META_ACCOUNT)?)
        else {
            return Ok(None);
        };
        let record = serde_json::from_str(&record).map_err(|_| ApiError::Unavailable)?;
        Ok(Some(Account { username, record }))
    }

    fn require_account(&self) -> ApiResult<Account> {
        self.load_account()?.ok_or(ApiError::NotFound)
    }

    fn save_record(&self, record: &AccountRecordWire) -> ApiResult<()> {
        let text = serde_json::to_string(record).map_err(|_| ApiError::Unavailable)?;
        self.db.set_meta(META_ACCOUNT, &text)?;
        Ok(())
    }

    /// A random per-deployment value that makes fake salts unpredictable.
    fn seed(&self) -> ApiResult<String> {
        if let Some(seed) = self.db.meta(META_SEED)? {
            return Ok(seed);
        }
        let seed = hex_encode(&random::bytes::<32>()?);
        self.db.set_meta(META_SEED, &seed)?;
        Ok(seed)
    }

    fn fake_digest(&self, label: &str, name: &str) -> ApiResult<[u8; 32]> {
        let mut hasher = Sha256::new();
        hasher.update(b"note-everywhere/v1/fake-");
        hasher.update(label.as_bytes());
        hasher.update(self.seed()?.as_bytes());
        hasher.update(name.as_bytes());
        Ok(hasher.finalize().into())
    }

    /// A wrapped-key-shaped blob that is the same for the same name every
    /// time and unpredictable without the seed.
    fn fake_wrapped_key(&self, label: &str, name: &str) -> ApiResult<String> {
        let mut bytes = Vec::with_capacity(96);
        for part in 0..3u8 {
            bytes.extend_from_slice(&self.fake_digest(&format!("{label}{part}"), name)?);
        }
        bytes.truncate(WRAPPED_KEY_LEN);
        bytes[0] = SEALED_VERSION;
        Ok(b64_encode(&bytes))
    }

    fn session_response(
        &self,
        label: &str,
        record: AccountRecordWire,
        now: u64,
    ) -> ApiResult<SessionResponse> {
        let session = sessions::create(&self.db, label, self.cfg.session_ttl_ms, now)?;
        let workspace_id = self.db.meta(META_WORKSPACE)?.ok_or(ApiError::Unavailable)?;
        Ok(SessionResponse {
            token: session.token,
            session_id: session.id,
            expires_at: session.expires_at,
            workspace_id,
            record,
        })
    }

    pub(crate) async fn salt(&self, req: &mut Request) -> ApiResult<Response> {
        let body: SaltRequest = read_json(req).await?;
        let name = normalize_username(&body.username).ok_or(ApiError::BadRequest)?;
        let account = self.load_account()?;
        let response = match &account {
            Some(a) if digest_eq(&a.username, &name) => SaltResponse {
                kdf_salt: a.record.kdf_salt.clone(),
                kdf_params: a.record.kdf_params,
            },
            other => SaltResponse {
                kdf_salt: b64_encode(&self.fake_digest("salt", &name)?[..16]),
                kdf_params: other.as_ref().map_or(DEFAULT_KDF, |a| a.record.kdf_params),
            },
        };
        Ok(json(200, &response)?)
    }

    pub(crate) async fn register(
        &self,
        req: &mut Request,
        ip: &str,
        now: u64,
    ) -> ApiResult<Response> {
        let body: RegisterRequest = read_json(req).await?;
        let setup = self
            .cfg
            .setup_code
            .as_deref()
            .filter(|code| code.len() >= MIN_SETUP_CODE_LEN)
            .ok_or(ApiError::Forbidden)?;
        if self.load_account()?.is_some() {
            return Err(ApiError::RegistrationClosed);
        }
        let buckets = [throttle::ip_key(ip), SETUP_BUCKET.to_owned()];
        if let Some(secs) = throttle::remaining_lock(&self.db, &buckets, now)? {
            return Err(ApiError::Locked(secs));
        }
        if !digest_eq(&body.setup_code, setup) {
            throttle::record_failure(&self.db, &self.cfg, &buckets, now)?;
            return Err(ApiError::Forbidden);
        }
        let username = normalize_username(&body.username).ok_or(ApiError::BadRequest)?;
        let auth_key: [u8; auth::LEN] =
            b64_decode_array(&body.auth_key).ok_or(ApiError::BadRequest)?;
        let recovery_key: [u8; auth::LEN] =
            b64_decode_array(&body.recovery_auth_key).ok_or(ApiError::BadRequest)?;
        if !is_valid_label(&body.device_label) || !body.record.is_valid() {
            return Err(ApiError::BadRequest);
        }
        let workspace_id = format!("ws-{}", hex_encode(&random::bytes::<16>()?));

        // Everything is validated; from here on only writes, with no await in
        // between, so a registration is stored completely or not at all.
        let mut record = body.record;
        record.auth_verifier = b64_encode(&auth::auth_verifier(&auth_key));
        record.recovery_verifier = Some(b64_encode(&auth::auth_verifier(&recovery_key)));
        self.db.set_meta(META_USERNAME, &username)?;
        self.save_record(&record)?;
        self.db.set_meta(META_WORKSPACE, &workspace_id)?;
        throttle::record_success(&self.db, &buckets)?;
        Ok(json(
            201,
            &self.session_response(&body.device_label, record, now)?,
        )?)
    }

    pub(crate) async fn login(&self, req: &mut Request, ip: &str, now: u64) -> ApiResult<Response> {
        let body: LoginRequest = read_json(req).await?;
        let name = normalize_username(&body.username).ok_or(ApiError::BadRequest)?;
        let auth_key: [u8; auth::LEN] =
            b64_decode_array(&body.auth_key).ok_or(ApiError::BadRequest)?;
        if !is_valid_label(&body.device_label) {
            return Err(ApiError::BadRequest);
        }
        let buckets = [throttle::account_key(&name), throttle::ip_key(ip)];
        if let Some(secs) = throttle::remaining_lock(&self.db, &buckets, now)? {
            return Err(ApiError::Locked(secs));
        }
        let account = self.load_account()?;
        // Unknown names run the same verification against a verifier nobody
        // can match, so timing and the error are identical to a wrong password.
        let (verifier, name_matches) = match &account {
            Some(a) => (
                b64_decode_array::<{ auth::LEN }>(&a.record.auth_verifier)
                    .ok_or(ApiError::Unavailable)?,
                digest_eq(&a.username, &name),
            ),
            None => (self.fake_digest("verifier", &name)?, false),
        };
        let key_matches = auth::verify(&auth_key, &verifier);
        let record = match (account, key_matches & name_matches) {
            (Some(a), true) => a.record,
            _ => {
                throttle::record_failure(&self.db, &self.cfg, &buckets, now)?;
                return Err(ApiError::InvalidCredentials);
            }
        };
        throttle::record_success(&self.db, &buckets)?;
        Ok(json(
            200,
            &self.session_response(&body.device_label, record, now)?,
        )?)
    }

    /// Hands out the blobs the recovery key opens. Unknown names get made-up
    /// blobs, so the answer says nothing about which names exist.
    /// Test-only: turns the stored account into one created before recovery
    /// verification existed.
    pub(crate) fn strip_recovery_verifier(&self) -> ApiResult<()> {
        let mut account = self.require_account()?;
        account.record.recovery_verifier = None;
        self.save_record(&account.record)
    }

    pub(crate) async fn recovery_lookup(&self, req: &mut Request) -> ApiResult<Response> {
        let body: RecoveryLookupRequest = read_json(req).await?;
        let name = normalize_username(&body.username).ok_or(ApiError::BadRequest)?;
        let response = match self.load_account()? {
            Some(a) if digest_eq(&a.username, &name) => RecoveryLookupResponse {
                root_key_by_recovery: a.record.root_key_by_recovery,
                workspace_key: a.record.workspace_key,
            },
            _ => RecoveryLookupResponse {
                root_key_by_recovery: self.fake_wrapped_key("recovery-root", &name)?,
                workspace_key: self.fake_wrapped_key("recovery-workspace", &name)?,
            },
        };
        Ok(json(200, &response)?)
    }

    /// Sets a new password for someone who proves they hold the recovery key.
    /// Throttled like a login, in buckets of its own. Every session is ended
    /// and a fresh one is returned.
    pub(crate) async fn recover(
        &self,
        req: &mut Request,
        ip: &str,
        now: u64,
    ) -> ApiResult<Response> {
        let body: RecoverRequest = read_json(req).await?;
        let name = normalize_username(&body.username).ok_or(ApiError::BadRequest)?;
        let recovery_key: [u8; auth::LEN] =
            b64_decode_array(&body.recovery_auth_key).ok_or(ApiError::BadRequest)?;
        let new_key: [u8; auth::LEN] =
            b64_decode_array(&body.new_auth_key).ok_or(ApiError::BadRequest)?;
        if !is_valid_label(&body.device_label) || !body.record.is_valid() {
            return Err(ApiError::BadRequest);
        }
        let buckets = [
            throttle::recovery_account_key(&name),
            throttle::recovery_ip_key(ip),
        ];
        if let Some(secs) = throttle::remaining_lock(&self.db, &buckets, now)? {
            return Err(ApiError::Locked(secs));
        }
        let account = self.load_account()?;
        // Unknown names are checked against a verifier nobody can match, so the
        // timing and the answer equal those of a wrong recovery key.
        let (verifier, name_matches) = match &account {
            Some(a) if digest_eq(&a.username, &name) => {
                match a.record.recovery_verifier.as_deref() {
                    Some(stored) => (
                        b64_decode_array::<{ auth::LEN }>(stored).ok_or(ApiError::Unavailable)?,
                        true,
                    ),
                    // Created before recovery verification existed: there is
                    // nothing to check against. Only reachable with the right
                    // name, which is no secret on a one-account server.
                    None => return Err(ApiError::RecoveryUnavailable),
                }
            }
            _ => (self.fake_digest("recovery-verifier", &name)?, false),
        };
        let key_matches = auth::verify(&recovery_key, &verifier);
        let mut record = match (account, key_matches & name_matches) {
            (Some(a), true) => a.record,
            _ => {
                throttle::record_failure(&self.db, &self.cfg, &buckets, now)?;
                return Err(ApiError::InvalidCredentials);
            }
        };
        throttle::record_success(&self.db, &buckets)?;

        // Only the new password material is taken from the request; the
        // recovery blobs and verifier stay as stored, so the recovery key
        // remains valid and unchanged.
        record.kdf_salt = body.record.kdf_salt;
        record.kdf_params = body.record.kdf_params;
        record.root_key_by_password = body.record.root_key_by_password;
        record.auth_verifier = b64_encode(&auth::auth_verifier(&new_key));
        self.save_record(&record)?;
        let revoked = sessions::revoke_all(&self.db)?;
        self.close_session_sockets(&revoked);
        Ok(json(
            200,
            &self.session_response(&body.device_label, record, now)?,
        )?)
    }

    pub(crate) fn logout(&self, req: &Request, now: u64) -> ApiResult<Response> {
        let session = self.authenticate(req, now)?;
        sessions::revoke(&self.db, &session.id)?;
        self.close_session_sockets(&[session.id]);
        ok_empty()
    }

    pub(crate) fn list_sessions(&self, req: &Request, now: u64) -> ApiResult<Response> {
        let session = self.authenticate(req, now)?;
        let sessions = sessions::list(&self.db, &session.id, now)?;
        Ok(json(200, &SessionList { sessions })?)
    }

    pub(crate) fn revoke_session(&self, req: &Request, id: &str, now: u64) -> ApiResult<Response> {
        self.authenticate(req, now)?;
        if id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ApiError::NotFound);
        }
        if !sessions::revoke(&self.db, id)? {
            return Err(ApiError::NotFound);
        }
        self.close_session_sockets(&[id.to_owned()]);
        ok_empty()
    }

    pub(crate) async fn change_password(
        &self,
        req: &mut Request,
        ip: &str,
        now: u64,
    ) -> ApiResult<Response> {
        let session = self.authenticate(req, now)?;
        let body: ChangePasswordRequest = read_json(req).await?;
        let current: [u8; auth::LEN] =
            b64_decode_array(&body.current_auth_key).ok_or(ApiError::BadRequest)?;
        let new_key: [u8; auth::LEN] =
            b64_decode_array(&body.new_auth_key).ok_or(ApiError::BadRequest)?;
        if !body.record.is_valid() {
            return Err(ApiError::BadRequest);
        }
        let account = self.require_account()?;
        let buckets = [
            throttle::account_key(&account.username),
            throttle::ip_key(ip),
        ];
        if let Some(secs) = throttle::remaining_lock(&self.db, &buckets, now)? {
            return Err(ApiError::Locked(secs));
        }
        let stored = b64_decode_array::<{ auth::LEN }>(&account.record.auth_verifier)
            .ok_or(ApiError::Unavailable)?;
        if !auth::verify(&current, &stored) {
            throttle::record_failure(&self.db, &self.cfg, &buckets, now)?;
            return Err(ApiError::InvalidCredentials);
        }
        throttle::record_success(&self.db, &buckets)?;

        let mut record = body.record;
        record.auth_verifier = b64_encode(&auth::auth_verifier(&new_key));
        self.save_record(&record)?;
        let revoked = sessions::revoke_all_except(&self.db, &session.id)?;
        self.close_session_sockets(&revoked);
        ok_empty()
    }

    pub(crate) fn account_record(&self, req: &Request, now: u64) -> ApiResult<Response> {
        self.authenticate(req, now)?;
        Ok(json(200, &self.require_account()?.record)?)
    }

    pub(crate) async fn put_vault(&self, req: &mut Request, now: u64) -> ApiResult<Response> {
        self.authenticate(req, now)?;
        let record: VaultRecordWire = read_json(req).await?;
        if !record.is_valid() {
            return Err(ApiError::BadRequest);
        }
        let text = serde_json::to_string(&record).map_err(|_| ApiError::Unavailable)?;
        self.db.set_meta(META_VAULT, &text)?;
        ok_empty()
    }

    pub(crate) fn get_vault(&self, req: &Request, now: u64) -> ApiResult<Response> {
        self.authenticate(req, now)?;
        let text = self.db.meta(META_VAULT)?.ok_or(ApiError::NotFound)?;
        let record: VaultRecordWire =
            serde_json::from_str(&text).map_err(|_| ApiError::Unavailable)?;
        Ok(json(200, &record)?)
    }

    pub(crate) async fn put_workspace_key(
        &self,
        req: &mut Request,
        now: u64,
    ) -> ApiResult<Response> {
        self.authenticate(req, now)?;
        let body: WorkspaceKeyBody = read_json(req).await?;
        if !is_wrapped_key(&body.workspace_key) {
            return Err(ApiError::BadRequest);
        }
        let mut account = self.require_account()?;
        account.record.workspace_key = body.workspace_key;
        self.save_record(&account.record)?;
        ok_empty()
    }

    pub(crate) fn get_workspace_key(&self, req: &Request, now: u64) -> ApiResult<Response> {
        self.authenticate(req, now)?;
        let account = self.require_account()?;
        Ok(json(
            200,
            &WorkspaceKeyBody {
                workspace_key: account.record.workspace_key,
            },
        )?)
    }
}
