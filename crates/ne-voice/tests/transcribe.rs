//! Needs a downloaded model and the spike clips, so every test is `#[ignore]`.
//!
//! cargo test --release -p ne-voice --features transcribe -- --ignored
//!
//! Override locations with `NE_VOICE_MODEL` (model file) and `NE_VOICE_SPIKE`
//! (directory holding `audio/` and `sentences/`). No network or microphone.
#![cfg(feature = "transcribe")]

use std::path::PathBuf;

use ne_voice::transcribe::{Language, TranscribeOptions, Transcriber};
use ne_voice::wer::word_errors;
use ne_voice::{CancelToken, SAMPLE_RATE, VoiceError};

fn spike_dir() -> PathBuf {
    std::env::var_os("NE_VOICE_SPIKE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spikes/voice"))
}

fn model_path() -> PathBuf {
    std::env::var_os("NE_VOICE_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| spike_dir().join("models/ggml-small-q5_1.bin"))
}

/// Loads the model and a clip, or explains what is missing.
fn fixture(set: &str, index: usize) -> Option<(Transcriber, Vec<f32>, String)> {
    let model = model_path();
    let wav = spike_dir().join(format!("audio/{set}-{index:02}.wav"));
    if !model.exists() || !wav.exists() {
        eprintln!("skipping: needs {} and {}", model.display(), wav.display());
        return None;
    }
    let reference = std::fs::read_to_string(spike_dir().join(format!("sentences/{set}.txt")))
        .expect("sentence fixture")
        .lines()
        .nth(index - 1)
        .expect("sentence line")
        .to_owned();
    let mut reader = hound::WavReader::open(&wav).expect("open wav");
    assert_eq!(reader.spec().sample_rate, SAMPLE_RATE);
    let pcm = reader
        .samples::<i16>()
        .map(|s| f32::from(s.expect("sample")) / 32768.0)
        .collect();
    let transcriber = Transcriber::load(&model, false).expect("load model");
    Some((transcriber, pcm, reference))
}

#[test]
#[ignore = "needs a model file and the spike clips"]
fn transcribes_english_with_a_pinned_language() {
    let Some((transcriber, pcm, reference)) = fixture("en", 2) else {
        return;
    };
    let options = TranscribeOptions {
        language: Language::English,
        ..TranscribeOptions::default()
    };
    let text = transcriber
        .transcribe(&pcm, &options, &CancelToken::new())
        .expect("transcribe");
    assert!(word_errors(&reference, &text).rate() < 0.2, "got: {text}");
}

#[test]
#[ignore = "needs a model file and the spike clips"]
fn detects_vietnamese_automatically_and_keeps_diacritics() {
    let Some((transcriber, pcm, reference)) = fixture("vi", 5) else {
        return;
    };
    let text = transcriber
        .transcribe(&pcm, &TranscribeOptions::default(), &CancelToken::new())
        .expect("transcribe");
    assert!(word_errors(&reference, &text).rate() < 0.3, "got: {text}");
}

#[test]
#[ignore = "needs a model file and the spike clips"]
fn runs_off_thread_and_returns_the_text() {
    let Some((transcriber, pcm, reference)) = fixture("en", 3) else {
        return;
    };
    let job = transcriber.spawn(
        pcm,
        TranscribeOptions {
            language: Language::English,
            initial_prompt: Some("Da Nang, Note Everywhere".into()),
            threads: Some(4),
        },
    );
    let text = job.wait().expect("job result");
    assert!(word_errors(&reference, &text).rate() < 0.2, "got: {text}");
}

#[test]
#[ignore = "needs a model file and the spike clips"]
fn a_cancelled_job_reports_cancellation() {
    let Some((transcriber, pcm, _)) = fixture("en", 4) else {
        return;
    };
    let job = transcriber.spawn(pcm, TranscribeOptions::default());
    job.cancel();
    assert!(matches!(
        job.wait(),
        Err(VoiceError::TranscriptionCancelled)
    ));
}

#[test]
#[ignore = "needs a model file"]
fn empty_audio_is_rejected() {
    let model = model_path();
    if !model.exists() {
        eprintln!("skipping: needs {}", model.display());
        return;
    }
    let transcriber = Transcriber::load(&model, false).expect("load model");
    assert!(matches!(
        transcriber.transcribe(&[], &TranscribeOptions::default(), &CancelToken::new()),
        Err(VoiceError::NoAudio)
    ));
}
