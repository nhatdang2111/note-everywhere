//! Silence trimming with a short-time energy detector.
//!
//! This is deliberately simple: it finds where speech starts and ends so the
//! model does not decode long stretches of silence, and so a recording of pure
//! silence is rejected before it reaches the model. It is not a speech/noise
//! classifier; loud non-speech sound counts as speech.

use std::ops::Range;

use crate::SAMPLE_RATE;

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    /// Analysis frame length in milliseconds.
    pub frame_ms: u32,
    /// A frame is speech when it is this many dB above the estimated noise floor.
    pub margin_db: f32,
    /// The detection threshold never goes below this level (dBFS)...
    pub min_threshold_db: f32,
    /// ...or above this one, so continuous loud speech is still detected.
    pub max_threshold_db: f32,
    /// Consecutive speech frames needed to count as speech; rejects clicks.
    pub min_speech_ms: u32,
    /// Audio kept before the first and after the last speech frame.
    pub padding_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            margin_db: 12.0,
            min_threshold_db: -55.0,
            max_threshold_db: -35.0,
            min_speech_ms: 80,
            padding_ms: 200,
        }
    }
}

/// Level of one frame in dBFS (full scale = 1.0 RMS), floored at -120.
fn frame_db(frame: &[f32]) -> f32 {
    let mean_square = frame.iter().map(|&s| s * s).sum::<f32>() / frame.len().max(1) as f32;
    (10.0 * mean_square.max(1e-12).log10()).max(-120.0)
}

/// Returns the sample range holding speech plus padding, or `None` when the
/// recording contains no speech.
pub fn speech_range(pcm: &[f32], config: &VadConfig) -> Option<Range<usize>> {
    let frame_len = (SAMPLE_RATE * config.frame_ms / 1000).max(1) as usize;
    let levels: Vec<f32> = pcm.chunks(frame_len).map(frame_db).collect();
    if levels.is_empty() {
        return None;
    }

    // The quietest tenth of frames approximates the background noise.
    let mut sorted = levels.clone();
    sorted.sort_by(f32::total_cmp);
    let noise_floor = sorted[sorted.len() / 10];
    let threshold =
        (noise_floor + config.margin_db).clamp(config.min_threshold_db, config.max_threshold_db);

    let min_run = (config.min_speech_ms / config.frame_ms).max(1) as usize;
    let (mut first, mut last) = (None, None);
    let mut run = 0;
    for (i, &level) in levels.iter().enumerate() {
        if level > threshold {
            run += 1;
            if run >= min_run {
                first.get_or_insert(i + 1 - min_run);
                last = Some(i);
            }
        } else {
            run = 0;
        }
    }

    let (first, last) = (first?, last?);
    let padding = (SAMPLE_RATE * config.padding_ms / 1000) as usize;
    let start = (first * frame_len).saturating_sub(padding);
    let end = ((last + 1) * frame_len + padding).min(pcm.len());
    Some(start..end)
}

/// The part of `pcm` containing speech, or an empty slice when there is none.
pub fn trim_silence<'a>(pcm: &'a [f32], config: &VadConfig) -> &'a [f32] {
    speech_range(pcm, config).map_or(&[], |range| &pcm[range])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const RATE: usize = SAMPLE_RATE as usize;

    /// Deterministic low-level noise (xorshift), amplitude `amp`.
    fn noise(len: usize, amp: f32, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 - 0.5) * 2.0 * amp
            })
            .collect()
    }

    fn tone(len: usize, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| amp * (2.0 * PI * 220.0 * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn clip(lead_s: f32, speech_s: f32, tail_s: f32, noise_amp: f32) -> Vec<f32> {
        let n = |s: f32| (s * RATE as f32) as usize;
        let mut pcm = noise(n(lead_s), noise_amp, 1);
        pcm.extend(
            tone(n(speech_s), 0.3)
                .iter()
                .zip(noise(n(speech_s), noise_amp, 2))
                .map(|(a, b)| a + b),
        );
        pcm.extend(noise(n(tail_s), noise_amp, 3));
        pcm
    }

    #[test]
    fn pure_silence_has_no_speech() {
        assert!(speech_range(&vec![0.0; RATE], &VadConfig::default()).is_none());
        assert!(trim_silence(&noise(RATE, 0.001, 7), &VadConfig::default()).is_empty());
    }

    #[test]
    fn empty_input_has_no_speech() {
        assert!(speech_range(&[], &VadConfig::default()).is_none());
    }

    #[test]
    fn trims_leading_and_trailing_silence_keeping_padding() {
        let pcm = clip(1.5, 1.0, 1.5, 0.002);
        let range = speech_range(&pcm, &VadConfig::default()).expect("speech");
        let start_s = range.start as f32 / RATE as f32;
        let end_s = range.end as f32 / RATE as f32;
        // Speech runs 1.5 s..2.5 s; padding is 0.2 s each side.
        assert!((start_s - 1.3).abs() < 0.05, "start {start_s}");
        assert!((end_s - 2.7).abs() < 0.05, "end {end_s}");
    }

    #[test]
    fn works_with_a_noisy_background() {
        let pcm = clip(1.0, 1.0, 1.0, 0.01);
        let trimmed = trim_silence(&pcm, &VadConfig::default());
        let seconds = trimmed.len() as f32 / RATE as f32;
        assert!((1.2..1.7).contains(&seconds), "kept {seconds}s");
    }

    #[test]
    fn continuous_speech_is_kept_whole() {
        let pcm: Vec<f32> = tone(RATE * 2, 0.3);
        let range = speech_range(&pcm, &VadConfig::default()).expect("speech");
        assert_eq!(range, 0..pcm.len());
    }

    #[test]
    fn a_click_shorter_than_the_minimum_is_ignored() {
        let mut pcm = vec![0.0; RATE];
        for s in &mut pcm[8_000..8_100] {
            *s = 0.9;
        }
        assert!(speech_range(&pcm, &VadConfig::default()).is_none());
    }

    #[test]
    fn padding_never_leaves_the_buffer() {
        let pcm = clip(0.05, 0.5, 0.05, 0.001);
        let range = speech_range(&pcm, &VadConfig::default()).expect("speech");
        assert_eq!(range.start, 0);
        assert_eq!(range.end, pcm.len());
    }
}
