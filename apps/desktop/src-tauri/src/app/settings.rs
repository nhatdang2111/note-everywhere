//! Settings, persisted in the store's key-value table (`settings.*` keys).
//!
//! Before a workspace exists they live in memory and are written once it is
//! created, so the first-run screens can already change the theme.

use std::time::Duration;

use super::types::{Settings, SettingsPatch};
use super::{AppCore, AppError, AppResult, lock};

const MAX_IDLE_SECONDS: u64 = 24 * 60 * 60;

fn validate(settings: &Settings) -> AppResult<()> {
    if !matches!(settings.theme.as_str(), "system" | "light" | "dark") {
        return Err(AppError::invalid(
            "The theme must be system, light or dark.",
        ));
    }
    if settings.vault_idle_lock_seconds > MAX_IDLE_SECONDS {
        return Err(AppError::invalid(
            "The vault idle time cannot be more than 24 hours.",
        ));
    }
    if !matches!(settings.voice_language.as_str(), "auto" | "vi" | "en") {
        return Err(AppError::invalid(
            "The voice language must be auto, vi or en.",
        ));
    }
    let hotkey = settings.voice_hotkey.trim();
    if hotkey.is_empty() || hotkey.len() > 64 {
        return Err(AppError::invalid(
            "The voice hotkey must be 1 to 64 characters.",
        ));
    }
    Ok(())
}

impl AppCore {
    pub(crate) fn load_settings(&self) -> AppResult<()> {
        let defaults = Settings::default();
        let loaded = self.store(|ws| {
            let text = |key: &str, default: &str| -> ne_store::Result<String> {
                Ok(ws.setting(key)?.unwrap_or_else(|| default.to_owned()))
            };
            Ok(Settings {
                theme: text("settings.theme", &defaults.theme)?,
                vault_idle_lock_seconds: text("settings.vault_idle_lock_seconds", "")?
                    .parse()
                    .unwrap_or(defaults.vault_idle_lock_seconds),
                voice_hotkey: text("settings.voice_hotkey", &defaults.voice_hotkey)?,
                voice_language: text("settings.voice_language", &defaults.voice_language)?,
                voice_keep_audio: text("settings.voice_keep_audio", "")?
                    .parse()
                    .unwrap_or(defaults.voice_keep_audio),
            })
        })?;
        // A damaged stored value falls back to the defaults rather than blocking start-up.
        *lock(&self.inner.settings) = if validate(&loaded).is_ok() {
            loaded
        } else {
            defaults
        };
        self.apply_settings_effects();
        Ok(())
    }

    pub(crate) fn persist_settings(&self) -> AppResult<()> {
        let settings = lock(&self.inner.settings).clone();
        self.store(|ws| {
            ws.set_setting("settings.theme", &settings.theme)?;
            ws.set_setting(
                "settings.vault_idle_lock_seconds",
                &settings.vault_idle_lock_seconds.to_string(),
            )?;
            ws.set_setting("settings.voice_hotkey", &settings.voice_hotkey)?;
            ws.set_setting("settings.voice_language", &settings.voice_language)?;
            ws.set_setting(
                "settings.voice_keep_audio",
                &settings.voice_keep_audio.to_string(),
            )
        })
    }

    pub fn settings_now(&self) -> Settings {
        lock(&self.inner.settings).clone()
    }

    pub async fn settings_get(&self) -> AppResult<Settings> {
        Ok(self.settings_now())
    }

    pub async fn settings_set(&self, patch: SettingsPatch) -> AppResult<Settings> {
        let before = self.settings_now();
        let mut next = before.clone();
        if let Some(v) = patch.theme {
            next.theme = v;
        }
        if let Some(v) = patch.vault_idle_lock_seconds {
            next.vault_idle_lock_seconds = v;
        }
        if let Some(v) = patch.voice_hotkey {
            next.voice_hotkey = v.trim().to_owned();
        }
        if let Some(v) = patch.voice_language {
            next.voice_language = v;
        }
        if let Some(v) = patch.voice_keep_audio {
            next.voice_keep_audio = v;
        }
        validate(&next)?;
        if next.voice_hotkey != before.voice_hotkey {
            self.register_hotkey(&next.voice_hotkey)?;
        }
        *lock(&self.inner.settings) = next.clone();
        if lock(&self.inner.ws).is_some() {
            self.persist_settings()?;
        }
        self.apply_settings_effects();
        Ok(next)
    }

    /// Pushes settings that other components read: the vault idle time.
    fn apply_settings_effects(&self) {
        let seconds = self.settings_now().vault_idle_lock_seconds;
        let idle = (seconds > 0).then(|| Duration::from_secs(seconds));
        self.inner.idle_after.send_replace(idle);
        self.apply_vault_policy();
    }
}
