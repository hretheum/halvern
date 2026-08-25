//! Offline bench for voice activity detection and transcription.
//!
//! Runs a recorded meeting through the same VAD and Parakeet path the live
//! pipeline uses, with the detection thresholds supplied on the command line.
//! The point is to turn threshold tuning into a table instead of a discussion:
//! every experiment currently costs a real meeting, and guessing under that
//! cost is how wrong theories get shipped.
//!
//! Usage:
//!   cargo run --offline --features metal --bin vad-bench -- <plik> [opcje]
//!
//! Options:
//!   --speech <f32>       positive speech threshold      (default 0.50)
//!   --silence <f32>      negative speech threshold      (default 0.35)
//!   --redemption <ms>    pause bridged before closing   (default 2000)
//!   --min-speech <ms>    shortest kept segment          (default 250)
//!   --pre-pad <ms>       audio kept before onset        (default 300)
//!   --post-pad <ms>      audio kept after offset        (default 400)
//!   --preset pro         Meetily Pro's vad_config.json values
//!   --sweep              run a grid of thresholds and print a comparison table
//!   --no-transcribe      VAD only, skip Parakeet (fast)
//!   --model <name>       Parakeet model (default parakeet-tdt-0.6b-v3-int8)
//!   --quiet              suppress per-segment output
//!
//! Examples:
//!   cargo run --bin vad-bench -- audio.mp4 --preset pro
//!   cargo run --bin vad-bench -- audio.mp4 --sweep --no-transcribe

use std::path::{Path, PathBuf};

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::vad::{ContinuousVadProcessor, VadTuning};
use app_lib::parakeet_engine::ParakeetEngine;

const DEFAULT_MODEL: &str = "parakeet-tdt-0.6b-v3-int8";

struct Options {
    file: PathBuf,
    tuning: VadTuning,
    transcribe: bool,
    sweep: bool,
    quiet: bool,
    model: String,
    /// Overrides where Parakeet weights are looked up, so quantised and
    /// full-precision variants can be compared on identical audio.
    models_dir: Option<PathBuf>,
}

/// One measured run: what the thresholds produced.
struct Outcome {
    tuning: VadTuning,
    segments: usize,
    speech_seconds: f64,
    texts: Vec<String>,
}

impl Outcome {
    fn characters(&self) -> usize {
        self.texts.iter().map(|t| t.chars().count()).sum()
    }
}

fn print_usage() {
    eprintln!(
        "\
Usage: vad_bench <audio file> [options]

  --speech <f32>      speech threshold                (default 0.50)
  --silence <f32>     silence threshold               (default 0.35)
  --redemption <ms>   bridged gap                     (default 2000)
  --min-speech <ms>   shortest segment kept           (default 250)
  --pre-pad <ms>      audio before speech starts      (default 300)
  --post-pad <ms>     audio after speech ends         (default 400)
  --preset pro        values from Meetily Pro's vad_config.json
  --preset default    the values this build ships with
  --sweep             threshold grid and comparison table
  --no-transcribe     sam VAD, bez Parakeeta (szybko)
  --model <nazwa>     Parakeet model (default {})
  --quiet             do not print segments

Examples:
  vad_bench audio.mp4 --preset pro
  vad_bench audio.mp4 --sweep --no-transcribe",
        DEFAULT_MODEL
    );
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);

    let file = args
        .next()
        .ok_or_else(|| "give a path to an audio file".to_string())?;

    if file == "--help" || file == "-h" {
        return Err("help".to_string());
    }

    let mut opts = Options {
        file: PathBuf::from(file),
        tuning: VadTuning::default(),
        transcribe: true,
        sweep: false,
        quiet: false,
        model: DEFAULT_MODEL.to_string(),
        models_dir: None,
    };

    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("missing value after {}", flag))
        };

        match flag.as_str() {
            "--speech" => opts.tuning.positive_speech_threshold = parse_num(&value()?, &flag)?,
            "--silence" => opts.tuning.negative_speech_threshold = parse_num(&value()?, &flag)?,
            "--redemption" => opts.tuning.redemption_time_ms = parse_num(&value()?, &flag)?,
            "--min-speech" => opts.tuning.min_speech_time_ms = parse_num(&value()?, &flag)?,
            "--pre-pad" => opts.tuning.pre_speech_pad_ms = parse_num(&value()?, &flag)?,
            "--post-pad" => opts.tuning.post_speech_pad_ms = parse_num(&value()?, &flag)?,
            "--model" => opts.model = value()?,
            "--models-dir" => opts.models_dir = Some(PathBuf::from(value()?)),
            "--preset" => match value()?.as_str() {
                "pro" => opts.tuning = VadTuning::pro_reference(),
                "default" | "ce" => opts.tuning = VadTuning::default(),
                other => return Err(format!("unknown preset '{}', available: pro, default", other)),
            },
            "--sweep" => opts.sweep = true,
            "--no-transcribe" => opts.transcribe = false,
            "--quiet" => opts.quiet = true,
            other => return Err(format!("nieznana opcja '{}'", other)),
        }
    }

    Ok(opts)
}

fn parse_num<T: std::str::FromStr>(raw: &str, flag: &str) -> Result<T, String> {
    raw.parse()
        .map_err(|_| format!("cannot parse value '{}' for {}", raw, flag))
}

/// Decodes to the 16 kHz mono form both VAD and Parakeet expect.
fn load_samples(path: &Path) -> Result<(Vec<f32>, f64), String> {
    if !path.exists() {
        return Err(format!("no such file: {}", path.display()));
    }

    let decoded = decode_audio_file(path).map_err(|e| format!("decoding failed: {}", e))?;
    let duration = decoded.duration_seconds;
    let samples = decoded.to_whisper_format();

    if samples.is_empty() {
        return Err("the file decoded to zero samples".to_string());
    }

    Ok((samples, duration))
}

/// Runs one set of thresholds over already-decoded audio.
///
/// Feeds the samples in one-second slices so the processor exercises the same
/// streaming path as the live pipeline rather than a single giant buffer.
async fn run_once(
    samples: &[f32],
    tuning: VadTuning,
    engine: Option<&ParakeetEngine>,
) -> Result<Outcome, String> {
    let mut processor = ContinuousVadProcessor::with_tuning(16_000, tuning.clone())
        .map_err(|e| format!("failed to create the VAD processor: {}", e))?;

    let mut segments = Vec::new();

    for slice in samples.chunks(16_000) {
        let found = processor
            .process_audio(slice)
            .map_err(|e| format!("the VAD returned an error: {}", e))?;
        segments.extend(found);
    }

    segments.extend(
        processor
            .flush()
            .map_err(|e| format!("flushing the VAD failed: {}", e))?,
    );

    let speech_seconds = segments
        .iter()
        .map(|s| s.samples.len() as f64 / 16_000.0)
        .sum();

    let mut texts = Vec::new();
    if let Some(engine) = engine {
        for segment in &segments {
            match engine.transcribe_audio(segment.samples.clone()).await {
                Ok(text) => {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        texts.push(trimmed);
                    }
                }
                // A failed segment must not abort the run; the count still matters.
                Err(e) => eprintln!("  ! transcribing the segment failed: {}", e),
            }
        }
    }

    Ok(Outcome {
        tuning,
        segments: segments.len(),
        speech_seconds,
        texts,
    })
}

async fn load_engine(model: &str, override_dir: Option<&Path>) -> Result<ParakeetEngine, String> {
    let models_dir = match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => dirs::data_dir()
            .map(|d| d.join("io.halvern.app").join("models"))
            .ok_or_else(|| "could not find the application data directory".to_string())?,
    };

    if !models_dir.join("parakeet").join(model).exists() {
        return Err(format!(
            "model '{}' missing from {}. Run the app once so it downloads, or pass --model",
            model,
            models_dir.join("parakeet").display()
        ));
    }

    let engine = ParakeetEngine::new_with_models_dir(Some(models_dir))
        .map_err(|e| format!("failed to create the Parakeet engine: {}", e))?;

    // `load_model` looks the name up in the table that discovery fills in;
    // without this it reports "Model not found" even with the files present.
    engine
        .discover_models()
        .await
        .map_err(|e| format!("searching for models failed: {}", e))?;

    engine
        .load_model(model)
        .await
        .map_err(|e| format!("failed to load model '{}': {}", model, e))?;

    Ok(engine)
}

/// Threshold grid for `--sweep`, spanning this build's defaults through Pro's.
fn sweep_grid() -> Vec<VadTuning> {
    let mut grid = Vec::new();
    for (speech, silence) in [(0.50, 0.35), (0.40, 0.30), (0.35, 0.25), (0.30, 0.20), (0.25, 0.15)] {
        for redemption in [2000, 1000, 500] {
            grid.push(VadTuning {
                positive_speech_threshold: speech,
                negative_speech_threshold: silence,
                redemption_time_ms: redemption,
                ..VadTuning::default()
            });
        }
    }
    grid
}

#[tokio::main]
async fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            if e != "help" {
                eprintln!("Error: {}\n", e);
            }
            print_usage();
            std::process::exit(if e == "help" { 0 } else { 2 });
        }
    };

    let (samples, duration) = match load_samples(&opts.file) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("file:      {}", opts.file.display());
    println!(
        "material:  {:.1} s, {} samples after conversion to 16 kHz mono\n",
        duration,
        samples.len()
    );

    let engine = if opts.transcribe {
        match load_engine(&opts.model, opts.models_dir.as_deref()).await {
            Ok(e) => Some(e),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let runs = if opts.sweep {
        sweep_grid()
    } else {
        vec![opts.tuning.clone()]
    };

    let mut outcomes = Vec::new();

    for tuning in runs {
        let outcome = match run_once(&samples, tuning, engine.as_ref()).await {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        };

        if !opts.sweep && !opts.quiet {
            report_single(&outcome, duration);
        }

        outcomes.push(outcome);
    }

    if opts.sweep {
        report_sweep(&outcomes, duration);
    }
}

fn report_single(outcome: &Outcome, duration: f64) {
    let t = &outcome.tuning;
    println!("progi:     mowa {:.2}, cisza {:.2}, wybaczanie {} ms, min {} ms",
        t.positive_speech_threshold, t.negative_speech_threshold,
        t.redemption_time_ms, t.min_speech_time_ms);
    println!("segmenty:  {}", outcome.segments);
    println!(
        "speech:    {:.1} s of {:.1} s material ({:.0}%)",
        outcome.speech_seconds,
        duration,
        if duration > 0.0 { outcome.speech_seconds / duration * 100.0 } else { 0.0 }
    );
    println!("znaki:     {}\n", outcome.characters());

    for (i, text) in outcome.texts.iter().enumerate() {
        println!("  [{:>3}] {}", i, text);
    }
}

fn report_sweep(outcomes: &[Outcome], duration: f64) {
    println!("{:>6} {:>7} {:>11} {:>9} {:>9} {:>7}", "mowa", "cisza", "wybaczanie", "segmenty", "sekundy", "znaki");
    println!("{}", "-".repeat(56));

    for o in outcomes {
        println!(
            "{:>6.2} {:>7.2} {:>9} ms {:>9} {:>8.1}s {:>7}",
            o.tuning.positive_speech_threshold,
            o.tuning.negative_speech_threshold,
            o.tuning.redemption_time_ms,
            o.segments,
            o.speech_seconds,
            o.characters()
        );
    }

    println!("\nmaterial: {:.1} s", duration);
    println!("More segments is not better — look for the threshold where the character");
    println!("count grows, not the segment count. Over-splitting shows up as many segments");
    println!("with few characters.");
}
