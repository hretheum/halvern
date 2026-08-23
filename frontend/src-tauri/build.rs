// Progress and diagnostics from the build script.
//
// `cargo:warning=` is the only channel cargo shows in a normal build, which is
// why every message here used to use it — including "Building for macos" and
// the whole FFmpeg download narration. A clean build printed dozens of
// warnings when nothing at all was wrong, which teaches the reader to skip
// warnings, and that is the opposite of what a warning is for.
//
// The split is by whether the reader has to do something about it:
//   note!        progress and confirmation. Goes to stderr, which cargo shows
//                under `-vv` and hides otherwise.
//   warn_build!  an anomaly worth acting on: a cache that failed verification,
//                an archive entry pointing outside its directory, a build that
//                will be materially slower than the reader expects.
macro_rules! note {
    ($($arg:tt)*) => { eprintln!($($arg)*) };
}
macro_rules! warn_build {
    ($($arg:tt)*) => { println!("cargo:warning={}", format!($($arg)*)) };
}

#[path = "build/ffmpeg.rs"]
mod ffmpeg;

fn main() {
    report_acceleration();

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=Foundation");

        // Let the enhanced_macos crate handle its own Swift compilation
        // The swift-rs crate build will be handled in the enhanced_macos crate's build.rs
    }

    // Download and bundle FFmpeg binary at build-time
    ffmpeg::ensure_ffmpeg_binary();

    tauri_build::build()
}

/// Records which acceleration backend this build is configured for, and warns
/// when the answer is "none" — the one case where the reader has a decision to make.
fn report_acceleration() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    note!("halvern build: target {}", target_os);

    match target_os.as_str() {
        "macos" => {
            note!("acceleration: Metal (macOS default)");
            #[cfg(feature = "coreml")]
            note!("acceleration: CoreML");
        }
        "windows" => {
            if cfg!(feature = "cuda") {
                note!("acceleration: CUDA");
            } else if cfg!(feature = "vulkan") {
                note!("acceleration: Vulkan");
            } else if cfg!(feature = "openblas") {
                note!("acceleration: OpenBLAS");
            } else {
                warn_build!("no GPU or BLAS acceleration: builds and transcription will be materially slower. See docs/BUILDING.md for the feature flags.");
            }
        }
        "linux" => {
            if cfg!(feature = "cuda") {
                note!("acceleration: CUDA");
            } else if cfg!(feature = "vulkan") {
                note!("acceleration: Vulkan");
            } else if cfg!(feature = "hipblas") {
                note!("acceleration: ROCm/HIP");
            } else if cfg!(feature = "openblas") {
                note!("acceleration: OpenBLAS");
            } else {
                warn_build!("no GPU or BLAS acceleration: builds and transcription will be materially slower. See docs/BUILDING.md for the feature flags.");
            }
        }
        _ => {
            warn_build!("unknown target platform {}: no acceleration configured", target_os);
        }
    }
}
