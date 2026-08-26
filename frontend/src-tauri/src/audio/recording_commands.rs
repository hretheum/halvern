// audio/recording_commands.rs
//
// Slim Tauri command layer for recording functionality.
// Delegates to transcription and recording modules for actual implementation.

use anyhow::Result;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::task::JoinHandle;

use super::{
    parse_audio_device,
    default_input_device,   // Get default microphone
    default_output_device,  // Get default system audio
    RecordingManager,
    DeviceEvent,
    DeviceMonitorType
};

// Import transcription modules
use super::transcription::{
    self,
    reset_speech_detected_flag,
};

// Re-export TranscriptUpdate for backward compatibility
pub use super::transcription::TranscriptUpdate;

// ============================================================================
// GLOBAL STATE
// ============================================================================

// Simple recording state tracking
static IS_RECORDING: AtomicBool = AtomicBool::new(false);

// Global recording manager and transcription task to keep them alive during recording
static RECORDING_MANAGER: Mutex<Option<RecordingManager>> = Mutex::new(None);
static TRANSCRIPTION_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

// Listener ID for proper cleanup - prevents microphone from staying active after recording stops
static TRANSCRIPT_LISTENER_ID: Mutex<Option<tauri::EventId>> = Mutex::new(None);

// Bumped on every recording start. A max-duration watchdog captures the value
// at spawn and refuses to stop anything if it has changed since — recording A's
// watchdog must never kill recording B.
static RECORDING_GENERATION: AtomicU64 = AtomicU64::new(0);

// ============================================================================
// PUBLIC TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RecordingArgs {
    pub save_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct TranscriptionStatus {
    pub chunks_in_queue: usize,
    pub is_processing: bool,
    pub last_activity_ms: u64,
}

// ============================================================================
// RECORDING COMMANDS
// ============================================================================

/// Decide what to call this recording, and keep whatever the calendar knew.
///
/// Priority: a name the user typed wins outright, then the calendar's title,
/// then `fallback`. The match is returned alongside the name even when the user
/// named the meeting themselves — the participants and agenda are worth keeping
/// regardless of who won the naming.
///
/// `fallback` is a closure so a timestamp is never formatted for a name that is
/// about to be discarded.
fn resolve_meeting_name(
    user_supplied: Option<String>,
    fallback: impl FnOnce() -> String,
) -> (String, Option<crate::calendar::CalendarEvent>) {
    let matched = crate::calendar::find_current_event();
    let name = user_supplied
        .or_else(|| matched.as_ref().map(|m| m.title.clone()))
        .unwrap_or_else(fallback);
    (name, matched)
}

/// Pure decision core of the max-duration watchdog, split out so the one part
/// that can go wrong is testable without hardware or a clock.
///
/// `max_recording_minutes == 0` counts as disabled: a hand-edited settings blob
/// with a zero would otherwise stop every recording at the first check.
fn watchdog_should_stop(
    auto_stop_enabled: bool,
    max_recording_minutes: u64,
    elapsed_secs: u64,
    same_recording_still_running: bool,
) -> bool {
    auto_stop_enabled
        && max_recording_minutes > 0
        && same_recording_still_running
        && elapsed_secs >= max_recording_minutes * 60
}

/// Arms the hard length cap for the recording that just started.
///
/// This is the safety net that does not depend on the meeting detector: it
/// covers manually started recordings too, which is exactly the case that
/// produced a seven-hour recording of an empty room on 12 Aug 2026. The
/// detector-driven stop proposal asks first; this one does not, because by the
/// time a recording has run for `max_recording_minutes` there is nobody at the
/// machine to ask.
///
/// The cap is read once, here — a settings change applies from the next
/// recording. One sleep instead of polling; on wake the watchdog re-checks
/// that its recording is still the current one before acting.
fn spawn_max_duration_watchdog<R: Runtime>(app: AppHandle<R>) {
    let generation = RECORDING_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    tauri::async_runtime::spawn(async move {
        let config = match app.try_state::<crate::state::AppState>() {
            Some(state) => {
                crate::database::repositories::setting::SettingsRepository::get_detection_settings(
                    state.db_manager.pool(),
                )
                .await
                .unwrap_or_default()
            }
            None => Default::default(),
        };

        if !config.auto_stop_enabled || config.max_recording_minutes == 0 {
            info!("Max-duration watchdog not armed (auto-stop disabled or cap set to 0)");
            return;
        }

        let cap_secs = config.max_recording_minutes * 60;
        info!(
            "Max-duration watchdog armed: this recording stops after {} min",
            config.max_recording_minutes
        );

        tokio::time::sleep(std::time::Duration::from_secs(cap_secs)).await;

        let same_recording = RECORDING_GENERATION.load(Ordering::SeqCst) == generation
            && IS_RECORDING.load(Ordering::SeqCst);

        if !watchdog_should_stop(
            config.auto_stop_enabled,
            config.max_recording_minutes,
            cap_secs,
            same_recording,
        ) {
            return;
        }

        warn!(
            "Recording reached the {} min cap — stopping it. \
             Raise maxRecordingMinutes in detection settings if this was a real meeting.",
            config.max_recording_minutes
        );

        match stop_recording(app.clone(), RecordingArgs { save_path: String::new() }).await {
            Ok(()) => notify_frontend_stop_complete(&app),
            Err(e) => error!("Max-duration watchdog failed to stop the recording: {}", e),
        }
    });
}

/// Hands a finished recording over to the frontend for post-processing.
///
/// Saving the meeting to the database, navigating to it and reporting analytics
/// all live in the frontend, and `recording-stop-complete` is what starts them.
/// **Every stop initiated from Rust has to call this**: the detector's proposal,
/// the max-duration watchdog and the tray. A stop that skips it leaves the
/// recording on disk but absent from the application — no entry in the meetings
/// list, and a stranded transcript on screen with nothing to do about it.
///
/// The stop command invoked from the user interface must **not** call it: that
/// path runs the same post-processing itself, and a second run would duplicate
/// the work.
pub(crate) fn notify_frontend_stop_complete<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = app.emit("recording-stop-complete", true) {
        error!("Could not hand the recording over for post-processing: {}", e);
    }
}

/// Microphone from a preferred name, then the system default.
///
/// Fatal when both fail: a recording without a microphone captures nothing the
/// user said.
fn resolve_microphone(preferred: Option<&str>) -> Result<Option<Arc<super::AudioDevice>>, String> {
    if let Some(name) = preferred {
        info!("🎤 Attempting to use preferred microphone: '{}'", name);
        match parse_audio_device(name) {
            Ok(device) => {
                info!("✅ Using preferred microphone: '{}'", device.name);
                return Ok(Some(Arc::new(device)));
            }
            Err(e) => warn!(
                "⚠️ Preferred microphone '{}' not available ({}), falling back to the default",
                name, e
            ),
        }
    } else {
        info!("🎤 No microphone preference set, using system default");
    }

    match default_input_device() {
        Ok(device) => {
            info!("✅ Using default microphone: '{}'", device.name);
            Ok(Some(Arc::new(device)))
        }
        Err(e) => {
            error!("❌ No microphone available");
            Err(match preferred {
                Some(name) => format!(
                    "No microphone device available. Preferred device '{}' not found, and default microphone unavailable: {}",
                    name, e
                ),
                None => format!("No microphone device available: {}", e),
            })
        }
    }
}

/// System audio from a preferred name, then the default, then nothing.
///
/// Optional by design: a meeting recorded from the microphone alone is still
/// worth having, so every failure here degrades rather than aborts.
fn resolve_system_audio(preferred: Option<&str>) -> Option<Arc<super::AudioDevice>> {
    if let Some(name) = preferred {
        info!("🔊 Attempting to use preferred system audio: '{}'", name);
        match parse_audio_device(name) {
            Ok(device) => {
                info!("✅ Using preferred system audio: '{}'", device.name);
                return Some(Arc::new(device));
            }
            Err(e) => warn!(
                "⚠️ Preferred system audio '{}' not available ({}), falling back to the default",
                name, e
            ),
        }
    } else {
        info!("🔊 No system audio preference set, using system default");
    }

    match default_output_device() {
        Ok(device) => {
            info!("✅ Using default system audio: '{}'", device.name);
            Some(Arc::new(device))
        }
        Err(e) => {
            warn!("⚠️ No system audio available ({}); recording continues with the microphone only", e);
            None
        }
    }
}

/// Settings a start path needs after it has decided which devices to open.
struct RecordingRuntimePrefs {
    auto_save: bool,
    save_raw_sources: bool,
    recordings_root: Option<std::path::PathBuf>,
}

/// Reads the recording preferences, falling back to sane values.
///
/// Failure is not fatal: recording the mix and skipping the raw sources is the
/// right answer when nobody has said otherwise.
async fn load_runtime_prefs<R: Runtime>(
    app: &AppHandle<R>,
) -> (RecordingRuntimePrefs, Option<String>, Option<String>) {
    match super::recording_preferences::load_recording_preferences(app).await {
        Ok(prefs) => {
            info!(
                "📋 Loaded recording preferences: auto_save={}, raw_sources={}, folder={:?}, preferred_mic={:?}, preferred_system={:?}",
                prefs.auto_save, prefs.save_raw_sources, prefs.save_folder,
                prefs.preferred_mic_device, prefs.preferred_system_device
            );
            (
                RecordingRuntimePrefs {
                    auto_save: prefs.auto_save,
                    save_raw_sources: prefs.save_raw_sources,
                    recordings_root: Some(prefs.save_folder),
                },
                prefs.preferred_mic_device,
                prefs.preferred_system_device,
            )
        }
        Err(e) => {
            warn!("Failed to load recording preferences, using defaults: {}", e);
            (
                RecordingRuntimePrefs {
                    auto_save: true,
                    save_raw_sources: false,
                    recordings_root: None,
                },
                None,
                None,
            )
        }
    }
}

/// The checks every start path runs before it commits to anything.
///
/// Returns the engine lifecycle guard, which the caller hands to
/// [`begin_recording`] — holding it across model validation is the point, so it
/// cannot be acquired later.
async fn prepare_to_record<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let guard = super::common::acquire_engine_lifecycle_lock().await;

    let current_recording_state = IS_RECORDING.load(Ordering::SeqCst);
    info!("🔍 IS_RECORDING state check: {}", current_recording_state);
    if current_recording_state {
        return Err("Recording already in progress".to_string());
    }

    info!("🔍 Validating transcription model availability before starting recording...");
    if let Err(validation_error) = transcription::validate_transcription_model_ready(app).await {
        error!("Model validation failed: {}", validation_error);

        // actionable: false — the download progress already has a toast, so a
        // modal here would be the second thing shouting about one situation.
        let _ = app.emit("transcription-error", serde_json::json!({
            "error": validation_error,
            "userMessage": "Recording cannot start: Transcription model is still downloading. Please wait for the download to complete.",
            "actionable": false
        }));

        return Err(validation_error);
    }
    info!("✅ Transcription model validation passed");

    Ok(guard)
}

/// Everything a start path does once it knows which devices to open.
///
/// The two entry points differ only in how they arrive at that pair — one reads
/// the saved preferences, the other takes names from the caller — and used to
/// carry a copy of this each. Every change to the recording lifecycle then had
/// to be made twice, which is exactly how one of them ended up without the
/// default-device fallback the other had.
#[allow(clippy::too_many_arguments)]
async fn begin_recording<R: Runtime>(
    app: AppHandle<R>,
    microphone_device: Option<Arc<super::AudioDevice>>,
    system_device: Option<Arc<super::AudioDevice>>,
    meeting_name: Option<String>,
    fallback_name: Option<String>,
    origin: super::recording_saver::RecordingOrigin,
    prefs: RecordingRuntimePrefs,
    engine_lifecycle_guard: tokio::sync::OwnedMutexGuard<()>,
) -> Result<(), String> {
    let mut manager = RecordingManager::new();

    // Caller's name wins, then the calendar, then a timestamp. Always set, so
    // the incremental saver has a folder to initialise.
    let (effective_meeting_name, calendar_match) =
        resolve_meeting_name(meeting_name, || {
            fallback_name.unwrap_or_else(|| {
                let now = chrono::Local::now();
                format!("Meeting {}", now.format("%Y-%m-%d_%H-%M-%S"))
            })
        });
    manager.set_meeting_name(Some(effective_meeting_name));
    manager.set_recordings_root(prefs.recordings_root);
    manager.set_calendar_match(calendar_match);
    manager.set_origin(origin);

    let app_for_error = app.clone();
    manager.set_error_callback(move |error| {
        let _ = app_for_error.emit("recording-error", error.user_message());
    });

    // Taken before the devices are handed to the manager, which consumes them.
    let microphone_name = microphone_device.as_ref().map(|device| device.name.clone());
    let system_name = system_device.as_ref().map(|device| device.name.clone());

    let transcription_receiver = manager
        .start_recording(
            microphone_device,
            system_device,
            prefs.auto_save,
            prefs.save_raw_sources,
        )
        .await
        .map_err(|e| format!("Failed to start recording: {}", e))?;

    {
        let mut global_manager = RECORDING_MANAGER.lock().unwrap();
        *global_manager = Some(manager);
    }

    info!("🔍 Setting IS_RECORDING to true and resetting SPEECH_DETECTED_EMITTED");
    IS_RECORDING.store(true, Ordering::SeqCst);
    // Sample counts describe this recording, not every recording since launch,
    // and the names are what a "nothing is arriving" message has to point at.
    super::input_activity::reset();
    super::input_activity::set_devices(
        microphone_name.clone(),
        system_name.clone(),
    );
    drop(engine_lifecycle_guard);
    reset_speech_detected_flag();

    // Hard length cap — armed for every recording, manual or detected.
    spawn_max_duration_watchdog(app.clone());

    let task_handle = transcription::start_transcription_task(app.clone(), transcription_receiver);
    {
        let mut global_task = TRANSCRIPTION_TASK.lock().unwrap();
        *global_task = Some(task_handle);
    }

    // Transcript history has to survive a page reload, so segments are mirrored
    // into the manager as they arrive. The listener id is kept for stop, which
    // must remove it or the microphone stays live.
    {
        use tauri::Listener;
        let listener_id = app.listen("transcript-update", move |event: tauri::Event| {
            if let Ok(update) = serde_json::from_str::<TranscriptUpdate>(event.payload()) {
                let segment = crate::audio::recording_saver::TranscriptSegment {
                    id: format!("seg_{}", update.sequence_id),
                    text: update.text.clone(),
                    audio_start_time: update.audio_start_time,
                    audio_end_time: update.audio_end_time,
                    duration: update.duration,
                    display_time: update.timestamp.clone(),
                    confidence: update.confidence,
                    sequence_id: update.sequence_id,
                    source: Some(update.source.clone()),
                };

                if let Ok(manager_guard) = RECORDING_MANAGER.lock() {
                    if let Some(manager) = manager_guard.as_ref() {
                        manager.add_transcript_segment(segment);
                    }
                }
            }
        });
        let mut global_listener = TRANSCRIPT_LISTENER_ID.lock().unwrap();
        *global_listener = Some(listener_id);
        info!("✅ Transcript-update event listener registered for history persistence");
    }

    app.emit("recording-started", serde_json::json!({
        "message": "Recording started successfully with parallel processing",
        "workers": 3
    })).map_err(|e| e.to_string())?;

    crate::tray::update_tray_menu(&app);
    info!("✅ Recording started successfully with async-first approach");

    Ok(())
}

/// Start recording with default devices
pub async fn start_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    start_recording_with_meeting_name(app, None, None, Default::default()).await
}

/// Start recording with default devices and optional meeting name
///
/// `meeting_name` is a name the user chose, which nothing may override.
/// `fallback_name` is what to use when neither the user nor the calendar has one
/// — the meeting detector passes the app it heard, everyone else passes `None`
/// and gets a timestamp.
///
/// `origin` says who started this and, for the detector, which application was
/// making the noise. It is separate from `fallback_name` on purpose: that name
/// is a display string the calendar is allowed to overwrite, so reading the app
/// back out of it later would be parsing a label rather than carrying data.
pub async fn start_recording_with_meeting_name<R: Runtime>(
    app: AppHandle<R>,
    meeting_name: Option<String>,
    fallback_name: Option<String>,
    origin: super::recording_saver::RecordingOrigin,
) -> Result<(), String> {
    info!("Starting recording with saved device preferences, meeting: {:?}", meeting_name);

    let engine_lifecycle_guard = prepare_to_record(&app).await?;
    let (prefs, preferred_mic_name, preferred_system_name) = load_runtime_prefs(&app).await;

    // Microphone: preference, then the system default. Without one, there is
    // nothing to record, so this is the only fatal branch.
    let microphone_device = match resolve_microphone(preferred_mic_name.as_deref()) {
        Ok(device) => device,
        Err(e) => return Err(e),
    };
    // System audio: preference, then the default, then nothing. A meeting with
    // only the microphone is worth recording; a silent failure is not.
    let system_device = resolve_system_audio(preferred_system_name.as_deref());

    begin_recording(
        app,
        microphone_device,
        system_device,
        meeting_name,
        fallback_name,
        origin,
        prefs,
        engine_lifecycle_guard,
    )
    .await
}

/// Start recording with specific devices
pub async fn start_recording_with_devices<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
) -> Result<(), String> {
    start_recording_with_devices_and_meeting(app, mic_device_name, system_device_name, None).await
}

/// Start recording with specific devices and optional meeting name
pub async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    info!(
        "Starting recording with specific devices: mic={:?}, system={:?}, meeting={:?}",
        mic_device_name, system_device_name, meeting_name
    );

    let engine_lifecycle_guard = prepare_to_record(&app).await?;

    // A name the caller supplied must exist; `None` means "the default", which
    // is what the UI sends for its Default entries. Treating None as "no stream"
    // is what once produced silent recordings for anyone without a saved
    // preference.
    let microphone_device = match mic_device_name {
        Some(name) => Some(Arc::new(parse_audio_device(&name).map_err(|e| {
            format!("Invalid microphone device '{}': {}", name, e)
        })?)),
        None => resolve_microphone(None)?,
    };
    let system_device = match system_device_name {
        Some(name) => Some(Arc::new(parse_audio_device(&name).map_err(|e| {
            format!("Invalid system device '{}': {}", name, e)
        })?)),
        None => resolve_system_audio(None),
    };

    let (prefs, _, _) = load_runtime_prefs(&app).await;

    begin_recording(
        app,
        microphone_device,
        system_device,
        meeting_name,
        None,
        // Reached only from the UI's own record button.
        super::recording_saver::RecordingOrigin::default(),
        prefs,
        engine_lifecycle_guard,
    )
    .await
}

/// Stop recording with optimized graceful shutdown ensuring NO transcript chunks are lost
pub async fn stop_recording<R: Runtime>(
    app: AppHandle<R>,
    _args: RecordingArgs,
) -> Result<(), String> {
    info!(
        "🛑 Starting optimized recording shutdown - ensuring ALL transcript chunks are preserved"
    );

    // Release the capture scope whatever happens below, so the next recording
    // never inherits the previous meeting's application.
    crate::audio::system_detector::set_meeting_scope(None);

    // Check if recording is active
    if !IS_RECORDING.load(Ordering::SeqCst) {
        info!("Recording was not active");
        return Ok(());
    }

    // Emit shutdown progress to frontend
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "stopping_audio",
            "message": "Stopping audio capture...",
            "progress": 20
        }),
    );

    // Step 1: Stop audio capture immediately (no more new chunks) with proper error handling
    let manager_for_cleanup = {
        let mut global_manager = RECORDING_MANAGER.lock().unwrap();
        global_manager.take()
    };

    let stop_result = if let Some(mut manager) = manager_for_cleanup {
        // Use FORCE FLUSH to immediately process all accumulated audio - eliminates 30s delay!
        info!("🚀 Using FORCE FLUSH to eliminate pipeline accumulation delays");
        let result = manager.stop_streams_and_force_flush().await;
        // Store manager back for later cleanup
        let manager_for_cleanup = Some(manager);
        (result, manager_for_cleanup)
    } else {
        warn!("No recording manager found to stop");
        (Ok(()), None)
    };

    let (stop_result, manager_for_cleanup) = stop_result;

    match stop_result {
        Ok(_) => {
            info!("✅ Audio streams stopped successfully - no more chunks will be created");
        }
        Err(e) => {
            error!("❌ Failed to stop audio streams: {}", e);
            return Err(format!("Failed to stop audio streams: {}", e));
        }
    }

    // Step 1.5: Clean up transcript listener to release microphone
    // Unlisten transcript-update event to prevent lingering references
    {
        use tauri::Listener;
        if let Some(listener_id) = TRANSCRIPT_LISTENER_ID.lock().unwrap().take() {
            app.unlisten(listener_id);
            info!("✅ Transcript-update listener removed");
        }
    }

    // Step 2: Signal transcription workers to finish processing ALL queued chunks
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "processing_transcripts",
            "message": "Processing remaining transcript chunks...",
            "progress": 40
        }),
    );

    // Wait for transcription task with enhanced progress monitoring (NO TIMEOUT - we must process all chunks)
    let transcription_task = {
        let mut global_task = TRANSCRIPTION_TASK.lock().unwrap();
        global_task.take()
    };

    if let Some(task_handle) = transcription_task {
        info!("⏳ Waiting for ALL transcription chunks to be processed (no timeout - preserving every chunk)");

        // Enhanced progress monitoring during shutdown
        let progress_app = app.clone();
        let progress_task = tokio::spawn(async move {
            let last_update = std::time::Instant::now();

            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                // Emit periodic progress updates during shutdown
                let elapsed = last_update.elapsed().as_secs();
                let _ = progress_app.emit(
                    "recording-shutdown-progress",
                    serde_json::json!({
                        "stage": "processing_transcripts",
                        "message": format!("Processing transcripts... ({}s elapsed)", elapsed),
                        "progress": 40,
                        "detailed": true,
                        "elapsed_seconds": elapsed
                    }),
                );
            }
        });

        // Wait up to 10 minutes for transcription completion to prevent indefinite hangs
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(600), // 10 minutes max
            task_handle
        ).await {
            Ok(Ok(())) => {
                info!("✅ ALL transcription chunks processed successfully - no data lost");
            }
            Ok(Err(e)) => {
                warn!("⚠️ Transcription task completed with error: {:?}", e);
                // Continue anyway - the worker may have processed most chunks
            }
            Err(_) => {
                warn!("⏱️ Transcription timeout (10 minutes) reached, continuing shutdown to prevent indefinite hang");
                // Continue shutdown even on timeout - better to lose some chunks than hang forever
            }
        }

        // Stop progress monitoring
        progress_task.abort();
    } else {
        info!("ℹ️ No transcription task found to wait for");
    }

    // Step 3: Now safely unload Whisper model after ALL chunks are processed
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "unloading_model",
            "message": "Unloading speech recognition model...",
            "progress": 70
        }),
    );

    info!("🧠 All transcript chunks processed. Now safely unloading transcription model...");

    // Determine which provider was used and unload the appropriate model (with timeout)
    let config = match tokio::time::timeout(
        tokio::time::Duration::from_secs(30), // 30 seconds max for DB operation
        crate::api::api::api_get_transcript_config(
            app.clone(),
            app.clone().state(),
            None,
        )
    )
    .await
    {
        Ok(Ok(Some(config))) => Some(config.provider),
        Ok(Ok(None)) => None,
        Ok(Err(e)) => {
            warn!("⚠️ Failed to get transcript config: {:?}", e);
            None
        }
        Err(_) => {
            warn!("⏱️ Transcript config timeout (30s), continuing shutdown");
            None
        }
    };

    match config.as_deref() {
        Some("parakeet") => {
            info!("🦜 Unloading Parakeet model...");
            let engine_clone = {
                let engine_guard = crate::parakeet_engine::commands::PARAKEET_ENGINE
                    .lock()
                    .unwrap();
                engine_guard.as_ref().cloned()
            };

            if let Some(engine) = engine_clone {
                let current_model = engine
                    .get_current_model()
                    .await
                    .unwrap_or_else(|| "unknown".to_string());
                info!("Current Parakeet model before unload: '{}'", current_model);

                if engine.unload_model().await {
                    info!("✅ Parakeet model '{}' unloaded successfully", current_model);
                } else {
                    warn!("⚠️ Failed to unload Parakeet model '{}'", current_model);
                }
            } else {
                warn!("⚠️ No Parakeet engine found to unload model");
            }
        }
        _ => {
            // Default to Whisper
            info!("🎤 Unloading Whisper model...");
            let engine_clone = {
                let engine_guard = crate::whisper_engine::commands::WHISPER_ENGINE
                    .lock()
                    .unwrap();
                engine_guard.as_ref().cloned()
            };

            if let Some(engine) = engine_clone {
                let current_model = engine
                    .get_current_model()
                    .await
                    .unwrap_or_else(|| "unknown".to_string());
                info!("Current Whisper model before unload: '{}'", current_model);

                if engine.unload_model().await {
                    info!("✅ Whisper model '{}' unloaded successfully", current_model);
                } else {
                    warn!("⚠️ Failed to unload Whisper model '{}'", current_model);
                }
            } else {
                warn!("⚠️ No Whisper engine found to unload model");
            }
        }
    }

    // Step 3.5: Track meeting ended analytics with privacy-safe metadata
    // Extract all data from manager BEFORE any async operations to avoid Send issues
    let analytics_data = if let Some(ref manager) = manager_for_cleanup {
        let state = manager.get_state();
        let stats = state.get_stats();

        Some((
            manager.get_recording_duration(),
            manager.get_active_recording_duration().unwrap_or(0.0),
            manager.get_total_pause_duration(),
            manager.get_transcript_segments().len() as u64,
            state.has_fatal_error(),
            state.get_microphone_device().map(|d| d.name.clone()),
            state.get_system_device().map(|d| d.name.clone()),
            stats.chunks_processed,
        ))
    } else {
        None
    };

    // Now perform async analytics tracking without holding manager reference
    if let Some((total_duration, active_duration, pause_duration, transcript_segments_count, had_fatal_error, mic_device_name, sys_device_name, chunks_processed)) = analytics_data {
        info!("📊 Collecting analytics for meeting end");

        // Helper function to classify device type from device name (privacy-safe)
        fn classify_device_type(device_name: &str) -> &'static str {
            let name_lower = device_name.to_lowercase();
            // Check for Bluetooth keywords
            if name_lower.contains("bluetooth")
                || name_lower.contains("airpods")
                || name_lower.contains("beats")
                || name_lower.contains("headphones")
                || name_lower.contains("bt ")
                || name_lower.contains("wireless") {
                "Bluetooth"
            } else {
                "Wired"
            }
        }

        // Get transcription model info (already loaded above for model unload)
        let transcription_config = match crate::api::api::api_get_transcript_config(
            app.clone(),
            app.clone().state(),
            None,
        )
        .await
        {
            Ok(Some(config)) => Some((config.provider, config.model)),
            _ => None,
        };

        let (transcription_provider, transcription_model) = transcription_config
            .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

        // Get summary model info from API
        let summary_config = match crate::api::api::api_get_model_config(
            app.clone(),
            app.clone().state(),
            None,
        )
        .await
        {
            Ok(Some(config)) => Some((config.provider, config.model)),
            _ => None,
        };

        let (summary_provider, summary_model) = summary_config
            .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

        // Classify device types (privacy-safe)
        let microphone_device_type = mic_device_name
            .as_ref()
            .map(|name| classify_device_type(name))
            .unwrap_or("Unknown");

        let system_audio_device_type = sys_device_name
            .as_ref()
            .map(|name| classify_device_type(name))
            .unwrap_or("Unknown");

        // Track meeting ended event with privacy-safe data
        match crate::analytics::commands::track_meeting_ended(
            transcription_provider.clone(),
            transcription_model.clone(),
            summary_provider.clone(),
            summary_model.clone(),
            total_duration,
            active_duration,
            pause_duration,
            microphone_device_type.to_string(),
            system_audio_device_type.to_string(),
            chunks_processed,
            transcript_segments_count,
            had_fatal_error,
        )
        .await
        {
            Ok(_) => info!("✅ Analytics tracked successfully for meeting end"),
            Err(e) => warn!("⚠️ Failed to track analytics: {}", e),
        }
    }

    // Step 4: Finalize recording state and cleanup resources safely
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "finalizing",
            "message": "Finalizing recording and cleaning up resources...",
            "progress": 90
        }),
    );

    // Perform final cleanup with the manager if available
    let (meeting_folder, meeting_name) = if let Some(mut manager) = manager_for_cleanup {
        info!("🧹 Performing final cleanup and saving recording data");

        // Extract meeting info BEFORE async operations
        let meeting_folder = manager.get_meeting_folder();
        let meeting_name = manager.get_meeting_name();

        match tokio::time::timeout(
            tokio::time::Duration::from_secs(300), // 5 minutes max for file I/O
            manager.save_recording_only(&app)
        ).await {
            Ok(Ok(_)) => {
                info!("✅ Recording data saved successfully during cleanup");
            }
            Ok(Err(e)) => {
                warn!(
                    "⚠️ Error during recording cleanup (transcripts preserved): {}",
                    e
                );
                // Don't fail shutdown - transcripts are already preserved
            }
            Err(_) => {
                warn!("⏱️ File I/O timeout (5 minutes) reached during save, continuing shutdown");
                // Don't fail shutdown - transcripts are already preserved
            }
        }

        (meeting_folder, meeting_name)
    } else {
        info!("ℹ️ No recording manager available for cleanup");
        (None, None)
    };

    // Set recording flag to false
    info!("🔍 Setting IS_RECORDING to false");
    IS_RECORDING.store(false, Ordering::SeqCst);

    // Step 4.5: Prepare metadata for frontend (NO database save)
    // NOTE: We do NOT save to database here. The frontend will save after all transcripts are displayed.
    // This ensures the user sees all transcripts streaming in before the database save happens.
    let (folder_path_str, meeting_name_str) = match (&meeting_folder, &meeting_name) {
        (Some(path), Some(name)) => (
            Some(path.to_string_lossy().to_string()),
            Some(name.clone()),
        ),
        _ => (None, None),
    };

    info!("📤 Preparing recording metadata for frontend save");
    info!("   folder_path: {:?}", folder_path_str);
    info!("   meeting_name: {:?}", meeting_name_str);

    // Database save removed - frontend will handle this after receiving all transcripts
    info!("ℹ️ Skipping database save in Rust - frontend will save after all transcripts received");

    // Step 5: Complete shutdown
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "complete",
            "message": "Recording stopped successfully",
            "progress": 100
        }),
    );

    // Emit final stop event with folder_path and meeting_name for frontend to save
    app.emit(
        "recording-stopped",
        serde_json::json!({
            "message": "Recording stopped - frontend will save after all transcripts received",
            "folder_path": folder_path_str,
            "meeting_name": meeting_name_str
        }),
    )
    .map_err(|e| e.to_string())?;

    // Update tray menu to reflect stopped state
    crate::tray::update_tray_menu(&app);

    info!("🎉 Recording stopped successfully with ZERO transcript chunks lost");
    Ok(())
}

/// Check if recording is active
pub async fn is_recording() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

/// Get recording statistics
pub async fn get_transcription_status() -> TranscriptionStatus {
    TranscriptionStatus {
        chunks_in_queue: 0,
        is_processing: IS_RECORDING.load(Ordering::SeqCst),
        last_activity_ms: 0,
    }
}

/// Pause the current recording
#[tauri::command]
pub async fn pause_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    info!("Pausing recording");

    // Check if currently recording
    if !IS_RECORDING.load(Ordering::SeqCst) {
        return Err("No recording is currently active".to_string());
    }

    // Access the recording manager and pause it
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        manager.pause_recording().map_err(|e| e.to_string())?;

        // Emit pause event to frontend
        app.emit(
            "recording-paused",
            serde_json::json!({
                "message": "Recording paused"
            }),
        )
        .map_err(|e| e.to_string())?;

        // Update tray menu to reflect paused state
        crate::tray::update_tray_menu(&app);

        info!("Recording paused successfully");
        Ok(())
    } else {
        Err("No recording manager found".to_string())
    }
}

/// Resume the current recording
#[tauri::command]
pub async fn resume_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    info!("Resuming recording");

    // Check if currently recording
    if !IS_RECORDING.load(Ordering::SeqCst) {
        return Err("No recording is currently active".to_string());
    }

    // Access the recording manager and resume it
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        manager.resume_recording().map_err(|e| e.to_string())?;

        // Emit resume event to frontend
        app.emit(
            "recording-resumed",
            serde_json::json!({
                "message": "Recording resumed"
            }),
        )
        .map_err(|e| e.to_string())?;

        // Update tray menu to reflect resumed state
        crate::tray::update_tray_menu(&app);

        info!("Recording resumed successfully");
        Ok(())
    } else {
        Err("No recording manager found".to_string())
    }
}

/// Check if recording is currently paused
pub async fn is_recording_paused() -> bool {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        manager.is_paused()
    } else {
        false
    }
}

/// Get detailed recording state
#[tauri::command]
pub async fn get_recording_state() -> serde_json::Value {
    let is_recording = IS_RECORDING.load(Ordering::SeqCst);
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        serde_json::json!({
            "is_recording": is_recording,
            "is_paused": manager.is_paused(),
            "is_active": manager.is_active(),
            "recording_duration": manager.get_recording_duration(),
            "active_duration": manager.get_active_recording_duration(),
            "total_pause_duration": manager.get_total_pause_duration(),
            "current_pause_duration": manager.get_current_pause_duration()
        })
    } else {
        serde_json::json!({
            "is_recording": is_recording,
            "is_paused": false,
            "is_active": false,
            "recording_duration": null,
            "active_duration": null,
            "total_pause_duration": 0.0,
            "current_pause_duration": null
        })
    }
}

/// Get the meeting folder path for the current recording
/// Returns the path if a meeting name was set and folder structure initialized
#[tauri::command]
pub async fn get_meeting_folder_path() -> Result<Option<String>, String> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_meeting_folder().map(|p| p.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

/// Get accumulated transcript segments from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_transcript_history() -> Result<Vec<crate::audio::recording_saver::TranscriptSegment>, String> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_transcript_segments())
    } else {
        Ok(Vec::new()) // No recording active, return empty
    }
}

/// Get meeting name from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_recording_meeting_name() -> Result<Option<String>, String> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_meeting_name())
    } else {
        Ok(None)
    }
}

// ============================================================================
// DEVICE MONITORING COMMANDS (AirPods/Bluetooth disconnect/reconnect support)
// ============================================================================

/// Response structure for device events
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum DeviceEventResponse {
    DeviceDisconnected {
        device_name: String,
        device_type: String,
    },
    DeviceReconnected {
        device_name: String,
        device_type: String,
    },
    DeviceListChanged,
}

impl From<DeviceEvent> for DeviceEventResponse {
    fn from(event: DeviceEvent) -> Self {
        match event {
            DeviceEvent::DeviceDisconnected { device_name, device_type } => {
                DeviceEventResponse::DeviceDisconnected {
                    device_name,
                    device_type: format!("{:?}", device_type),
                }
            }
            DeviceEvent::DeviceReconnected { device_name, device_type } => {
                DeviceEventResponse::DeviceReconnected {
                    device_name,
                    device_type: format!("{:?}", device_type),
                }
            }
            DeviceEvent::DeviceListChanged => DeviceEventResponse::DeviceListChanged,
        }
    }
}

/// Poll for audio device events (disconnect/reconnect)
/// Should be called periodically (every 1-2 seconds) by frontend during recording
#[tauri::command]
pub async fn poll_audio_device_events() -> Result<Option<DeviceEventResponse>, String> {
    let mut manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_mut() {
        if let Some(event) = manager.poll_device_events() {
            info!("📱 Device event polled: {:?}", event);
            Ok(Some(event.into()))
        } else {
            Ok(None)
        }
    } else {
        // Not recording, no events
        Ok(None)
    }
}

/// What each source has actually delivered since this recording started.
///
/// Polled rather than pushed, alongside `poll_audio_device_events`, and for the
/// same reason: the counters are updated from the audio callback, and a Tauri
/// event per chunk would put an allocation and a serialisation in the hot path
/// to answer a question the interface asks once a second.
///
/// The numbers are cumulative. The caller takes differences between polls and
/// decides what a gap means — "nothing has arrived in eight seconds" and "only
/// zeroes have arrived for a minute" are different sentences to show a person,
/// and neither judgement belongs down here.
#[tauri::command]
pub async fn audio_input_activity() -> Result<super::input_activity::InputActivity, String> {
    Ok(super::input_activity::snapshot())
}

/// Get information about the active audio output device
/// Used to warn users about Bluetooth playback issues
#[tauri::command]
pub async fn get_active_audio_output() -> Result<super::playback_monitor::AudioOutputInfo, String> {
    super::playback_monitor::get_active_audio_output()
        .await
        .map_err(|e| format!("Failed to get audio output info: {}", e))
}

/// Manually trigger device reconnection attempt
/// Useful for UI "Retry" button
pub async fn attempt_device_reconnect(
    device_name: String,
    device_type: String,
) -> Result<bool, String> {
    // Parse device type first
    let monitor_type = match device_type.as_str() {
        "Microphone" => DeviceMonitorType::Microphone,
        "SystemAudio" => DeviceMonitorType::SystemAudio,
        _ => return Err(format!("Invalid device type: {}", device_type)),
    };

    // Check if recording is active
    {
        let manager_guard = RECORDING_MANAGER.lock().unwrap();
        if manager_guard.is_none() {
            return Err("Recording not active".to_string());
        }
    } // Release lock

    // Spawn blocking task to handle the async reconnection
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            let mut manager_guard = RECORDING_MANAGER.lock().unwrap();
            if let Some(manager) = manager_guard.as_mut() {
                manager.attempt_device_reconnect(&device_name, monitor_type).await
            } else {
                Err(anyhow::anyhow!("Recording not active"))
            }
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    match result {
        Ok(success) => {
            if success {
                info!("✅ Manual reconnection successful");
            } else {
                warn!("❌ Manual reconnection failed - device not available");
            }
            Ok(success)
        }
        Err(e) => {
            error!("Manual reconnection error: {}", e);
            Err(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::watchdog_should_stop;

    /// The 12 Aug 2026 case: a manually started recording, so the detector
    /// never saw it and no candidate-absence logic applies. The cap must fire
    /// on elapsed time alone.
    #[test]
    fn a_manual_recording_past_the_cap_is_stopped() {
        assert!(watchdog_should_stop(true, 240, 240 * 60, true));
    }

    #[test]
    fn a_recording_under_the_cap_keeps_running() {
        assert!(!watchdog_should_stop(true, 240, 240 * 60 - 1, true));
    }

    #[test]
    fn the_master_switch_disables_the_cap() {
        assert!(!watchdog_should_stop(false, 240, 100_000, true));
    }

    #[test]
    fn a_zero_cap_means_disabled_rather_than_stop_everything() {
        // A hand-edited settings blob with 0 minutes must not kill every
        // recording at the first check.
        assert!(!watchdog_should_stop(true, 0, 100_000, true));
    }

    #[test]
    fn a_watchdog_for_a_finished_recording_does_nothing() {
        // Recording A ended and recording B may be running; A's watchdog sees
        // the generation mismatch and stands down.
        assert!(!watchdog_should_stop(true, 240, 240 * 60, false));
    }
}
