//! Does a long-running process see a microphone that appears after it started?
//!
//! Written to settle one question and nothing else: when the system's default
//! input changes while the app is running — AirPods connect, and macOS makes
//! them the default — does `cpal` report the change, or does the process go on
//! believing what was true at launch?
//!
//! The app's own code does not cache: `list_audio_devices` builds a fresh
//! `cpal::default_host()` on every call, and `default_input_device` does the
//! same. So if this loop also goes stale, the staleness is below us, in the
//! CoreAudio HAL client, and no amount of re-querying from our side will fix
//! it. If this loop follows the change, the fault is ours and lives further up.
//!
//! Run it, then connect or disconnect a Bluetooth headset:
//!
//! ```sh
//! cd frontend/src-tauri && cargo run --example device_watch
//! ```
//!
//! Every line is a poll. Only changes are printed, so silence after connecting
//! AirPods *is* the finding.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};

fn snapshot() -> (String, String, BTreeSet<String>, BTreeSet<String>) {
    // A fresh host every poll, exactly as the app does it.
    let host = cpal::default_host();

    let default_in = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "<none>".to_string());
    let default_out = host
        .default_output_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "<none>".to_string());

    let inputs = host
        .input_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    let outputs = host
        .output_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();

    (default_in, default_out, inputs, outputs)
}

fn main() {
    let started = Instant::now();
    println!("Polling every 2s. Connect or disconnect a headset and watch.");
    println!("Only changes print — silence means the process did not notice.\n");

    let mut previous: Option<(String, String, BTreeSet<String>, BTreeSet<String>)> = None;

    loop {
        let current = snapshot();
        let changed = previous.as_ref() != Some(&current);

        if changed {
            let (default_in, default_out, inputs, outputs) = &current;
            println!("[{:>6.1}s] CHANGE", started.elapsed().as_secs_f32());
            println!("  default input : {default_in}");
            println!("  default output: {default_out}");
            println!("  inputs  ({}): {}", inputs.len(), join(inputs));
            println!("  outputs ({}): {}", outputs.len(), join(outputs));
            println!();
            previous = Some(current);
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

fn join(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(", ")
}
