//! Turning detector events into recording decisions.
//!
//! The detector fires only on transitions of `DEVICE_IS_RUNNING_SOMEWHERE`, so
//! it cannot by itself tell us that a candidate has *persisted*. When a
//! candidate needs to survive the debounce window, this module schedules one
//! re-check rather than polling: it sleeps for the window and asks the hardware
//! again. Quiet machines therefore cost nothing.

use std::sync::Mutex;
use std::time::Instant;

use log::{error, info, warn};
use once_cell::sync::Lazy;
use tauri::{AppHandle, Manager, Runtime};

use crate::audio::system_detector::{snapshot_audio_apps, DetectedApp};
use crate::database::repositories::setting::SettingsRepository;
use crate::detection::policy::{
    evaluate, pick_candidate, Debouncer, Decision, DetectionConfig, ProposalLedger,
};
use crate::state::AppState;
use tauri::Emitter;

/// Process-start baseline for the monotonic clock the debouncer is fed with.
static CLOCK_ORIGIN: Lazy<Instant> = Lazy::new(Instant::now);

static DEBOUNCER: Lazy<Mutex<Debouncer>> = Lazy::new(|| Mutex::new(Debouncer::default()));

/// Guards against two overlapping re-checks for the same pending candidate.
static RECHECK_PENDING: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

/// The one stop proposal that may be outstanding, shared between the observer
/// (which opens it), the answer command, the confirmation timeout and the
/// meeting-resumed path (each of which may close it).
static STOP_LEDGER: Lazy<Mutex<ProposalLedger>> = Lazy::new(|| Mutex::new(ProposalLedger::default()));

/// How often the observer looks at audio activity while a recording runs.
///
/// Thirty seconds gives the absence clock a resolution well under the default
/// 120 s threshold while keeping the Core Audio enumeration rare.
const OBSERVER_INTERVAL_SECS: u64 = 30;

/// Payload of the `recording-stop-proposed` event.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StopProposalEvent {
    /// Identifies which proposal this is. The frontend echoes it back with
    /// the user's answer, so a dialog that outlives its proposal — the
    /// meeting resumed and went quiet again while it stood unanswered —
    /// cannot resolve one it was never shown.
    proposal_id: u64,
    /// How long the frontend has to answer before the recording stops anyway.
    timeout_seconds: u64,
}

/// Payload of the `recording-stop-proposal-resolved` event, emitted so an open
/// dialog can close no matter which path resolved the proposal.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StopProposalResolvedEvent {
    /// One of: "stopped", "kept", "timeout", "meeting-resumed".
    outcome: &'static str,
}

fn now_secs() -> u64 {
    CLOCK_ORIGIN.elapsed().as_secs()
}

/// Starts system-audio monitoring at application startup, when detection is
/// enabled in settings.
///
/// Nothing else starts the monitor — `start_system_audio_monitoring` is exposed
/// as a command but no caller invokes it — so without this the detector would
/// never run.
pub async fn autostart_if_enabled<R: Runtime>(app: AppHandle<R>) {
    let config = load_config(&app).await;

    // The auto-stop observer runs regardless of the `enabled` switch: that
    // switch governs *starting* recordings, while the observer watches for the
    // end of recordings the user started by hand too. It re-reads settings on
    // every tick, so auto_stop_enabled applies live.
    spawn_auto_stop_observer(app.clone());

    if !config.enabled {
        info!("Detection: disabled in settings, monitor not started");
        return;
    }

    let Some(detector_state) =
        app.try_state::<crate::audio::system_audio_commands::SystemAudioDetectorState>()
    else {
        warn!("Detection: audio detector state not managed, monitor not started");
        return;
    };

    match crate::audio::system_audio_commands::start_system_audio_monitoring(
        app.clone(),
        detector_state,
    )
    .await
    {
        Ok(()) => info!("Detection: system audio monitor started"),
        Err(e) => {
            error!("Detection: could not start the monitor: {}", e);
            return;
        }
    }

    // Evaluate what is already happening.
    //
    // The monitor only reports *transitions* of the output device. Starting the
    // application in the middle of a call produces no transition, so without
    // this the common case — quit the app, rejoin a meeting, reopen the app —
    // would never trigger anything.
    let apps = match tauri::async_runtime::spawn_blocking(snapshot_audio_apps).await {
        Ok(apps) => apps,
        Err(e) => {
            error!("Detection: initial snapshot failed: {}", e);
            return;
        }
    };

    if apps.is_empty() {
        info!("Detection: nothing is using audio at startup");
        return;
    }

    info!("Detection: {} application(s) already using audio at startup", apps.len());
    handle(app, apps, config, false).await;
}

/// Called when the output device reports silence.
///
/// Deliberately does **not** clear the pending candidate. During a real Teams
/// call the device flaps between running and stopped every second or two, and
/// discarding accumulated time on each blip means the confirmation window can
/// never be reached through events. Genuine silence is recognised instead by
/// the scheduled re-check, whose snapshot comes back empty and clears the
/// candidate through the normal path.
pub fn on_audio_stopped() {
    log::debug!("Detection: output device reported silence (candidate kept)");
}

/// Reads detection settings, falling back to defaults when the row is absent.
async fn load_config<R: Runtime>(app: &AppHandle<R>) -> DetectionConfig {
    let Some(state) = app.try_state::<AppState>() else {
        warn!("Detection: AppState not managed yet, using defaults");
        return DetectionConfig::default();
    };

    match SettingsRepository::get_detection_settings(state.db_manager.pool()).await {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Detection: could not read settings ({}), using defaults", e);
            DetectionConfig::default()
        }
    }
}

/// Handles one batch of applications reported by the detector.
pub async fn on_audio_started<R: Runtime>(app: AppHandle<R>, apps: Vec<DetectedApp>) {
    let config = load_config(&app).await;
    handle(app, apps, config, false).await;
}

/// Shared body for the initial event and for the delayed re-check.
async fn handle<R: Runtime>(
    app: AppHandle<R>,
    apps: Vec<DetectedApp>,
    config: DetectionConfig,
    is_recheck: bool,
) {
    let is_recording = crate::audio::recording_commands::is_recording().await;

    let decision = {
        let Ok(mut debouncer) = DEBOUNCER.lock() else {
            error!("Detection: debouncer mutex poisoned");
            return;
        };
        evaluate(
            &apps,
            &config,
            &mut debouncer,
            now_secs(),
            is_recording,
        )
    };

    match decision {
        Decision::Start { name, key, reason } => {
            info!("Detection: starting recording for '{}' ({})", name, reason.as_str());
            // Must be set before the streams open: the capture layer reads it
            // while building the tap, and after that the tap's shape is fixed.
            crate::audio::system_detector::set_meeting_scope(Some(key));
            start_recording(app, &name, config.show_notifications).await;
        }
        Decision::Waiting => {
            if !is_recheck {
                schedule_recheck(app, config);
            }
        }
        Decision::Ignore => {
            if is_recheck {
                info!("Detection: candidate disappeared before the window elapsed");
            }
        }
        Decision::Stop => {
            propose_stop(app, config).await;
        }
        Decision::AlreadyRecording => {
            // A pending proposal is withdrawn the moment the meeting audibly
            // resumes. Without this, the confirmation timeout would cut off a
            // meeting that came back while nobody was at the machine — the one
            // outcome this flow must never produce.
            let resolution = {
                let Ok(mut ledger) = STOP_LEDGER.lock() else { return };
                let candidate_present = pick_candidate(&apps, &config).is_some();
                ledger.withdraw_for_resumed_meeting(candidate_present)
            };
            if resolution.was_pending {
                info!("Detection: the meeting resumed — stop proposal withdrawn");
                // Capture comes back with the meeting, but only if this
                // proposal is what paused it. The observer runs every
                // OBSERVER_INTERVAL_SECS, so up to that much of the returning
                // meeting is missed — the price of not recording whatever
                // played while the question stood.
                if resolution.owns_pause {
                    resume_capture_if_still_paused(&app, "the meeting returned").await;
                }
                let _ = app.emit(
                    "recording-stop-proposal-resolved",
                    StopProposalResolvedEvent { outcome: "meeting-resumed" },
                );
            }
        }
        Decision::Disabled => {}
    }
}

/// Emits a stop proposal to the frontend and arms its confirmation timeout.
///
/// The proposal asks; it does not decide. The recording stops only when the
/// user confirms, or when the timeout below expires unanswered — no answer
/// means nobody is at the machine, which is exactly the situation that once
/// produced a seven-hour recording of an empty room.
async fn propose_stop<R: Runtime>(app: AppHandle<R>, config: DetectionConfig) {
    let id = {
        let Ok(mut ledger) = STOP_LEDGER.lock() else {
            error!("Detection: stop ledger mutex poisoned");
            return;
        };
        ledger.open()
    };

    // Mute capture for as long as the question stands. The system tap is
    // global — it carries whatever the machine plays, not just the meeting —
    // so anything started while the dialog waits would otherwise land in the
    // recording and in the transcript. The claim is taken only when this call
    // is what paused capture: a recording the user paused by hand stays theirs.
    match crate::audio::recording_commands::pause_recording(app.clone()).await {
        Ok(()) => {
            if let Ok(mut ledger) = STOP_LEDGER.lock() {
                ledger.claim_pause();
            }
            info!("Detection: capture paused while the stop proposal stands");
        }
        Err(e) => info!("Detection: capture was not paused by this proposal ({})", e),
    }

    let timeout_seconds = config.confirmation_timeout_seconds;
    info!(
        "Detection: meeting appears to be over — proposing to stop ({}s to answer)",
        timeout_seconds
    );

    let _ = app.emit(
        "recording-stop-proposed",
        StopProposalEvent { proposal_id: id, timeout_seconds },
    );

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(timeout_seconds)).await;

        let still_pending = STOP_LEDGER
            .lock()
            .map(|mut ledger| ledger.expire(id))
            .unwrap_or(false);
        if !still_pending {
            // Answered, withdrawn or superseded while we slept.
            return;
        }

        let _ = app.emit(
            "recording-stop-proposal-resolved",
            StopProposalResolvedEvent { outcome: "timeout" },
        );

        if !crate::audio::recording_commands::is_recording().await {
            return;
        }

        warn!(
            "Detection: stop proposal unanswered for {}s — stopping the recording",
            timeout_seconds
        );
        match crate::audio::recording_commands::stop_recording(
            app.clone(),
            crate::audio::recording_commands::RecordingArgs { save_path: String::new() },
        )
        .await
        {
            Ok(()) => crate::audio::recording_commands::notify_frontend_stop_complete(&app),
            Err(e) => error!("Detection: stopping after an unanswered proposal failed: {}", e),
        }
    });
}

/// Brings back capture that a stop proposal paused, unless it is already back.
///
/// The tray menu offers Resume while the dialog stands, so the user may have
/// lifted the pause by hand. Resuming again would only fail and log an error
/// about a situation that is perfectly fine.
async fn resume_capture_if_still_paused<R: Runtime>(app: &AppHandle<R>, context: &str) {
    if !crate::audio::recording_commands::is_recording_paused().await {
        info!("Detection: capture was already resumed by hand ({})", context);
        return;
    }

    match crate::audio::recording_commands::resume_recording(app.clone()).await {
        Ok(()) => info!("Detection: capture resumed ({})", context),
        Err(e) => error!("Detection: resuming capture failed ({}): {}", context, e),
    }
}

/// Handles the user's answer to a stop proposal. Called by the Tauri command.
///
/// `proposal_id` is the id the dialog was opened with. An answer whose id no
/// longer matches the pending proposal — the timeout, the meeting resuming,
/// or a fresh proposal opened in between — is stale and resolves nothing,
/// even if some other proposal happens to be pending right now.
pub async fn respond_to_stop_proposal<R: Runtime>(app: AppHandle<R>, proposal_id: u64, accept: bool) {
    let resolution = STOP_LEDGER
        .lock()
        .map(|mut ledger| ledger.answer(proposal_id))
        .unwrap_or_default();

    if !resolution.was_pending {
        // A stale dialog: the timeout, the resumed meeting, or a fresh
        // proposal got here first.
        info!("Detection: stop-proposal answer arrived for a proposal that is no longer pending");
        return;
    }

    if accept {
        info!("Detection: user confirmed — stopping the recording");
        let _ = app.emit(
            "recording-stop-proposal-resolved",
            StopProposalResolvedEvent { outcome: "stopped" },
        );
        match crate::audio::recording_commands::stop_recording(
            app.clone(),
            crate::audio::recording_commands::RecordingArgs { save_path: String::new() },
        )
        .await
        {
            Ok(()) => crate::audio::recording_commands::notify_frontend_stop_complete(&app),
            Err(e) => error!("Detection: stopping on user confirmation failed: {}", e),
        }
    } else {
        // Keep recording. The policy keeps this absence episode marked as
        // already-proposed, so the next question can only come after the
        // meeting resumes and goes quiet again.
        info!("Detection: user chose to keep recording");
        if resolution.owns_pause {
            resume_capture_if_still_paused(&app, "user kept recording").await;
        }
        let _ = app.emit(
            "recording-stop-proposal-resolved",
            StopProposalResolvedEvent { outcome: "kept" },
        );
    }
}

/// Feeds the absence clock while a recording runs.
///
/// The detector is event-driven and silence produces no events: once the
/// meeting application quits, nothing would ever reach `evaluate` again and
/// the absence clock would never advance. This loop supplies those
/// observations — and only while they can matter. Outside a recording each
/// tick checks one flag and goes back to sleep, keeping quiet machines cheap.
fn spawn_auto_stop_observer<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(OBSERVER_INTERVAL_SECS)).await;

            if !crate::audio::recording_commands::is_recording().await {
                continue;
            }

            let config = load_config(&app).await;
            if !config.auto_stop_enabled {
                continue;
            }

            // Enumerating Core Audio blocks, so keep it off the async runtime.
            let apps = match tauri::async_runtime::spawn_blocking(snapshot_audio_apps).await {
                Ok(apps) => apps,
                Err(e) => {
                    error!("Detection: auto-stop observation failed: {}", e);
                    continue;
                }
            };

            // is_recheck = true: the observer must never schedule start-side
            // re-checks of its own.
            handle(app.clone(), apps, config, true).await;
        }
    });
}

/// Sleeps for the debounce window, then asks the hardware again.
fn schedule_recheck<R: Runtime>(app: AppHandle<R>, config: DetectionConfig) {
    {
        let Ok(mut pending) = RECHECK_PENDING.lock() else {
            return;
        };
        if *pending {
            return; // one re-check in flight is enough
        }
        *pending = true;
    }

    let delay = std::time::Duration::from_secs(config.min_duration_seconds.max(1));

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;

        // Enumerating Core Audio blocks, so keep it off the async runtime.
        let apps = match tauri::async_runtime::spawn_blocking(snapshot_audio_apps).await {
            Ok(apps) => apps,
            Err(e) => {
                error!("Detection: re-check task failed: {}", e);
                Vec::new()
            }
        };

        if let Ok(mut pending) = RECHECK_PENDING.lock() {
            *pending = false;
        }

        handle(app, apps, config, true).await;
    });
}

async fn start_recording<R: Runtime>(app: AppHandle<R>, app_name: &str, notify: bool) {
    // The calendar knows what this meeting is called; the detector only knows
    // which app is making noise. So the app name goes in as the fallback, not as
    // the name — that lets the calendar win, and keeps the lookup in one place
    // where its participants and agenda can reach the recording's metadata.
    let fallback_name = format!("{} — auto", app_name);

    // `start_recording_with_devices_and_meeting(.., None, None, ..)` does not
    // mean "use the defaults", it means "no devices", and the pipeline then
    // fails with "No audio streams could be created". This variant resolves the
    // default input and output itself.
    match crate::audio::recording_commands::start_recording_with_meeting_name(
        app.clone(),
        None,
        Some(fallback_name.clone()),
        // The app name travels as data here, not only inside the fallback
        // title: the calendar is allowed to replace that title, and the
        // meetings list needs to know which application was captured.
        crate::audio::recording_saver::RecordingOrigin {
            source: crate::audio::recording_saver::RecordingSource::Auto,
            app_name: Some(app_name.to_string()),
        },
    )
    .await
    {
        Ok(()) => {
            // The calendar may have supplied a better name than our fallback, so
            // ask what the recording actually ended up being called. The user then
            // sees the real meeting title in the notification, not the app name.
            let actual_name = crate::audio::recording_commands::get_recording_meeting_name()
                .await
                .ok()
                .flatten()
                .unwrap_or(fallback_name);

            info!("Detection: recording started automatically as '{}'", actual_name);
            crate::tray::update_tray_menu(&app);

            if notify {
                let notification_state = app
                    .try_state::<crate::notifications::commands::NotificationManagerState<R>>();

                if let Some(state) = notification_state {
                    if let Err(e) = crate::notifications::commands::show_recording_started_notification(
                        &app,
                        &state,
                        Some(actual_name),
                    )
                    .await
                    {
                        warn!("Detection: could not show notification: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            error!("Detection: automatic recording failed to start: {}", e);
            // Nothing is recording, so nothing will call stop to release it.
            // Left standing, this scope would silently narrow the next
            // recording — including one the user starts by hand.
            crate::audio::system_detector::set_meeting_scope(None);
        }
    }
}
