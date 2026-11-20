//! Audio capture and offline speech-to-text. All processing is local: audio
//! never leaves the machine, and the only network use is downloading a model
//! file the user asked for.
//!
//! Capture, silence trimming, model management and word error rate need no
//! native toolchain. The whisper.cpp engine is behind the `transcribe` feature
//! (add `metal` or `vulkan` for GPU decoding).

pub mod capture;
pub mod models;
pub mod resample;
pub mod vad;
pub mod wer;

mod cancel;
mod error;

#[cfg(feature = "transcribe")]
pub mod transcribe;

pub use cancel::CancelToken;
pub use error::VoiceError;

/// Sample rate (Hz) of the mono PCM that models expect and capture produces.
pub const SAMPLE_RATE: u32 = 16_000;
