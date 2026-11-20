//! Measures word error rate and speed of a whisper model on the spike clips.
//!
//! cargo run --release -p ne-voice --features transcribe --example measure-transcription -- \
//!     --model spikes/voice/models/ggml-large-v3-turbo-q5_0.bin [--gpu] [--auto] [--threads N] [--verbose]
//!
//! Clips are `<set>-NN.wav` in `--audio`; the reference for clip NN is line NN
//! of `<set>.txt` in `--sentences`. Compiled with `--features metal` or
//! `vulkan`, `--gpu` runs the GPU backend; otherwise decoding is CPU-only.

use std::path::{Path, PathBuf};
use std::time::Instant;

use ne_voice::CancelToken;
use ne_voice::SAMPLE_RATE;
use ne_voice::transcribe::{Language, TranscribeOptions, Transcriber};
use ne_voice::wer::{WordErrors, word_errors};

const SETS: [&str; 3] = ["vi", "en", "mixed"];

struct Args {
    model: PathBuf,
    audio: PathBuf,
    sentences: PathBuf,
    gpu: bool,
    auto: bool,
    threads: Option<usize>,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        model: PathBuf::new(),
        audio: PathBuf::from("spikes/voice/audio"),
        sentences: PathBuf::from("spikes/voice/sentences"),
        gpu: false,
        auto: false,
        threads: None,
        verbose: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--model" => args.model = PathBuf::from(value("--model")?),
            "--audio" => args.audio = PathBuf::from(value("--audio")?),
            "--sentences" => args.sentences = PathBuf::from(value("--sentences")?),
            "--threads" => {
                args.threads = Some(value("--threads")?.parse().map_err(|_| "bad --threads")?)
            }
            "--gpu" => args.gpu = true,
            "--auto" => args.auto = true,
            "--verbose" => args.verbose = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if args.model.as_os_str().is_empty() {
        return Err("--model is required".into());
    }
    Ok(args)
}

fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 {
        return Err(format!("{}: expected 16 kHz mono", path.display()));
    }
    reader
        .samples::<i16>()
        .map(|s| s.map(|v| f32::from(v) / 32768.0).map_err(|e| e.to_string()))
        .collect()
}

fn language_for(set: &str, auto: bool) -> Language {
    match (auto, set) {
        (true, _) => Language::Auto,
        (false, "en") => Language::English,
        (false, _) => Language::Vietnamese,
    }
}

fn run(args: &Args) -> Result<(), String> {
    let transcriber = Transcriber::load(&args.model, args.gpu).map_err(|e| e.to_string())?;
    let cancel = CancelToken::new();
    let options = |set: &str| TranscribeOptions {
        language: language_for(set, args.auto),
        initial_prompt: None,
        threads: args.threads,
    };

    // The first run pays one-off costs (kernel compilation, page-in), so it is
    // excluded from timing.
    let warm = read_wav(&args.audio.join("en-01.wav"))?;
    transcriber
        .transcribe(&warm, &options("en"), &cancel)
        .map_err(|e| e.to_string())?;

    println!(
        "model={} backend={} language={} threads={}",
        args.model.display(),
        if args.gpu { "gpu" } else { "cpu" },
        if args.auto { "auto" } else { "pinned" },
        args.threads.map_or("all".to_owned(), |t| t.to_string()),
    );
    println!(
        "{:<6} {:>5} {:>8} {:>8} {:>6} {:>10}",
        "set", "clips", "WER %", "audio s", "RTF", "s per 10 s"
    );

    let mut probes = Vec::new();
    for set in SETS {
        let mut clips = Vec::new();
        let text = std::fs::read_to_string(args.sentences.join(format!("{set}.txt")))
            .map_err(|e| format!("{set}.txt: {e}"))?;
        let references: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let mut total = WordErrors::default();
        let (mut audio_s, mut compute_s) = (0.0_f64, 0.0_f64);
        for (i, reference) in references.iter().enumerate() {
            let pcm = read_wav(&args.audio.join(format!("{set}-{:02}.wav", i + 1)))?;
            let started = Instant::now();
            let hypothesis = transcriber
                .transcribe(&pcm, &options(set), &cancel)
                .map_err(|e| e.to_string())?;
            compute_s += started.elapsed().as_secs_f64();
            audio_s += pcm.len() as f64 / f64::from(SAMPLE_RATE);
            let errors = word_errors(reference, &hypothesis);
            if args.verbose {
                println!(
                    "  [{set}-{:02}] {:.0}% | REF: {reference}\n             HYP: {hypothesis}",
                    i + 1,
                    errors.rate() * 100.0
                );
            }
            total.add(errors);
            clips.push(pcm);
        }
        let rtf = compute_s / audio_s;
        println!(
            "{set:<6} {:>5} {:>8.1} {audio_s:>8.1} {rtf:>6.2} {:>10.2}",
            references.len(),
            total.rate() * 100.0,
            rtf * 10.0
        );
        probes.push((set, clips));
    }

    // Whisper encodes a fixed 30 s window, so latency does not scale linearly
    // with clip length. Joining consecutive clips into ~10 s utterances gives
    // the latency the gate is actually about.
    println!("\n10 s utterances (consecutive clips joined with 0.3 s gaps)");
    println!(
        "{:<6} {:>5} {:>10} {:>12} {:>10}",
        "set", "n", "audio s", "mean lat s", "max lat s"
    );
    for (set, clips) in probes {
        let utterances = join_to_ten_seconds(&clips);
        let mut latencies = Vec::new();
        for pcm in &utterances {
            let started = Instant::now();
            transcriber
                .transcribe(pcm, &options(set), &cancel)
                .map_err(|e| e.to_string())?;
            latencies.push(started.elapsed().as_secs_f64());
        }
        let n = utterances.len() as f64;
        let mean_audio =
            utterances.iter().map(Vec::len).sum::<usize>() as f64 / n / f64::from(SAMPLE_RATE);
        let mean_lat = latencies.iter().sum::<f64>() / n;
        let max_lat = latencies.iter().copied().fold(0.0, f64::max);
        println!(
            "{set:<6} {:>5} {mean_audio:>10.1} {mean_lat:>12.2} {max_lat:>10.2}",
            utterances.len()
        );
    }
    Ok(())
}

fn join_to_ten_seconds(clips: &[Vec<f32>]) -> Vec<Vec<f32>> {
    const TARGET: usize = 10 * SAMPLE_RATE as usize;
    const GAP: usize = 3 * SAMPLE_RATE as usize / 10;
    let mut out = Vec::new();
    let mut current: Vec<f32> = Vec::new();
    for clip in clips {
        if !current.is_empty() {
            current.resize(current.len() + GAP, 0.0);
        }
        current.extend_from_slice(clip);
        if current.len() >= TARGET {
            out.push(std::mem::take(&mut current));
        }
    }
    out
}

fn main() {
    let result = parse_args().and_then(|args| run(&args));
    if let Err(message) = result {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}
