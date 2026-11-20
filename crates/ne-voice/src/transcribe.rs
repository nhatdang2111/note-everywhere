//! Offline speech-to-text on whisper.cpp. Nothing here touches the network.

use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::cancel::CancelToken;
use crate::error::VoiceError;

/// Spoken language. `Auto` lets the model detect it per recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Auto,
    Vietnamese,
    English,
}

impl Language {
    fn code(self) -> Option<&'static str> {
        match self {
            Language::Auto => None,
            Language::Vietnamese => Some("vi"),
            Language::English => Some("en"),
        }
    }
}

impl std::str::FromStr for Language {
    type Err = VoiceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Language::Auto),
            "vi" => Ok(Language::Vietnamese),
            "en" => Ok(Language::English),
            other => Err(VoiceError::Transcribe(format!(
                "unsupported language: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    pub language: Language,
    /// Custom vocabulary or context, e.g. names and technical terms. It biases
    /// the decoder towards that spelling.
    pub initial_prompt: Option<String>,
    /// CPU threads for decoding; `None` uses all available cores.
    pub threads: Option<usize>,
}

/// A loaded model. Cheap to clone; loading is the expensive step, so keep one
/// around and reuse it for every recording.
#[derive(Clone)]
pub struct Transcriber {
    ctx: Arc<WhisperContext>,
}

impl Transcriber {
    /// Loads a ggml model. `use_gpu` only has an effect when the crate is
    /// compiled with a GPU backend feature (`metal`, `vulkan`).
    pub fn load(model_path: &Path, use_gpu: bool) -> Result<Self, VoiceError> {
        let path = model_path
            .to_str()
            .ok_or_else(|| VoiceError::Transcribe("model path is not valid UTF-8".into()))?;
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(path, params)
            .map_err(|e| VoiceError::Transcribe(format!("cannot load model: {e}")))?;
        Ok(Self { ctx: Arc::new(ctx) })
    }

    /// Transcribes 16 kHz mono f32 PCM on the calling thread.
    pub fn transcribe(
        &self,
        pcm: &[f32],
        options: &TranscribeOptions,
        cancel: &CancelToken,
    ) -> Result<String, VoiceError> {
        if pcm.is_empty() {
            return Err(VoiceError::NoAudio);
        }
        if cancel.is_cancelled() {
            return Err(VoiceError::TranscriptionCancelled);
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| VoiceError::Transcribe(e.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(options.language.code());
        params.set_translate(false);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(threads) = options.threads {
            params.set_n_threads(i32::try_from(threads).unwrap_or(i32::MAX));
        }
        if let Some(prompt) = options.initial_prompt.as_deref().filter(|p| !p.is_empty()) {
            params.set_initial_prompt(prompt);
        }
        let flag = cancel.clone();
        // whisper-rs 0.16 hands the callback to C as a pointer to a boxed trait
        // object but reads it back as the closure type. Naming the closure type
        // `Box<dyn FnMut>` makes both sides agree; a bare closure is UB.
        let abort: Box<dyn FnMut() -> bool> = Box::new(move || flag.is_cancelled());
        params.set_abort_callback_safe(abort);

        let result = state.full(params, pcm);
        if cancel.is_cancelled() {
            return Err(VoiceError::TranscriptionCancelled);
        }
        result.map_err(|e| VoiceError::Transcribe(e.to_string()))?;

        let mut text = String::new();
        for segment in state.as_iter() {
            let piece = segment
                .to_str_lossy()
                .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
            text.push_str(&piece);
        }
        Ok(text.trim().to_owned())
    }

    /// Runs [`Transcriber::transcribe`] on a background thread so the caller
    /// (a UI thread) never blocks.
    pub fn spawn(&self, pcm: Vec<f32>, options: TranscribeOptions) -> TranscriptionJob {
        let cancel = CancelToken::new();
        let worker = self.clone();
        let token = cancel.clone();
        let handle = std::thread::spawn(move || worker.transcribe(&pcm, &options, &token));
        TranscriptionJob { cancel, handle }
    }
}

/// Handle to a transcription running off the caller's thread.
pub struct TranscriptionJob {
    cancel: CancelToken,
    handle: JoinHandle<Result<String, VoiceError>>,
}

impl TranscriptionJob {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Blocks until the job ends and returns its result.
    pub fn wait(self) -> Result<String, VoiceError> {
        self.handle
            .join()
            .map_err(|_| VoiceError::Transcribe("transcription thread panicked".into()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_parses_known_codes() {
        assert_eq!("auto".parse::<Language>().expect("auto"), Language::Auto);
        assert_eq!("vi".parse::<Language>().expect("vi"), Language::Vietnamese);
        assert_eq!("en".parse::<Language>().expect("en"), Language::English);
        assert!("fr".parse::<Language>().is_err());
    }

    #[test]
    fn loading_a_missing_model_fails_cleanly() {
        let result = Transcriber::load(Path::new("/nonexistent/model.bin"), false);
        assert!(matches!(result, Err(VoiceError::Transcribe(_))));
    }
}
