use std::path::PathBuf;

/// Errors from capture, model management and transcription.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("no audio input device available")]
    NoInputDevice,
    #[error("audio input device not found: {0}")]
    DeviceNotFound(String),
    #[error("microphone access was denied by the operating system")]
    PermissionDenied,
    #[error("audio device error: {0}")]
    Audio(String),
    #[error("recording is not active")]
    NotRecording,
    #[error("unknown model: {0}")]
    UnknownModel(String),
    #[error("model is not installed: {0}")]
    ModelNotInstalled(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("checksum mismatch for {name}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("download cancelled")]
    Cancelled,
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("transcription error: {0}")]
    Transcribe(String),
    #[error("transcription cancelled")]
    TranscriptionCancelled,
    #[error("no audio to transcribe")]
    NoAudio,
}

impl VoiceError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
