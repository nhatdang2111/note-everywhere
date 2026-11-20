//! The Tauri updater plugin behind the core's [`UpdateBackend`].
//!
//! The public key is not in the source. It comes from `plugins.updater.pubkey`
//! of the Tauri configuration, which the release workflow merges in from a
//! repository variable when it also signs the update files. The base
//! `tauri.conf.json` has no `plugins.updater` section, so an ordinary build
//! (a local build, CI, a fork) has no key: the plugin is then not registered
//! at all (see [`updater_configured`]) and the core reports updates as
//! unavailable. The endpoint is fixed in [`UPDATE_ENDPOINT`].

use std::sync::{Arc, Mutex, PoisonError};

use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Error, Update, UpdaterExt};
use url::Url;

use crate::app::{BoxFuture, ProgressFn, UpdateBackend, UpdateFailure, UpdateOffer};

/// The `latest.json` asset of the newest published release.
pub const UPDATE_ENDPOINT: &str =
    "https://github.com/nhatdang2111/note-everywhere/releases/latest/download/latest.json";

/// Whether the configuration carries an updater public key.
pub fn updater_configured(config: &tauri::Config) -> bool {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|section| section.get("pubkey"))
        .and_then(|key| key.as_str())
        .is_some_and(|key| !key.trim().is_empty())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn classify(error: &Error) -> UpdateFailure {
    match error {
        Error::Reqwest(_) | Error::Network(_) | Error::ReleaseNotFound => {
            UpdateFailure::Unreachable
        }
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::MissingSignedVersion
        | Error::SignedVersionMismatch { .. } => UpdateFailure::BadSignature,
        _ => UpdateFailure::Other,
    }
}

pub struct TauriUpdater<R: Runtime> {
    app: AppHandle<R>,
    configured: bool,
    endpoints: Vec<Url>,
    /// The update the last check announced.
    found: Mutex<Option<Update>>,
    /// Its package, once downloaded and verified.
    package: Mutex<Option<Vec<u8>>>,
}

impl<R: Runtime> TauriUpdater<R> {
    /// `configured` is [`updater_configured`] of the app's configuration; the
    /// updater plugin must be registered exactly when it is true.
    pub fn new(app: AppHandle<R>, configured: bool) -> Self {
        let endpoint = Url::parse(UPDATE_ENDPOINT).expect("the update endpoint is a valid URL");
        Self {
            app,
            configured,
            endpoints: vec![endpoint],
            found: Mutex::new(None),
            package: Mutex::new(None),
        }
    }

    /// Replaces the endpoint, for tests against a local server.
    pub fn with_endpoint(mut self, endpoint: Url) -> Self {
        self.endpoints = vec![endpoint];
        self
    }

    fn found(&self) -> Option<Update> {
        lock(&self.found).clone()
    }
}

impl<R: Runtime> UpdateBackend for TauriUpdater<R> {
    fn configured(&self) -> bool {
        self.configured
    }

    fn check(&self) -> BoxFuture<'_, Result<Option<UpdateOffer>, UpdateFailure>> {
        Box::pin(async move {
            let updater = self
                .app
                .updater_builder()
                .endpoints(self.endpoints.clone())
                .and_then(|builder| builder.build())
                .map_err(|err| classify(&err))?;
            let found = updater.check().await.map_err(|err| classify(&err))?;
            *lock(&self.package) = None;
            let offer = found.as_ref().map(|update| UpdateOffer {
                version: update.version.clone(),
                notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
            });
            *lock(&self.found) = found;
            Ok(offer)
        })
    }

    fn download(&self, progress: ProgressFn) -> BoxFuture<'_, Result<(), UpdateFailure>> {
        Box::pin(async move {
            let update = self.found().ok_or(UpdateFailure::Other)?;
            let received = Arc::new(Mutex::new(0u64));
            let on_chunk = {
                let received = received.clone();
                move |chunk: usize, total: Option<u64>| {
                    let mut sum = lock(&received);
                    *sum += u64::try_from(chunk).unwrap_or(0);
                    progress(*sum, total);
                }
            };
            // The plugin verifies the signature before it returns the bytes.
            let bytes = update
                .download(on_chunk, || {})
                .await
                .map_err(|err| classify(&err))?;
            *lock(&self.package) = Some(bytes);
            Ok(())
        })
    }

    fn install(&self) -> Result<(), UpdateFailure> {
        let update = self.found().ok_or(UpdateFailure::Other)?;
        let package = lock(&self.package).take().ok_or(UpdateFailure::Other)?;
        // Verifies the signature again before touching the installation.
        update.install(package).map_err(|err| classify(&err))
    }

    fn restart(&self) {
        self.app.restart();
    }
}
