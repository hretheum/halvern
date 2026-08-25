//! Bake-off harness: run every corpus transcript through the real summary path.
//!
//! Copy to `frontend/src-tauri/examples/summary_bench.rs` and run from
//! `frontend/src-tauri` with `cargo build --release --example summary_bench`.
//! It follows the shape of `examples/vad_bench.rs`.
//!
//! Not `src/bin/`: cargo discovers everything there, and the Tauri bundler
//! ships what cargo builds — which is how this harness travelled inside a
//! signed 0.1.0 disk image, 29 MB of it, before anyone noticed.
//!
//! The point of calling `generate_meeting_summary` directly, rather than
//! reimplementing the prompts, is that the prompts under test must be the
//! application's own. `generate_meeting_summary` is `pub` and takes no
//! `AppHandle` — only an `app_data_dir` — so a plain binary can drive the whole
//! three-pass pipeline (chunk summaries → structured English → translation).
//!
//! Both the English intermediate and the final document are captured. Without
//! the intermediate you cannot tell a comprehension failure from a translation
//! failure, and those have different fixes.
//!
//!   cargo run --release --bin summary_bench -- \
//!       --corpus  ../../docs/experiments/summary-model-bakeoff/corpus \
//!       --out     ../../docs/experiments/summary-model-bakeoff/results/raw/2026-08-19 \
//!       --models  qwen3.5:4b,gemma3:4b \
//!       --arm     M \
//!       --repeats 3 \
//!       --data-dir ~/Library/Application\ Support/io.halvern.app
//!
//! The target directory is the repository root, not `src-tauri` — this is a
//! cargo workspace, so the binary lands in `<repo>/target/release/`.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

use app_lib::summary::generate_meeting_summary;
use app_lib::summary::templates::get_template;
use app_lib::summary::LLMProvider;

#[derive(Debug, Deserialize)]
struct Turn {
    t: f64,
    speaker: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct CorpusFile {
    id: String,
    language: String,
    group: String,
    length_class: String,
    template_id: String,
    participants: Vec<String>,
    transcript: Vec<Turn>,
}

#[derive(Debug, Serialize)]
struct Sampling {
    temperature: f32,
    top_k: i32,
    top_p: f32,
}

#[derive(Debug, Serialize)]
struct Record {
    transcript_id: String,
    model: String,
    arm: String,
    repeat: u32,
    language: String,
    group: String,
    length_class: String,
    chunks_used: i64,
    english_intermediate: String,
    final_markdown: String,
    wall_ms: u128,
    processor_sha: String,
    sampling: Sampling,
}

/// The transcript as the app would hand it to the summarizer: one line per
/// turn, timestamp and speaker included. The `standard_meeting` template asks
/// for a "Segment Time stamp" column, so dropping the timestamps here would
/// penalise every model for something we did.
fn render(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(|t| format!("[{:.1}] {}: {}", t.t, t.speaker, t.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn processor_sha() -> String {
    std::process::Command::new("git")
        .args(["log", "-1", "--format=%h", "--", "src/summary/processor.rs"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[tokio::main]
async fn main() -> Result<()> {
    let corpus = PathBuf::from(arg("--corpus").ok_or_else(|| anyhow!("--corpus required"))?);
    let out = PathBuf::from(arg("--out").ok_or_else(|| anyhow!("--out required"))?);
    let data_dir = PathBuf::from(arg("--data-dir").ok_or_else(|| anyhow!("--data-dir required"))?);
    let models: Vec<String> = arg("--models")
        .ok_or_else(|| anyhow!("--models required"))?
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let arm = arg("--arm").unwrap_or_else(|| "M".into());
    let repeats: u32 = arg("--repeats").unwrap_or_else(|| "3".into()).parse()?;

    std::fs::create_dir_all(&out)?;
    let sha = processor_sha();
    let client = reqwest::Client::new();

    // Arm M matches the sampler across families so the comparison is between
    // models rather than between presets; Arm S lets each model's own preset
    // apply. See 01-design.md.
    //
    // These arguments do NOT reach the local models — `generate_with_builtin`
    // takes no sampling parameters, so for BuiltInAI they are inert and only
    // the env var below has any effect. They are still passed because the same
    // function serves the cloud providers, where they do apply.
    let (temperature, top_p) = match arm.as_str() {
        "M" => (Some(0.0_f32), Some(1.0_f32)),
        "S" => (None, None),
        other => return Err(anyhow!("unknown arm {other:?}, expected M or S")),
    };
    if arm == "M" {
        std::env::set_var("HALVERN_BENCH_GREEDY", "1");
    } else {
        std::env::remove_var("HALVERN_BENCH_GREEDY");
    }

    let mut files: Vec<PathBuf> = walk(&corpus)?;
    files.sort();
    println!(
        "{} transcript(s), {} model(s), arm {arm}, {repeats} repeat(s)",
        files.len(),
        models.len(),
    );

    for path in &files {
        let doc: CorpusFile = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        let text = render(&doc.transcript);
        let template = get_template(&doc.template_id).map_err(|e| anyhow!(e))?;

        for model in &models {
            for repeat in 1..=repeats {
                let started = Instant::now();

                // `summary_language` is the transcript's own language, which is
                // what the app does when the user pins a language or the
                // meeting is detected as non-English. This is what triggers the
                // translation pass being measured by M1.
                let result = generate_meeting_summary(
                    &client,
                    &LLMProvider::BuiltInAI,
                    model,
                    "", // api_key: unused for BuiltInAI
                    &text,
                    "", // custom_prompt
                    &doc.template_id,
                    &template,
                    4000, // token_threshold, the app default
                    None, // ollama_endpoint
                    None, // custom_openai_endpoint
                    None, // max_tokens
                    temperature,
                    top_p,
                    Some(&data_dir),
                    None,                // cancellation_token
                    Some(&doc.language), // summary_language
                    Some(&doc.language), // detected_transcript_language
                    None,                // cached_english
                    None,                // agenda
                    &doc.participants,
                )
                .await;

                let wall_ms = started.elapsed().as_millis();

                match result {
                    Ok((final_markdown, english_intermediate, chunks_used)) => {
                        let rec = Record {
                            transcript_id: doc.id.clone(),
                            model: model.clone(),
                            arm: arm.clone(),
                            repeat,
                            language: doc.language.clone(),
                            group: doc.group.clone(),
                            length_class: doc.length_class.clone(),
                            chunks_used,
                            english_intermediate,
                            final_markdown,
                            wall_ms,
                            processor_sha: sha.clone(),
                            sampling: Sampling {
                                temperature: temperature.unwrap_or(-1.0),
                                top_k: if arm == "M" { 1 } else { -1 },
                                top_p: top_p.unwrap_or(-1.0),
                            },
                        };
                        let name = format!(
                            "{}__{}__{}__{}.json",
                            doc.id,
                            model.replace(':', "_"),
                            arm,
                            repeat
                        );
                        std::fs::write(out.join(name), serde_json::to_string_pretty(&rec)?)?;
                        println!("  ok   {} {} r{repeat} ({wall_ms} ms)", doc.id, model);
                    }
                    Err(e) => {
                        // A failure is data, not a reason to stop: a model that
                        // cannot finish a Japanese transcript has told you
                        // something. Record and continue.
                        eprintln!("  FAIL {} {} r{repeat}: {e}", doc.id, model);
                        let name = format!(
                            "{}__{}__{}__{}.error.txt",
                            doc.id,
                            model.replace(':', "_"),
                            arm,
                            repeat
                        );
                        std::fs::write(out.join(name), format!("{e}"))?;
                    }
                }
            }
        }
    }

    println!("\nwrote records to {}", out.display());
    Ok(())
}

fn walk(dir: &PathBuf) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            // `_control` holds real meetings. They are run like everything
            // else; the scorer is what keeps them away from the judge.
            out.extend(walk(&p)?);
        } else if p.extension().is_some_and(|e| e == "json") {
            out.push(p);
        }
    }
    Ok(out)
}
