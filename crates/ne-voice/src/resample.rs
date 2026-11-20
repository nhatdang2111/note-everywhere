//! Down-mixing and sample-rate conversion to 16 kHz mono.

use std::f64::consts::PI;

/// Averages interleaved frames into one mono channel.
pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Number of sinc zero crossings on each side of the kernel centre.
const KERNEL_ZEROS: f64 = 8.0;

/// Streaming windowed-sinc resampler. Feed it chunks in order; output is
/// identical to resampling the whole signal at once. When downsampling, the
/// kernel low-passes at the output Nyquist frequency so nothing aliases.
pub struct Resampler {
    /// Input samples advanced per output sample.
    step: f64,
    /// Kernel cut-off relative to the input Nyquist frequency (<= 1).
    cutoff: f64,
    /// Kernel half-width in input samples.
    half_width: usize,
    /// Pending input; index `half_width` of the very first buffer is sample 0.
    buffer: Vec<f32>,
    /// Position of the next output sample within `buffer`.
    position: f64,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        let step = f64::from(input_rate) / f64::from(output_rate);
        let cutoff = (1.0 / step).min(1.0);
        let half_width = (KERNEL_ZEROS / cutoff).ceil() as usize;
        Self {
            step,
            cutoff,
            half_width,
            // Leading zeros stand in for the signal before time zero.
            buffer: vec![0.0; half_width],
            position: half_width as f64,
        }
    }

    /// Resamples a chunk; some output is held back until enough input arrives.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.buffer.extend_from_slice(input);
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        while self.position + self.half_width as f64 + 1.0 <= self.buffer.len() as f64 {
            out.push(self.sample_at(self.position));
            self.position += self.step;
        }
        let keep_from = (self.position.floor() as usize).saturating_sub(self.half_width);
        self.buffer.drain(..keep_from);
        self.position -= keep_from as f64;
        out
    }

    /// Emits the tail by padding with silence. The resampler can be reused for
    /// a new signal afterwards only through [`Resampler::new`].
    pub fn finish(&mut self) -> Vec<f32> {
        let padding = vec![0.0; self.half_width + 1];
        self.process(&padding)
    }

    fn sample_at(&self, centre: f64) -> f32 {
        let first = (centre - self.half_width as f64).ceil().max(0.0) as usize;
        let last = ((centre + self.half_width as f64).floor() as usize).min(self.buffer.len() - 1);
        let (mut sum, mut weight_sum) = (0.0_f64, 0.0_f64);
        for (i, &sample) in self.buffer.iter().enumerate().take(last + 1).skip(first) {
            let x = i as f64 - centre;
            let w = self.cutoff * sinc(self.cutoff * x) * hann(x / self.half_width as f64);
            sum += w * f64::from(sample);
            weight_sum += w;
        }
        // Normalising keeps the gain at DC exactly 1 whatever the phase.
        if weight_sum.abs() > 1e-9 {
            sum /= weight_sum;
        }
        sum as f32
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// Hann window over `t` in [-1, 1].
fn hann(t: f64) -> f64 {
    if t.abs() >= 1.0 {
        0.0
    } else {
        0.5 * (1.0 + (PI * t).cos())
    }
}

/// One-shot conversion of a whole signal.
pub fn resample(input: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input_rate == output_rate {
        return input.to_vec();
    }
    let mut resampler = Resampler::new(input_rate, output_rate);
    let mut out = resampler.process(input);
    out.extend(resampler.finish());
    let expected = (input.len() as f64 * f64::from(output_rate) / f64::from(input_rate)).round();
    out.truncate(expected as usize);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, seconds: f64) -> Vec<f32> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32 * 0.5)
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn to_mono_averages_channels() {
        assert_eq!(to_mono(&[1.0, 3.0, -1.0, 1.0], 2), [2.0, 0.0]);
        assert_eq!(to_mono(&[0.5, 0.25], 1), [0.5, 0.25]);
    }

    #[test]
    fn to_mono_ignores_a_trailing_partial_frame() {
        assert_eq!(to_mono(&[1.0, 1.0, 9.0], 2), [1.0]);
    }

    #[test]
    fn same_rate_is_a_copy() {
        let x = sine(440.0, 16_000, 0.1);
        assert_eq!(resample(&x, 16_000, 16_000), x);
    }

    #[test]
    fn downsampling_48k_keeps_length_pitch_and_level() {
        let input = sine(440.0, 48_000, 1.0);
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        // Ignore the edges, where the kernel sees the implicit silence.
        let mid = &out[1_000..15_000];
        let crossings = zero_crossings(mid) as f64;
        let expected = 2.0 * 440.0 * (14_000.0 / 16_000.0);
        assert!((crossings - expected).abs() < 4.0, "crossings {crossings}");
        assert!((rms(mid) - 0.5 / 2.0_f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn non_integer_ratio_44k1_is_supported() {
        let input = sine(1_000.0, 44_100, 1.0);
        let out = resample(&input, 44_100, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!((rms(&out[1_000..15_000]) - 0.5 / 2.0_f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn tone_above_the_output_nyquist_is_removed() {
        let input = sine(12_000.0, 48_000, 1.0);
        let out = resample(&input, 48_000, 16_000);
        assert!(rms(&out[1_000..15_000]) < 0.01);
    }

    #[test]
    fn upsampling_8k_doubles_the_length() {
        let input = sine(300.0, 8_000, 1.0);
        let out = resample(&input, 8_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!((rms(&out[1_000..15_000]) - 0.5 / 2.0_f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn chunked_processing_matches_one_shot() {
        let input = sine(523.0, 48_000, 0.5);
        let whole = resample(&input, 48_000, 16_000);
        let mut r = Resampler::new(48_000, 16_000);
        let mut chunked = Vec::new();
        for chunk in input.chunks(701) {
            chunked.extend(r.process(chunk));
        }
        chunked.extend(r.finish());
        chunked.truncate(whole.len());
        assert_eq!(chunked.len(), whole.len());
        for (a, b) in chunked.iter().zip(&whole) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn dc_level_is_preserved() {
        let out = resample(&vec![0.25; 4_800], 48_000, 16_000);
        assert!(out[200..1_400].iter().all(|v| (v - 0.25).abs() < 1e-4));
    }
}
