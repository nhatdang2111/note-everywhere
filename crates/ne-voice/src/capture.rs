//! Microphone capture to 16 kHz mono PCM, shaped for hold-to-talk: call
//! [`Recording::start`] on key down and [`Recording::stop`] on key up.
//!
//! cpal streams are not `Send` on every platform, so each recording owns a
//! small thread that creates, runs and drops the stream. That keeps
//! [`Recording`] `Send` and lets the caller start and stop from any thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, SampleFormat, SizedSample};

use crate::SAMPLE_RATE;
use crate::error::VoiceError;
use crate::resample::{Resampler, to_mono};

/// Default cap on one recording.
pub const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(120);

/// An audio input the user can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    /// Stable identifier to pass back in [`CaptureConfig::device_id`].
    pub id: String,
    /// Human-readable name for a settings list.
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Device to record from; `None` uses the system default input.
    pub device_id: Option<String>,
    /// Recording stops accepting audio after this long.
    pub max_duration: Duration,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            max_duration: DEFAULT_MAX_DURATION,
        }
    }
}

/// Finished recording: 16 kHz mono samples in [-1, 1].
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    pub samples: Vec<f32>,
    /// True when the maximum length was reached and later audio was dropped.
    pub truncated: bool,
}

impl Recorded {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(SAMPLE_RATE))
    }
}

/// Converts device audio to bounded 16 kHz mono. Separate from cpal so the
/// conversion and the length bound are testable without hardware.
pub struct CaptureBuffer {
    channels: usize,
    input_rate: u32,
    /// Mono frames received so far; fixes the exact output length at the end.
    input_frames: u64,
    resampler: Option<Resampler>,
    samples: Vec<f32>,
    max_samples: usize,
    truncated: bool,
}

impl CaptureBuffer {
    pub fn new(input_rate: u32, channels: u16, max_duration: Duration) -> Self {
        let max_samples = (max_duration.as_secs_f64() * f64::from(SAMPLE_RATE)) as usize;
        Self {
            channels: usize::from(channels.max(1)),
            input_rate,
            input_frames: 0,
            resampler: (input_rate != SAMPLE_RATE).then(|| Resampler::new(input_rate, SAMPLE_RATE)),
            samples: Vec::new(),
            max_samples,
            truncated: false,
        }
    }

    /// Adds interleaved device samples; input past the maximum is discarded.
    pub fn push(&mut self, interleaved: &[f32]) {
        if self.is_full() {
            return;
        }
        let mono = to_mono(interleaved, self.channels);
        self.input_frames += mono.len() as u64;
        match &mut self.resampler {
            Some(resampler) => self.samples.extend(resampler.process(&mono)),
            None => self.samples.extend(mono),
        }
        self.clamp_to_max();
    }

    pub fn is_full(&self) -> bool {
        self.truncated || self.samples.len() >= self.max_samples
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Flushes the resampler tail and returns everything captured.
    pub fn finish(mut self) -> Recorded {
        if !self.is_full() {
            self.flush_resampler();
        }
        self.clamp_to_max();
        Recorded {
            samples: self.samples,
            truncated: self.truncated,
        }
    }

    fn flush_resampler(&mut self) {
        let Some(resampler) = &mut self.resampler else {
            return;
        };
        self.samples.extend(resampler.finish());
        // The flush pads with silence; keep only what the input covers.
        let exact = self.input_frames as f64 * f64::from(SAMPLE_RATE) / f64::from(self.input_rate);
        self.samples.truncate(exact.round() as usize);
    }

    fn clamp_to_max(&mut self) {
        if self.samples.len() > self.max_samples {
            self.samples.truncate(self.max_samples);
            self.truncated = true;
        }
    }
}

struct Shared {
    buffer: Option<CaptureBuffer>,
    stream_error: Option<String>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An active recording. Dropping it without calling [`Recording::stop`]
/// discards the audio and releases the microphone.
pub struct Recording {
    shared: Arc<Mutex<Shared>>,
    full: Arc<AtomicBool>,
    stop_tx: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl Recording {
    /// Opens the input device and starts capturing. Fails synchronously when
    /// the device is missing or the OS denies microphone access.
    pub fn start(config: &CaptureConfig) -> Result<Self, VoiceError> {
        let shared = Arc::new(Mutex::new(Shared {
            buffer: None,
            stream_error: None,
        }));
        let full = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), VoiceError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let worker_shared = Arc::clone(&shared);
        let worker_full = Arc::clone(&full);
        let device_id = config.device_id.clone();
        let max_duration = config.max_duration;
        let worker = std::thread::Builder::new()
            .name("ne-voice-capture".into())
            .spawn(move || {
                match open_stream(
                    device_id.as_deref(),
                    max_duration,
                    &worker_shared,
                    &worker_full,
                ) {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        // Returns when stop() sends or the Recording is dropped.
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|e| VoiceError::Audio(format!("cannot start capture thread: {e}")))?;

        let recording = Self {
            shared,
            full,
            stop_tx: Some(stop_tx),
            worker: Some(worker),
        };
        ready_rx
            .recv()
            .map_err(|_| VoiceError::Audio("capture thread ended unexpectedly".into()))??;
        Ok(recording)
    }

    /// True once the maximum length is reached; the caller may stop early.
    pub fn is_full(&self) -> bool {
        self.full.load(Ordering::Relaxed)
    }

    /// Audio captured so far.
    pub fn elapsed(&self) -> Duration {
        let len = lock(&self.shared)
            .buffer
            .as_ref()
            .map_or(0, CaptureBuffer::len);
        Duration::from_secs_f64(len as f64 / f64::from(SAMPLE_RATE))
    }

    /// Stops capturing and returns the audio. Errors if the device failed and
    /// nothing was captured.
    pub fn stop(mut self) -> Result<Recorded, VoiceError> {
        self.shutdown();
        let mut shared = lock(&self.shared);
        let recorded = shared
            .buffer
            .take()
            .ok_or(VoiceError::NotRecording)?
            .finish();
        match shared.stream_error.take() {
            Some(error) if recorded.samples.is_empty() => Err(VoiceError::Audio(error)),
            _ => Ok(recorded),
        }
    }

    fn shutdown(&mut self) {
        // Dropping the sender wakes the worker, which drops the stream.
        self.stop_tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Lists the available input devices.
pub fn list_input_devices() -> Result<Vec<InputDevice>, VoiceError> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host.input_devices().map_err(map_cpal_error)?;
    Ok(devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device
                .description()
                .map_or_else(|_| id.clone(), |d| d.name().to_owned());
            Some(InputDevice {
                is_default: default_id.as_deref() == Some(id.as_str()),
                id,
                name,
            })
        })
        .collect())
}

fn map_cpal_error(error: cpal::Error) -> VoiceError {
    match error.kind() {
        cpal::ErrorKind::PermissionDenied => VoiceError::PermissionDenied,
        _ => VoiceError::Audio(error.to_string()),
    }
}

fn open_stream(
    device_id: Option<&str>,
    max_duration: Duration,
    shared: &Arc<Mutex<Shared>>,
    full: &Arc<AtomicBool>,
) -> Result<cpal::Stream, VoiceError> {
    let host = cpal::default_host();
    let device = match device_id {
        Some(id) => {
            let parsed: DeviceId = id
                .parse()
                .map_err(|_| VoiceError::DeviceNotFound(id.to_owned()))?;
            host.device_by_id(&parsed)
                .filter(|d| d.supports_input())
                .ok_or_else(|| VoiceError::DeviceNotFound(id.to_owned()))?
        }
        None => host
            .default_input_device()
            .ok_or(VoiceError::NoInputDevice)?,
    };
    let supported = device.default_input_config().map_err(map_cpal_error)?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    lock(shared).buffer = Some(CaptureBuffer::new(
        config.sample_rate,
        config.channels,
        max_duration,
    ));

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, shared, full),
        SampleFormat::F64 => build::<f64>(&device, config, shared, full),
        SampleFormat::I8 => build::<i8>(&device, config, shared, full),
        SampleFormat::I16 => build::<i16>(&device, config, shared, full),
        SampleFormat::I32 => build::<i32>(&device, config, shared, full),
        SampleFormat::U8 => build::<u8>(&device, config, shared, full),
        SampleFormat::U16 => build::<u16>(&device, config, shared, full),
        SampleFormat::U32 => build::<u32>(&device, config, shared, full),
        other => {
            return Err(VoiceError::Audio(format!(
                "unsupported sample format: {other}"
            )));
        }
    }
    .map_err(map_cpal_error)?;
    stream.play().map_err(map_cpal_error)?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: &Arc<Mutex<Shared>>,
    full: &Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let data_shared = Arc::clone(shared);
    let error_shared = Arc::clone(shared);
    let full = Arc::clone(full);
    // Reused between callbacks to keep allocation off the audio thread.
    let mut scratch: Vec<f32> = Vec::new();
    device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            scratch.clear();
            scratch.extend(data.iter().map(|&s| s.to_sample::<f32>()));
            if let Some(buffer) = lock(&data_shared).buffer.as_mut() {
                buffer.push(&scratch);
                if buffer.is_full() {
                    full.store(true, Ordering::Relaxed);
                }
            }
        },
        move |error: cpal::Error| {
            // Overruns and re-routes are recoverable; anything else is kept
            // so stop() can report it if no audio arrived.
            if !matches!(
                error.kind(),
                cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged
            ) {
                lock(&error_shared).stream_error = Some(error.to_string());
            }
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    #[test]
    fn mono_16k_passes_through_unchanged() {
        let mut buffer = CaptureBuffer::new(16_000, 1, secs(10.0));
        buffer.push(&[0.1, 0.2, 0.3]);
        let recorded = buffer.finish();
        assert_eq!(recorded.samples, [0.1, 0.2, 0.3]);
        assert!(!recorded.truncated);
    }

    #[test]
    fn stereo_48k_becomes_mono_16k() {
        let mut buffer = CaptureBuffer::new(48_000, 2, secs(10.0));
        // One second of stereo DC: left 0.4, right 0.2 -> mono 0.3.
        let frames: Vec<f32> = (0..48_000).flat_map(|_| [0.4, 0.2]).collect();
        buffer.push(&frames);
        let recorded = buffer.finish();
        assert_eq!(recorded.samples.len(), 16_000);
        assert!(
            recorded.samples[500..15_000]
                .iter()
                .all(|v| (v - 0.3).abs() < 1e-3)
        );
        assert!((recorded.duration().as_secs_f64() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recording_is_bounded_and_flags_truncation() {
        let mut buffer = CaptureBuffer::new(16_000, 1, secs(1.0));
        buffer.push(&vec![0.5; 12_000]);
        assert!(!buffer.is_full());
        buffer.push(&vec![0.5; 12_000]);
        assert!(buffer.is_full());
        buffer.push(&vec![0.5; 12_000]);
        let recorded = buffer.finish();
        assert_eq!(recorded.samples.len(), 16_000);
        assert!(recorded.truncated);
    }

    #[test]
    fn bound_applies_after_resampling() {
        let mut buffer = CaptureBuffer::new(48_000, 1, secs(0.5));
        buffer.push(&vec![0.1; 48_000]);
        let recorded = buffer.finish();
        assert_eq!(recorded.samples.len(), 8_000);
        assert!(recorded.truncated);
    }

    #[test]
    fn chunk_boundaries_do_not_change_the_result() {
        let signal: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();
        let mut whole = CaptureBuffer::new(48_000, 1, secs(10.0));
        whole.push(&signal);
        let mut chunked = CaptureBuffer::new(48_000, 1, secs(10.0));
        for chunk in signal.chunks(480) {
            chunked.push(chunk);
        }
        let (a, b) = (whole.finish(), chunked.finish());
        assert_eq!(a.samples.len(), b.samples.len());
        assert!(
            a.samples
                .iter()
                .zip(&b.samples)
                .all(|(x, y)| (x - y).abs() < 1e-5)
        );
    }

    #[test]
    fn empty_buffer_reports_empty() {
        let buffer = CaptureBuffer::new(44_100, 2, secs(1.0));
        assert!(buffer.is_empty());
        assert!(buffer.finish().samples.is_empty());
    }

    #[test]
    fn default_config_uses_the_system_device_and_a_bounded_length() {
        let config = CaptureConfig::default();
        assert!(config.device_id.is_none());
        assert_eq!(config.max_duration, DEFAULT_MAX_DURATION);
    }
}
