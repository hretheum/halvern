// Retranscription module - allows re-processing stored audio with different settings

use crate::audio::decoder::decode_audio_file;
use crate::audio::transcription::TranscriptionEngine;
use crate::audio::vad::get_speech_chunks_with_progress;
use crate::database::repositories::setting::SettingsRepository;
use super::common::{
    create_transcript_segments_with_sources, split_segment_at_silence, write_transcripts_json,
};
use super::constants::AUDIO_EXTENSIONS;
use crate::config::{DEFAULT_WHISPER_MODEL, DEFAULT_PARAKEET_MODEL};
use crate::parakeet_engine::ParakeetEngine;
use crate::state::AppState;
use crate::whisper_engine::WhisperEngine;
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Global flag to track if retranscription is in progress
static RETRANSCRIPTION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Global flag to signal cancellation
static RETRANSCRIPTION_CANCELLED: AtomicBool = AtomicBool::new(false);

/// RAII guard for RETRANSCRIPTION_IN_PROGRESS flag
/// Ensures flag is cleared even if retranscription panics or returns early
struct RetranscriptionGuard;

impl RetranscriptionGuard {
    /// Create guard and set flag atomically
    fn acquire() -> Result<Self, String> {
        if RETRANSCRIPTION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Retranscription already in progress".to_string());
        }
        Ok(RetranscriptionGuard)
    }
}

impl Drop for RetranscriptionGuard {
    fn drop(&mut self) {
        RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// VAD redemption time in milliseconds - bridges natural pauses in speech
/// Batch processing needs longer redemption (2000ms) than live pipeline (400ms)
/// because the entire file is processed at once by VAD, and 400ms fragments
/// speech at every natural sentence/topic pause (500ms-2s)
const VAD_REDEMPTION_TIME_MS: u32 = 2000;

/// Progress update emitted during retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionProgress {
    pub meeting_id: String,
    pub stage: String, // "decoding", "transcribing", "saving"
    pub progress_percentage: u32,
    pub message: String,
}

/// Result of retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionResult {
    pub meeting_id: String,
    pub segments_count: usize,
    pub duration_seconds: f64,
    pub language: Option<String>,
    /// True when the meeting kept both raw per-source recordings and each was
    /// transcribed separately, so the mic/system speaker labels survived.
    #[serde(default)]
    pub sources_preserved: bool,
}

/// Error during retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionError {
    pub meeting_id: String,
    pub error: String,
}

/// Check if retranscription is currently in progress
pub fn is_retranscription_in_progress() -> bool {
    RETRANSCRIPTION_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Cancel ongoing retranscription
pub fn cancel_retranscription() {
    RETRANSCRIPTION_CANCELLED.store(true, Ordering::SeqCst);
}

/// Start retranscription of a meeting's audio
pub async fn start_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    // Acquire guard - ensures flag is cleared even on panic/early return
    let _guard = RetranscriptionGuard::acquire().map_err(|e| anyhow!(e))?;

    // Reset cancellation flag
    RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);

    let use_parakeet = provider.as_deref() == Some("parakeet");
    let is_remote = provider.as_deref() == Some("remote");
    let result = run_retranscription(app.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await;

    // Unload the engine after the batch job (success, failure, or cancellation).
    // The remote provider holds no local model, so there is nothing to unload
    // — and unloading would needlessly evict a model the live path may be using.
    if !is_remote {
        super::common::unload_engine_after_batch(use_parakeet).await;
    }

    // Guard will automatically clear flag on drop
    // No need for manual: RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);

    match &result {
        Ok(res) => {
            let _ = app.emit(
                "retranscription-complete",
                serde_json::json!({
                    "meeting_id": res.meeting_id,
                    "segments_count": res.segments_count,
                    "duration_seconds": res.duration_seconds,
                    "language": res.language,
                    "sources_preserved": res.sources_preserved
                }),
            );
        }
        Err(e) => {
            let _ = app.emit(
                "retranscription-error",
                RetranscriptionError {
                    meeting_id: meeting_id.clone(),
                    error: e.to_string(),
                },
            );
        }
    }

    result
}

/// Find audio file in meeting folder
/// Tries common names first, then scans for any file with an audio extension
fn find_audio_file(folder: &Path) -> Result<PathBuf> {
    let candidates = [
        "audio.mp4", "audio.m4a", "audio.wav", "audio.mp3",
        "audio.flac", "audio.ogg", "recording.mp4",
        "audio.mkv", "audio.webm", "audio.wma",
    ];

    for name in candidates {
        let path = folder.join(name);
        if path.exists() {
            return Ok(path);
        }
    }

    // Fallback: scan folder for any file with an audio extension
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                    return Ok(path);
                }
            }
        }
    }

    Err(anyhow!("No audio file found in: {}", folder.display()))
}

/// Merge the two per-source transcript streams into one chronological list.
///
/// Labels reuse the live pipeline's canonical `mic`/`system` values
/// (transcription::worker::source_label), so everything downstream —
/// "Ja"/"Rozmówcy" rendering, summaries, exports — works unchanged.
///
/// Ordering is by segment start time. The sort is stable and mic segments are
/// appended first, so segments starting at the same instant keep mic before
/// system — an arbitrary but deterministic order for genuinely simultaneous
/// speech. Overlap is not trimmed: both sides really did speak during the
/// overlap, and the transcript should say so.
fn merge_labeled_transcripts(
    mic: Vec<(String, f64, f64)>,
    system: Vec<(String, f64, f64)>,
) -> Vec<(String, f64, f64, Option<String>)> {
    let mut labeled: Vec<(String, f64, f64, Option<String>)> =
        Vec::with_capacity(mic.len() + system.len());
    labeled.extend(
        mic.into_iter()
            .map(|(t, s, e)| (t, s, e, Some("mic".to_string()))),
    );
    labeled.extend(
        system
            .into_iter()
            .map(|(t, s, e)| (t, s, e, Some("system".to_string()))),
    );
    labeled.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    labeled
}

/// Paths to the per-source raw recordings, when the meeting kept them.
///
/// Requires BOTH files: with only one source on disk, a "per-source"
/// transcript would silently drop the other side of the conversation
/// entirely, which is worse than the mixed file's honest loss of labels.
/// Raw taps are written by `raw_tap.rs` and are off by default since the
/// raw-audio toggle became opt-in, so `None` is the common case.
fn find_raw_sources(folder: &Path) -> Option<(PathBuf, PathBuf)> {
    let mic = folder.join("raw-microphone.wav");
    let system = folder.join("raw-system.wav");
    if mic.exists() && system.exists() {
        Some((mic, system))
    } else {
        None
    }
}

/// Internal function to run retranscription
async fn run_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    let folder_path = PathBuf::from(&meeting_folder_path);

    info!(
        "Starting retranscription for meeting {} with language {:?}, model {:?}, provider {:?}",
        meeting_id, language, model, provider
    );

    emit_progress(&app, &meeting_id, "decoding", 2, "Preparing transcription engine...");

    // Initialize the engine once, before any decoding: engine problems (model
    // not downloaded, remote endpoint unconfigured) should surface immediately,
    // not after minutes of audio processing.
    let engine = match provider.as_deref() {
        Some("parakeet") => {
            TranscriptionEngine::Parakeet(get_or_init_parakeet(&app, model.as_deref()).await?)
        }
        Some("remote") => {
            let app_state = app
                .try_state::<AppState>()
                .ok_or_else(|| anyhow!("App state not available"))?;
            let mut config = SettingsRepository::get_remote_transcription_config(
                app_state.db_manager.pool(),
            )
            .await
            .map_err(|e| anyhow!("Failed to read remote transcription config: {}", e))?
            .ok_or_else(|| {
                anyhow!(
                    "Remote transcription endpoint is not configured. \
                     Set it up in Settings → Transcription first."
                )
            })?;
            // The dialog may pass a model name; it wins over the stored one.
            if let Some(m) = model.as_deref().filter(|m| !m.is_empty()) {
                config.model = m.to_string();
            }
            TranscriptionEngine::remote_from_config(config).map_err(|e| anyhow!(e))?
        }
        _ => TranscriptionEngine::Whisper(get_or_init_whisper(&app, model.as_deref()).await?),
    };

    // Per-source when both raw recordings exist, mixed-file fallback otherwise.
    let raw_sources = find_raw_sources(&folder_path);
    let sources_preserved = raw_sources.is_some();

    let mut labeled: Vec<(String, f64, f64, Option<String>)> = Vec::new();
    let mut total_confidence = 0.0f32;
    let duration_seconds;

    match raw_sources {
        Some((mic_path, system_path)) => {
            info!("Raw per-source recordings found; transcribing each source separately");

            let (mic_transcripts, mic_duration) = transcribe_audio_file(
                &app,
                &meeting_id,
                &mic_path,
                &engine,
                language.clone(),
                "microphone",
                4,
                40,
                &mut total_confidence,
            )
            .await?;
            let (system_transcripts, system_duration) = transcribe_audio_file(
                &app,
                &meeting_id,
                &system_path,
                &engine,
                language.clone(),
                "system audio",
                40,
                78,
                &mut total_confidence,
            )
            .await?;

            // The two taps started within the same session but not at the same
            // instant; the recording is as long as the longer of them.
            duration_seconds = mic_duration.max(system_duration);

            labeled = merge_labeled_transcripts(mic_transcripts, system_transcripts);
        }
        None => {
            let audio_path = find_audio_file(&folder_path)?;
            let (transcripts, duration) = transcribe_audio_file(
                &app,
                &meeting_id,
                &audio_path,
                &engine,
                language.clone(),
                "audio",
                4,
                78,
                &mut total_confidence,
            )
            .await?;
            duration_seconds = duration;
            // The mixed file cannot tell the sources apart; the label stays
            // honestly absent rather than guessed.
            labeled.extend(transcripts.into_iter().map(|(t, s, e)| (t, s, e, None)));
        }
    }

    if labeled.is_empty() {
        warn!("No speech detected in audio");
        return Err(anyhow!("No speech detected in audio file"));
    }

    let transcribed_count = labeled.len();
    let avg_confidence = total_confidence / transcribed_count as f32;

    info!(
        "Transcription complete: {} segments transcribed, sources_preserved={}, avg confidence: {:.2}",
        transcribed_count, sources_preserved, avg_confidence
    );

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    emit_progress(&app, &meeting_id, "saving", 80, "Saving transcripts...");

    // Create transcript segments with proper timestamps from VAD, carrying
    // the per-source label when this run could establish one.
    let segments = create_transcript_segments_with_sources(&labeled);

    // Save to database
    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;

    // Wrap delete+insert+update in a transaction to prevent data loss
    let pool = app_state.db_manager.pool();
    let mut conn = pool.acquire().await.map_err(|e| anyhow!("DB error: {}", e))?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|e| anyhow!("Failed to start transaction: {}", e))?;

    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(&meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to delete existing transcripts: {}", e))?;

    for segment in &segments {
        // `speaker` is bound like the live-transcription INSERT does
        // (database/repositories/transcript.rs); it used to be omitted here,
        // which is what silently erased the mic/system labels on every
        // retranscription even before this rewrite made them recoverable.
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, duration)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&segment.id)
        .bind(&meeting_id)
        .bind(&segment.text)
        .bind(&segment.timestamp)
        .bind(&segment.speaker)
        .bind(segment.audio_start_time)
        .bind(segment.audio_end_time)
        .bind(segment.duration)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to insert transcript: {}", e))?;
    }

    tx.commit().await
        .map_err(|e| anyhow!("Failed to commit transaction: {}", e))?;

    info!(
        "Updated {} transcripts for meeting {} in transaction",
        segments.len(),
        meeting_id
    );

    // Write updated transcripts.json and metadata.json to the meeting folder
    emit_progress(&app, &meeting_id, "saving", 90, "Writing transcript files...");

    if let Err(e) = write_transcripts_json(&folder_path, &segments) {
        warn!("Failed to write transcripts.json: {}", e);
    }

    // Find audio filename for metadata. Even a per-source run keeps pointing
    // at the mixed file: it is the one meant for playback, the raw taps are
    // diagnostic side channels.
    let audio_filename = find_audio_file(&folder_path)
        .ok()
        .and_then(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "audio.mp4".to_string());

    if let Err(e) = write_retranscription_metadata(
        &folder_path,
        &meeting_id,
        duration_seconds,
        &audio_filename,
    ) {
        warn!("Failed to update metadata.json: {}", e);
    }

    emit_progress(&app, &meeting_id, "complete", 100, "Retranscription complete");

    Ok(RetranscriptionResult {
        meeting_id,
        segments_count: segments.len(),
        duration_seconds,
        language,
        sources_preserved,
    })
}

/// Decode one audio file, run VAD over it, and transcribe every speech
/// segment with the given engine.
///
/// `progress_start`/`progress_end` map this file's work onto a slice of the
/// meeting-wide progress bar, so a per-source run can process two files on
/// one 0–100 scale. `source_name` is only for progress messages and logs.
///
/// A file with no detected speech yields an empty vector rather than an
/// error: in a per-source run, one silent side (a meeting where nobody but
/// the user spoke, or vice versa) is a normal outcome, and the caller decides
/// whether the aggregate is empty.
#[allow(clippy::too_many_arguments)]
async fn transcribe_audio_file<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    audio_path: &Path,
    engine: &TranscriptionEngine,
    language: Option<String>,
    source_name: &str,
    progress_start: u32,
    progress_end: u32,
    total_confidence: &mut f32,
) -> Result<(Vec<(String, f64, f64)>, f64)> {
    let span = progress_end.saturating_sub(progress_start).max(1);
    let at = |fraction: f32| progress_start + (span as f32 * fraction) as u32;

    emit_progress(
        app,
        meeting_id,
        "decoding",
        at(0.0),
        &format!("Decoding {} file...", source_name),
    );

    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Decode the audio file (CPU-intensive, run in blocking task)
    let path_for_decode = audio_path.to_path_buf();
    let decoded = tokio::task::spawn_blocking(move || decode_audio_file(&path_for_decode))
        .await
        .map_err(|e| anyhow!("Decode task panicked: {}", e))??;
    let duration_seconds = decoded.duration_seconds;

    info!(
        "Decoded {} audio: {:.2}s, {}Hz, {} channels",
        source_name, duration_seconds, decoded.sample_rate, decoded.channels
    );

    emit_progress(
        app,
        meeting_id,
        "decoding",
        at(0.05),
        &format!("Converting {} format...", source_name),
    );

    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Convert to 16kHz mono format (CPU-intensive, run in blocking task)
    let audio_samples = tokio::task::spawn_blocking(move || decoded.to_whisper_format())
        .await
        .map_err(|e| anyhow!("Resample task panicked: {}", e))?;
    info!(
        "Converted {} to 16kHz mono format: {} samples",
        source_name,
        audio_samples.len()
    );

    emit_progress(
        app,
        meeting_id,
        "vad",
        at(0.1),
        &format!("Detecting speech in {}...", source_name),
    );

    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Use VAD to find natural speech boundaries (same approach as live
    // transcription). Runs in a blocking task: for large files (35+ minutes)
    // VAD processing can take several minutes.
    let app_for_vad = app.clone();
    let meeting_id_for_vad = meeting_id.to_string();
    let source_for_vad = source_name.to_string();
    let vad_low = at(0.1);
    let vad_high = at(0.2);

    let speech_segments = tokio::task::spawn_blocking(move || {
        get_speech_chunks_with_progress(
            &audio_samples,
            VAD_REDEMPTION_TIME_MS,
            |vad_progress, segments_found| {
                let overall_progress =
                    vad_low + ((vad_high - vad_low) as f32 * vad_progress as f32 / 100.0) as u32;
                emit_progress(
                    &app_for_vad,
                    &meeting_id_for_vad,
                    "vad",
                    overall_progress,
                    &format!(
                        "Detecting speech in {}... {}% ({} found)",
                        source_for_vad, vad_progress, segments_found
                    ),
                );

                // Return false to cancel if cancellation requested
                !RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst)
            },
        )
    })
    .await
    .map_err(|e| anyhow!("VAD task panicked: {}", e))?
    .map_err(|e| anyhow!("VAD processing failed: {}", e))?;

    let total_segments = speech_segments.len();
    info!(
        "VAD detected {} speech segments in {} (redemption_time={}ms)",
        total_segments, source_name, VAD_REDEMPTION_TIME_MS
    );

    if total_segments == 0 {
        return Ok((Vec::new(), duration_seconds));
    }

    // Split very long segments at silence boundaries for better transcription
    // quality. Hard cuts at arbitrary sample positions lose words at
    // boundaries; instead, scan for the lowest-energy window near the target
    // split point and cut there.
    const MAX_SEGMENT_SAMPLES: usize = 25 * 16000; // 25 seconds at 16kHz

    let mut processable_segments: Vec<crate::audio::vad::SpeechSegment> = Vec::new();
    for segment in &speech_segments {
        if segment.samples.len() > MAX_SEGMENT_SAMPLES {
            let sub_segments = split_segment_at_silence(segment, MAX_SEGMENT_SAMPLES);
            debug!(
                "Split large segment ({:.0}ms) into {} sub-segments",
                segment.end_timestamp_ms - segment.start_timestamp_ms,
                sub_segments.len()
            );
            processable_segments.extend(sub_segments);
        } else {
            processable_segments.push(segment.clone());
        }
    }

    let processable_count = processable_segments.len();
    info!(
        "Processing {} {} segments (after splitting)",
        processable_count, source_name
    );

    let transcribe_low = at(0.2);
    let mut transcripts: Vec<(String, f64, f64)> = Vec::new();

    for (i, segment) in processable_segments.iter().enumerate() {
        if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
            return Err(anyhow!("Retranscription cancelled"));
        }

        let progress = transcribe_low
            + ((i as f32 / processable_count as f32) * (progress_end - transcribe_low) as f32)
                as u32;
        let segment_duration_sec =
            (segment.end_timestamp_ms - segment.start_timestamp_ms) / 1000.0;
        emit_progress(
            app,
            meeting_id,
            "transcribing",
            progress,
            &format!(
                "Transcribing {} segment {} of {} ({:.1}s)...",
                source_name,
                i + 1,
                processable_count,
                segment_duration_sec
            ),
        );

        // Skip very short segments (< 100ms of audio = 1600 samples at 16kHz)
        if segment.samples.len() < 1600 {
            debug!(
                "Skipping short {} segment {} with {} samples",
                source_name,
                i,
                segment.samples.len()
            );
            continue;
        }

        let (text, conf) = engine
            .transcribe_batch(segment.samples.clone(), language.clone())
            .await
            .map_err(|e| {
                anyhow!(
                    "{} transcription failed on {} segment {}: {}",
                    engine.provider_name(),
                    source_name,
                    i,
                    e
                )
            })?;

        // Skip empty transcripts
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            debug!(
                "{} segment {}/{}: {:.1}s, conf={:.2}",
                source_name,
                i + 1,
                processable_count,
                segment_duration_sec,
                conf
            );
            transcripts.push((text, segment.start_timestamp_ms, segment.end_timestamp_ms));
            *total_confidence += conf;
        } else {
            debug!(
                "{} segment {}/{}: {:.1}s — empty transcription",
                source_name,
                i + 1,
                processable_count,
                segment_duration_sec
            );
        }
    }

    Ok((transcripts, duration_seconds))
}

/// Emit progress event
fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    stage: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "retranscription-progress",
        RetranscriptionProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Get or initialize the Whisper engine, auto-loading the model if needed
/// If `requested_model` is provided, ensures that specific model is loaded
async fn get_or_init_whisper<R: Runtime>(
    app: &AppHandle<R>,
    requested_model: Option<&str>,
) -> Result<Arc<WhisperEngine>> {
    use crate::whisper_engine::commands::WHISPER_ENGINE;

    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().cloned()
    };

    match engine {
        Some(e) => {
            // Determine which model to use
            let target_model = match requested_model {
                Some(model) => model.to_string(),
                None => get_configured_whisper_model(app).await?,
            };

            // Check if the correct model is already loaded
            let current_model = e.get_current_model().await;
            let needs_load = match &current_model {
                Some(loaded) => loaded != &target_model,
                None => true,
            };

            if needs_load {
                info!(
                    "Loading Whisper model '{}' (current: {:?})",
                    target_model, current_model
                );

                // Discover available models first (populates the internal cache)
                info!("Discovering available Whisper models...");
                if let Err(discover_err) = e.discover_models().await {
                    warn!("Error during model discovery (continuing anyway): {}", discover_err);
                }

                match e.load_model(&target_model).await {
                    Ok(_) => {
                        info!("Whisper model '{}' loaded successfully", target_model);
                        Ok(e)
                    }
                    Err(load_err) => {
                        error!("Failed to load Whisper model '{}': {}", target_model, load_err);
                        Err(anyhow!("Failed to load Whisper model '{}': {}", target_model, load_err))
                    }
                }
            } else {
                info!("Whisper model '{}' already loaded", target_model);
                Ok(e)
            }
        }
        None => Err(anyhow!("Whisper engine not initialized")),
    }
}

/// Get the configured Whisper model name from the database
async fn get_configured_whisper_model<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    debug!("Getting configured Whisper model from database...");

    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| {
            error!("App state not available");
            anyhow!("App state not available")
        })?;

    debug!("Querying transcript_settings table...");

    // Query the transcript settings from the database - get both provider and model
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM transcript_settings WHERE id = '1'"
    )
    .fetch_optional(app_state.db_manager.pool())
    .await
    .map_err(|e| {
        error!("Failed to query transcript config: {}", e);
        anyhow!("Failed to query transcript config: {}", e)
    })?;

    match result {
        Some((provider, model)) => {
            info!("Found transcript config: provider={}, model={}", provider, model);

            // Check if provider is Whisper-based
            if provider == "localWhisper" || provider == "whisper" {
                Ok(model)
            } else {
                error!("Retranscription requires Whisper provider, but configured provider is: {}", provider);
                Err(anyhow!("Retranscription requires Whisper. Current provider '{}' does not support retranscription with language selection.", provider))
            }
        },
        None => {
            // Default to configured Whisper model if no config exists
            warn!("No transcript config found, using default model '{}'", DEFAULT_WHISPER_MODEL);
            Ok(DEFAULT_WHISPER_MODEL.to_string())
        }
    }
}

/// Get or initialize the Parakeet engine, auto-loading the model if needed
async fn get_or_init_parakeet<R: Runtime>(
    app: &AppHandle<R>,
    requested_model: Option<&str>,
) -> Result<Arc<ParakeetEngine>> {
    use crate::parakeet_engine::commands::PARAKEET_ENGINE;

    let engine = {
        let guard = PARAKEET_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().cloned()
    };

    match engine {
        Some(e) => {
            // Determine which model to use
            let target_model = match requested_model {
                Some(model) => model.to_string(),
                None => get_configured_parakeet_model(app).await?,
            };

            // Check if the correct model is already loaded
            let current_model = e.get_current_model().await;
            let needs_load = match &current_model {
                Some(loaded) => loaded != &target_model,
                None => true,
            };

            if needs_load {
                info!(
                    "Loading Parakeet model '{}' (current: {:?})",
                    target_model, current_model
                );

                // Discover available models first
                info!("Discovering available Parakeet models...");
                if let Err(discover_err) = e.discover_models().await {
                    warn!("Error during Parakeet model discovery (continuing anyway): {}", discover_err);
                }

                match e.load_model(&target_model).await {
                    Ok(_) => {
                        info!("Parakeet model '{}' loaded successfully", target_model);
                        Ok(e)
                    }
                    Err(load_err) => {
                        error!("Failed to load Parakeet model '{}': {}", target_model, load_err);
                        Err(anyhow!("Failed to load Parakeet model '{}': {}", target_model, load_err))
                    }
                }
            } else {
                info!("Parakeet model '{}' already loaded", target_model);
                Ok(e)
            }
        }
        None => Err(anyhow!("Parakeet engine not initialized")),
    }
}

/// Get the configured Parakeet model name from the database
async fn get_configured_parakeet_model<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    debug!("Getting configured Parakeet model from database...");

    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| {
            error!("App state not available");
            anyhow!("App state not available")
        })?;

    // Query the transcript settings from the database
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM transcript_settings WHERE id = '1'"
    )
    .fetch_optional(app_state.db_manager.pool())
    .await
    .map_err(|e| {
        error!("Failed to query transcript config: {}", e);
        anyhow!("Failed to query transcript config: {}", e)
    })?;

    match result {
        Some((provider, model)) => {
            info!("Found transcript config: provider={}, model={}", provider, model);

            if provider == "parakeet" {
                Ok(model)
            } else {
                // Default to configured Parakeet model
                warn!("Configured provider is not Parakeet, using default model");
                Ok(DEFAULT_PARAKEET_MODEL.to_string())
            }
        },
        None => {
            // Default to configured Parakeet model if no config exists
            warn!("No transcript config found, using default Parakeet model");
            Ok(DEFAULT_PARAKEET_MODEL.to_string())
        }
    }
}

/// Write or update metadata.json for retranscription (preserves existing fields, adds retranscribed_at)
fn write_retranscription_metadata(
    folder: &Path,
    meeting_id: &str,
    duration_seconds: f64,
    audio_filename: &str,
) -> Result<()> {
    let metadata_path = folder.join("metadata.json");
    let temp_path = folder.join(".metadata.json.tmp");
    let now = chrono::Utc::now().to_rfc3339();

    // Try to read existing metadata and update it
    let json = if metadata_path.exists() {
        let existing = std::fs::read_to_string(&metadata_path)?;
        let mut value: serde_json::Value = serde_json::from_str(&existing)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("retranscribed_at".to_string(), serde_json::json!(now));
            obj.insert("status".to_string(), serde_json::json!("completed"));
            obj.insert("transcript_file".to_string(), serde_json::json!("transcripts.json"));
            obj.remove("detected_summary_language");
        }
        value
    } else {
        serde_json::json!({
            "version": "1.0",
            "meeting_id": meeting_id,
            "created_at": now,
            "completed_at": now,
            "retranscribed_at": now,
            "duration_seconds": duration_seconds,
            "audio_file": audio_filename,
            "transcript_file": "transcripts.json",
            "status": "completed",
            "source": "retranscription"
        })
    };

    let json_string = serde_json::to_string_pretty(&json)?;
    std::fs::write(&temp_path, &json_string)?;
    std::fs::rename(&temp_path, &metadata_path)?;

    info!("Wrote metadata.json to {}", metadata_path.display());
    Ok(())
}

// Tauri commands

/// Response when retranscription is started
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionStarted {
    pub meeting_id: String,
    pub message: String,
}

// Start retranscription (Beta gated using configContext.betaFeatures)
#[tauri::command]
pub async fn start_retranscription_command<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionStarted, String> {

    // Check if retranscription is already in progress (guard will be acquired in start_retranscription)
    if RETRANSCRIPTION_IN_PROGRESS.load(Ordering::SeqCst) {
        return Err("Retranscription already in progress".to_string());
    }

    // Clone values for the spawned task
    let meeting_id_clone = meeting_id.clone();

    // Spawn the retranscription in a background task
    tauri::async_runtime::spawn(async move {
        let result = start_retranscription(
            app,
            meeting_id_clone,
            meeting_folder_path,
            language,
            model,
            provider,
        )
        .await;

        // Errors are already emitted as events in start_retranscription
        // so we just log here for debugging
        if let Err(e) = result {
            error!("Retranscription failed: {}", e);
        }
    });

    Ok(RetranscriptionStarted {
        meeting_id,
        message: "Retranscription started".to_string(),
    })
}

#[tauri::command]
pub async fn cancel_retranscription_command() -> Result<(), String> {
    if !is_retranscription_in_progress() {
        return Err("No retranscription in progress".to_string());
    }
    cancel_retranscription();
    Ok(())
}

#[tauri::command]
pub async fn is_retranscription_in_progress_command() -> bool {
    is_retranscription_in_progress()
}

/// Whether this meeting kept both raw per-source recordings — i.e. whether
/// retranscription can run per source and preserve the mic/system speaker
/// labels. The dialog uses this to decide between the label-loss warning and
/// the labels-preserved note.
#[tauri::command]
pub async fn retranscription_sources_available(meeting_folder_path: String) -> bool {
    find_raw_sources(Path::new(&meeting_folder_path)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::common::create_transcript_segments;

    #[test]
    fn test_create_transcript_segments_empty() {
        let transcripts: Vec<(String, f64, f64)> = vec![];
        let segments = create_transcript_segments(&transcripts);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_create_transcript_segments_single() {
        let transcripts = vec![
            ("Hello world".to_string(), 0.0, 1500.0), // 0-1.5 seconds
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello world");
        assert_eq!(segments[0].audio_start_time, Some(0.0));
        assert_eq!(segments[0].audio_end_time, Some(1.5));
        assert_eq!(segments[0].duration, Some(1.5));
    }

    #[test]
    fn test_create_transcript_segments_multiple() {
        let transcripts = vec![
            ("First segment".to_string(), 0.0, 2000.0),      // 0-2 seconds
            ("Second segment".to_string(), 3000.0, 5000.0),  // 3-5 seconds
            ("Third segment".to_string(), 6500.0, 8000.0),   // 6.5-8 seconds
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 3);

        // First segment
        assert_eq!(segments[0].text, "First segment");
        assert_eq!(segments[0].audio_start_time, Some(0.0));
        assert_eq!(segments[0].audio_end_time, Some(2.0));
        assert_eq!(segments[0].duration, Some(2.0));

        // Second segment
        assert_eq!(segments[1].text, "Second segment");
        assert_eq!(segments[1].audio_start_time, Some(3.0));
        assert_eq!(segments[1].audio_end_time, Some(5.0));
        assert_eq!(segments[1].duration, Some(2.0));

        // Third segment
        assert_eq!(segments[2].text, "Third segment");
        assert_eq!(segments[2].audio_start_time, Some(6.5));
        assert_eq!(segments[2].audio_end_time, Some(8.0));
        assert_eq!(segments[2].duration, Some(1.5));
    }

    #[test]
    fn test_create_transcript_segments_trims_whitespace() {
        let transcripts = vec![
            ("  Hello with spaces  ".to_string(), 0.0, 1000.0),
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello with spaces");
    }

    #[test]
    fn test_create_transcript_segments_generates_unique_ids() {
        let transcripts = vec![
            ("Segment one".to_string(), 0.0, 1000.0),
            ("Segment two".to_string(), 1000.0, 2000.0),
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 2);
        assert_ne!(segments[0].id, segments[1].id);
        assert!(segments[0].id.starts_with("transcript-"));
        assert!(segments[1].id.starts_with("transcript-"));
    }

    #[test]
    fn test_merge_labeled_transcripts_orders_overlapping_segments_by_start() {
        // Synthetic overlapping conversation: system speech overlaps the tail
        // of the first mic segment, mic answers before system finishes.
        let mic = vec![
            ("I think we should".to_string(), 0.0, 4_000.0),
            ("agreed, let's do that".to_string(), 6_500.0, 9_000.0),
        ];
        let system = vec![
            ("well, actually".to_string(), 3_000.0, 7_000.0),
            ("great".to_string(), 9_500.0, 10_000.0),
        ];

        let merged = merge_labeled_transcripts(mic, system);

        let order: Vec<(&str, f64)> = merged
            .iter()
            .map(|(t, s, _, src)| (src.as_deref().unwrap(), *s))
            .map(|(src, s)| (src, s))
            .collect();
        assert_eq!(
            order,
            vec![
                ("mic", 0.0),
                ("system", 3_000.0),
                ("mic", 6_500.0),
                ("system", 9_500.0),
            ]
        );
        // Overlap is preserved, not trimmed: the system segment still spans
        // into the mic segment's time range.
        assert_eq!(merged[1].2, 7_000.0);
        assert!(merged[1].2 > merged[2].1);
    }

    #[test]
    fn test_merge_labeled_transcripts_tie_keeps_mic_first() {
        let mic = vec![("me".to_string(), 1_000.0, 2_000.0)];
        let system = vec![("them".to_string(), 1_000.0, 2_000.0)];

        let merged = merge_labeled_transcripts(mic, system);

        assert_eq!(merged[0].3.as_deref(), Some("mic"));
        assert_eq!(merged[1].3.as_deref(), Some("system"));
    }

    #[test]
    fn test_merge_labeled_transcripts_one_silent_side() {
        // A meeting where only the remote side spoke: mic contributes nothing,
        // system carries the whole transcript. This must not error.
        let merged = merge_labeled_transcripts(
            Vec::new(),
            vec![("only them".to_string(), 0.0, 1_000.0)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].3.as_deref(), Some("system"));
    }

    #[test]
    fn test_find_raw_sources_requires_both_files() {
        let dir = tempfile::tempdir().unwrap();

        // Nothing on disk → None
        assert!(find_raw_sources(dir.path()).is_none());

        // Only the microphone tap → still None: a per-source transcript with
        // one side missing would silently drop the other half of the meeting.
        std::fs::write(dir.path().join("raw-microphone.wav"), b"fake").unwrap();
        assert!(find_raw_sources(dir.path()).is_none());

        // Both taps → Some
        std::fs::write(dir.path().join("raw-system.wav"), b"fake").unwrap();
        let (mic, system) = find_raw_sources(dir.path()).unwrap();
        assert_eq!(mic.file_name().unwrap(), "raw-microphone.wav");
        assert_eq!(system.file_name().unwrap(), "raw-system.wav");
    }

    #[test]
    fn test_create_transcript_segments_with_sources_carries_labels() {
        use crate::audio::common::create_transcript_segments_with_sources;

        let labeled = vec![
            ("mine".to_string(), 0.0, 1_000.0, Some("mic".to_string())),
            ("theirs".to_string(), 1_000.0, 2_000.0, Some("system".to_string())),
            ("unknown".to_string(), 2_000.0, 3_000.0, None),
        ];
        let segments = create_transcript_segments_with_sources(&labeled);

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].speaker.as_deref(), Some("mic"));
        assert_eq!(segments[1].speaker.as_deref(), Some("system"));
        assert_eq!(segments[2].speaker, None);
    }

    #[test]
    fn test_cancellation_flag() {
        // Reset flag to known state
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);

        assert!(!is_retranscription_in_progress());

        // Test cancellation
        cancel_retranscription();
        assert!(RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst));

        // Reset for other tests
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_vad_redemption_time_constant() {
        // Batch processing uses 2000ms to bridge natural pauses in full-file VAD
        assert_eq!(VAD_REDEMPTION_TIME_MS, 2000);
    }

    #[test]
    fn test_find_audio_file_common_candidates() {
        let dir = tempfile::tempdir().unwrap();

        // No audio file → error
        assert!(find_audio_file(dir.path()).is_err());

        // Create audio.mp4 — should be found first
        std::fs::write(dir.path().join("audio.mp4"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.mp4");
    }

    #[test]
    fn test_find_audio_file_non_mp4_extensions() {
        let dir = tempfile::tempdir().unwrap();

        // Create audio.wav (imported as .wav, not .mp4)
        std::fs::write(dir.path().join("audio.wav"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.wav");
    }

    #[test]
    fn test_find_audio_file_fallback_scan() {
        let dir = tempfile::tempdir().unwrap();

        // Create a file with an audio extension but non-standard name
        std::fs::write(dir.path().join("my_recording.flac"), b"fake").unwrap();
        // Also add a non-audio file that should be ignored
        std::fs::write(dir.path().join("notes.txt"), b"text").unwrap();

        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "my_recording.flac");
    }

    #[test]
    fn test_find_audio_file_priority_order() {
        let dir = tempfile::tempdir().unwrap();

        // Create both audio.m4a and audio.mp4 — mp4 should win (listed first in candidates)
        std::fs::write(dir.path().join("audio.m4a"), b"fake").unwrap();
        std::fs::write(dir.path().join("audio.mp4"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.mp4");
    }

    #[test]
    fn test_find_audio_file_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_audio_file(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No audio file found"));
    }

    #[test]
    fn test_find_audio_file_nonexistent_folder() {
        let result = find_audio_file(Path::new("/nonexistent/path/12345"));
        assert!(result.is_err());
    }

    #[test]
    fn test_audio_extensions_constant() {
        // Verify all expected formats are covered
        assert!(AUDIO_EXTENSIONS.contains(&"mp4"));
        assert!(AUDIO_EXTENSIONS.contains(&"m4a"));
        assert!(AUDIO_EXTENSIONS.contains(&"wav"));
        assert!(AUDIO_EXTENSIONS.contains(&"mp3"));
        assert!(AUDIO_EXTENSIONS.contains(&"flac"));
        assert!(AUDIO_EXTENSIONS.contains(&"ogg"));
        assert!(AUDIO_EXTENSIONS.contains(&"aac"));
        // FFmpeg-backed formats
        assert!(AUDIO_EXTENSIONS.contains(&"mkv"));
        assert!(AUDIO_EXTENSIONS.contains(&"webm"));
        assert!(AUDIO_EXTENSIONS.contains(&"wma"));
        // Non-audio formats
        assert!(!AUDIO_EXTENSIONS.contains(&"txt"));
        assert!(!AUDIO_EXTENSIONS.contains(&"pdf"));
    }
}
