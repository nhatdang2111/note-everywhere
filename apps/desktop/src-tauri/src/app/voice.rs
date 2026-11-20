//! Push-to-talk transcription. Everything runs on this machine.
//!
//! The speech engine is behind the `transcribe` Cargo feature because it needs
//! a C++ toolchain to build. Without it every voice command except
//! `voice_status` reports `voice_unavailable`.

use super::types::VoiceStatus;
use super::{AppCore, AppError, AppResult};

/// 16-bit mono PCM WAV of 16 kHz samples, for keeping a recording.
#[cfg_attr(not(feature = "transcribe"), allow(dead_code))]
pub(crate) fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX - 36);
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        // Clamped, so the cast cannot overflow; rounding is irrelevant for speech.
        #[allow(clippy::cast_possible_truncation)]
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav
}

#[cfg(not(feature = "transcribe"))]
mod imp {
    use super::*;

    #[derive(Default)]
    pub struct VoiceState {
        _unused: (),
    }

    fn unavailable() -> AppError {
        AppError::new("voice_unavailable", "This build has no speech recognition.")
    }

    impl AppCore {
        pub(crate) fn voice_shutdown(&self) {}

        pub(crate) fn register_hotkey(&self, _accelerator: &str) -> AppResult<()> {
            Ok(())
        }

        pub(crate) fn register_saved_hotkey(&self) {}

        pub async fn voice_status(&self) -> AppResult<VoiceStatus> {
            Ok(VoiceStatus {
                available: false,
                recording: false,
                active_model: None,
                models: Vec::new(),
            })
        }

        pub async fn voice_download_model(&self, _name: &str) -> AppResult<()> {
            Err(unavailable())
        }

        pub async fn voice_delete_model(&self, _name: &str) -> AppResult<()> {
            Err(unavailable())
        }

        pub async fn voice_select_model(&self, _name: &str) -> AppResult<()> {
            Err(unavailable())
        }

        pub async fn voice_start(&self) -> AppResult<()> {
            Err(unavailable())
        }

        pub async fn voice_stop(&self) -> AppResult<super::super::types::VoiceText> {
            Err(unavailable())
        }

        pub async fn voice_cancel(&self) -> AppResult<()> {
            Err(unavailable())
        }
    }
}

#[cfg(feature = "transcribe")]
mod imp {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use ne_voice::capture::{CaptureConfig, Recording};
    use ne_voice::models::{CATALOG, DEFAULT_MODEL_ID, ModelStore, find_model};
    use ne_voice::transcribe::{Language, TranscribeOptions, Transcriber};
    use ne_voice::vad::{VadConfig, trim_silence};
    use ne_voice::{CancelToken, SAMPLE_RATE, VoiceError};

    use super::super::types::{VoiceModel, VoiceText};
    use super::super::{CoreEvent, lock};
    use super::*;

    const MODEL_SETTING: &str = "voice.model";

    #[derive(Default)]
    pub struct VoiceState {
        recording: Mutex<Option<Recording>>,
        transcriber: Mutex<Option<(String, Transcriber)>>,
        cancel: Mutex<Option<CancelToken>>,
    }

    fn map(err: VoiceError) -> AppError {
        match err {
            VoiceError::PermissionDenied => AppError::new(
                "microphone_denied",
                "Microphone access is turned off for this app in the system settings.",
            ),
            VoiceError::NoInputDevice | VoiceError::DeviceNotFound(_) => {
                AppError::new("no_microphone", "No microphone was found.")
            }
            VoiceError::UnknownModel(_) => {
                AppError::new("unknown_model", "That speech model does not exist.")
            }
            VoiceError::ModelNotInstalled(_) => {
                AppError::new("voice_model_missing", "Download a speech model first.")
            }
            VoiceError::Download(_) | VoiceError::ChecksumMismatch { .. } => {
                AppError::new("download_failed", "The model could not be downloaded.")
            }
            VoiceError::Cancelled | VoiceError::TranscriptionCancelled => {
                AppError::new("cancelled", "Cancelled.")
            }
            VoiceError::NoAudio => AppError::new("no_audio", "Nothing was recorded."),
            VoiceError::Io { .. } => {
                AppError::new("io_error", "A file could not be read or written.")
            }
            VoiceError::NotRecording | VoiceError::Audio(_) | VoiceError::Transcribe(_) => {
                AppError::new("voice_error", "Speech recognition failed.")
            }
        }
    }

    impl AppCore {
        fn model_store(&self) -> ModelStore {
            ModelStore::new(self.inner.cfg.models_dir.clone())
        }

        /// The chosen model if it is installed, else the preferred installed one.
        fn active_model(&self) -> Option<String> {
            let store = self.model_store();
            let chosen = self
                .with_ws(|ws| Ok(ws.setting(MODEL_SETTING)?))
                .ok()
                .flatten()
                .filter(|name| find_model(name).is_some_and(|s| store.is_installed(s)));
            chosen.or_else(|| {
                let mut ids: Vec<&str> = CATALOG
                    .iter()
                    .filter(|s| store.is_installed(s))
                    .map(|s| s.id)
                    .collect();
                ids.sort_by_key(|id| *id != DEFAULT_MODEL_ID);
                ids.first().map(|id| (*id).to_owned())
            })
        }

        pub(crate) fn voice_shutdown(&self) {
            lock(&self.inner.voice.recording).take();
            if let Some(cancel) = lock(&self.inner.voice.cancel).take() {
                cancel.cancel();
            }
        }

        pub(crate) fn register_hotkey(&self, accelerator: &str) -> AppResult<()> {
            self.inner
                .deps
                .hotkeys
                .register(accelerator)
                .map_err(|_| AppError::new("invalid_hotkey", "That shortcut cannot be used."))
        }

        /// Registers the stored hold-to-talk shortcut at start-up.
        pub(crate) fn register_saved_hotkey(&self) {
            let _ = self.register_hotkey(&self.settings_now().voice_hotkey);
        }

        pub async fn voice_status(&self) -> AppResult<VoiceStatus> {
            let store = self.model_store();
            Ok(VoiceStatus {
                available: true,
                recording: lock(&self.inner.voice.recording).is_some(),
                active_model: self.active_model(),
                models: CATALOG
                    .iter()
                    .map(|spec| VoiceModel {
                        name: spec.id.to_owned(),
                        byte_length: spec.size_bytes,
                        installed: store.is_installed(spec),
                    })
                    .collect(),
            })
        }

        pub async fn voice_download_model(&self, name: &str) -> AppResult<()> {
            let spec =
                find_model(name).ok_or_else(|| map(VoiceError::UnknownModel(name.to_owned())))?;
            let store = self.model_store();
            let events = self.inner.deps.events.clone();
            let id = spec.id.to_owned();
            self.blocking(move |_| {
                let mut last = Instant::now() - Duration::from_secs(1);
                let cancel = CancelToken::new();
                store
                    .download(
                        &id,
                        |progress| {
                            // Chunks arrive far faster than the UI needs them.
                            let done = progress.downloaded >= progress.total;
                            if done || last.elapsed() >= Duration::from_millis(150) {
                                last = Instant::now();
                                events.emit(CoreEvent::VoiceDownloadProgress {
                                    name: id.clone(),
                                    received_bytes: progress.downloaded,
                                    total_bytes: progress.total,
                                });
                            }
                        },
                        &cancel,
                    )
                    .map(drop)
                    .map_err(map)
            })
            .await
        }

        pub async fn voice_delete_model(&self, name: &str) -> AppResult<()> {
            self.model_store().delete(name).map_err(map)?;
            let mut cached = lock(&self.inner.voice.transcriber);
            if cached.as_ref().is_some_and(|(id, _)| id == name) {
                *cached = None;
            }
            Ok(())
        }

        pub async fn voice_select_model(&self, name: &str) -> AppResult<()> {
            let spec =
                find_model(name).ok_or_else(|| map(VoiceError::UnknownModel(name.to_owned())))?;
            if !self.model_store().is_installed(spec) {
                return Err(map(VoiceError::ModelNotInstalled(name.to_owned())));
            }
            self.store(|ws| ws.set_setting(MODEL_SETTING, spec.id))
        }

        pub async fn voice_start(&self) -> AppResult<()> {
            if self.active_model().is_none() {
                return Err(map(VoiceError::ModelNotInstalled(String::new())));
            }
            if lock(&self.inner.voice.recording).is_some() {
                return Err(AppError::new(
                    "already_recording",
                    "A recording is already in progress.",
                ));
            }
            let recording = self
                .blocking(|_| Recording::start(&CaptureConfig::default()).map_err(map))
                .await?;
            *lock(&self.inner.voice.recording) = Some(recording);
            Ok(())
        }

        pub async fn voice_stop(&self) -> AppResult<VoiceText> {
            let recording = lock(&self.inner.voice.recording)
                .take()
                .ok_or_else(|| map(VoiceError::NotRecording))?;
            let model = self
                .active_model()
                .ok_or_else(|| map(VoiceError::ModelNotInstalled(String::new())))?;
            let settings = self.settings_now();
            let language = settings
                .voice_language
                .parse::<Language>()
                .unwrap_or_default();
            let cancel = CancelToken::new();
            *lock(&self.inner.voice.cancel) = Some(cancel.clone());
            let store = self.model_store();
            // Stopping joins the capture thread and transcription is CPU-heavy: both stay off the async threads.
            let outcome = self
                .blocking(move |core| {
                    let recorded = recording.stop().map_err(map)?;
                    let speech = trim_silence(&recorded.samples, &VadConfig::default());
                    if speech.is_empty() {
                        return Ok((String::new(), recorded.samples));
                    }
                    let transcriber = {
                        let mut cached = lock(&core.inner.voice.transcriber);
                        match cached.as_ref().filter(|(id, _)| *id == model) {
                            Some((_, t)) => t.clone(),
                            None => {
                                let spec = find_model(&model)
                                    .ok_or_else(|| map(VoiceError::UnknownModel(model.clone())))?;
                                let loaded =
                                    Transcriber::load(&store.path_for(spec), true).map_err(map)?;
                                *cached = Some((model.clone(), loaded.clone()));
                                loaded
                            }
                        }
                    };
                    let options = TranscribeOptions {
                        language,
                        ..TranscribeOptions::default()
                    };
                    let text = transcriber
                        .transcribe(speech, &options, &cancel)
                        .map_err(map)?;
                    Ok((text, recorded.samples))
                })
                .await;
            lock(&self.inner.voice.cancel).take();
            let (text, samples) = outcome?;
            let audio = if settings.voice_keep_audio && !samples.is_empty() {
                let wav = super::encode_wav(&samples, SAMPLE_RATE);
                Some(
                    self.attachment_add_bytes(wav, "audio/wav", Some("voice-note.wav"))
                        .await?,
                )
            } else {
                None
            };
            Ok(VoiceText { text, audio })
        }

        pub async fn voice_cancel(&self) -> AppResult<()> {
            lock(&self.inner.voice.recording).take();
            if let Some(cancel) = lock(&self.inner.voice.cancel).as_ref() {
                cancel.cancel();
            }
            Ok(())
        }
    }
}

pub(crate) use imp::VoiceState;

#[cfg(test)]
mod tests {
    use super::encode_wav;

    #[test]
    fn wav_header_and_samples() {
        let wav = encode_wav(&[0.0, 1.0, -1.0, 2.0], 16_000);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(
            u32::from_le_bytes(wav[24..28].try_into().expect("rate")),
            16_000
        );
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().expect("len")), 8);
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(
            i16::from_le_bytes(wav[46..48].try_into().expect("s")),
            i16::MAX
        );
        assert_eq!(
            i16::from_le_bytes(wav[48..50].try_into().expect("s")),
            -i16::MAX
        );
        assert_eq!(
            i16::from_le_bytes(wav[50..52].try_into().expect("s")),
            i16::MAX
        );
    }
}
