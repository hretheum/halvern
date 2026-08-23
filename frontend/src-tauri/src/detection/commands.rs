//! Tauri commands for automatic meeting detection.

use log::info as log_info;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::database::repositories::setting::SettingsRepository;
use crate::detection::policy::DetectionConfig;
use crate::state::AppState;

/// Detection settings as exchanged with the frontend.
///
/// The auto-stop fields carry serde defaults so a payload from a frontend
/// build that predates them still deserialises.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSettings {
    pub enabled: bool,
    #[serde(rename = "ignoredBundleIds")]
    pub ignored_bundle_ids: Vec<String>,
    #[serde(rename = "alwaysMeetingBundleIds")]
    pub always_meeting_bundle_ids: Vec<String>,
    #[serde(rename = "minDurationSeconds")]
    pub min_duration_seconds: u64,
    #[serde(rename = "showNotifications")]
    pub show_notifications: bool,
    #[serde(rename = "autoStopEnabled", default = "default_auto_stop_enabled")]
    pub auto_stop_enabled: bool,
    #[serde(rename = "silenceDurationSeconds", default = "default_silence_duration_seconds")]
    pub silence_duration_seconds: u64,
    #[serde(
        rename = "confirmationTimeoutSeconds",
        default = "default_confirmation_timeout_seconds"
    )]
    pub confirmation_timeout_seconds: u64,
    #[serde(rename = "maxRecordingMinutes", default = "default_max_recording_minutes")]
    pub max_recording_minutes: u64,
}

fn default_auto_stop_enabled() -> bool {
    DetectionConfig::default().auto_stop_enabled
}

fn default_silence_duration_seconds() -> u64 {
    DetectionConfig::default().silence_duration_seconds
}

fn default_confirmation_timeout_seconds() -> u64 {
    DetectionConfig::default().confirmation_timeout_seconds
}

fn default_max_recording_minutes() -> u64 {
    DetectionConfig::default().max_recording_minutes
}

impl From<DetectionConfig> for DetectionSettings {
    fn from(c: DetectionConfig) -> Self {
        Self {
            enabled: c.enabled,
            ignored_bundle_ids: c.ignored_bundle_ids,
            always_meeting_bundle_ids: c.always_meeting_bundle_ids,
            min_duration_seconds: c.min_duration_seconds,
            show_notifications: c.show_notifications,
            auto_stop_enabled: c.auto_stop_enabled,
            silence_duration_seconds: c.silence_duration_seconds,
            confirmation_timeout_seconds: c.confirmation_timeout_seconds,
            max_recording_minutes: c.max_recording_minutes,
        }
    }
}

impl From<DetectionSettings> for DetectionConfig {
    fn from(s: DetectionSettings) -> Self {
        Self {
            enabled: s.enabled,
            ignored_bundle_ids: s.ignored_bundle_ids,
            always_meeting_bundle_ids: s.always_meeting_bundle_ids,
            min_duration_seconds: s.min_duration_seconds,
            show_notifications: s.show_notifications,
            auto_stop_enabled: s.auto_stop_enabled,
            silence_duration_seconds: s.silence_duration_seconds,
            confirmation_timeout_seconds: s.confirmation_timeout_seconds,
            max_recording_minutes: s.max_recording_minutes,
        }
    }
}


#[tauri::command]
pub async fn api_get_detection_settings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<DetectionSettings, String> {
    let config = SettingsRepository::get_detection_settings(state.db_manager.pool())
        .await
        .map_err(|e| format!("Reading the detection settings failed: {}", e))?;

    Ok(config.into())
}

#[tauri::command]
pub async fn api_save_detection_settings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    settings: DetectionSettings,
) -> Result<(), String> {
    log_info!(
        "api_save_detection_settings called: enabled={}, min_duration={}s",
        settings.enabled,
        settings.min_duration_seconds
    );

    if settings.min_duration_seconds > 600 {
        return Err("The confirmation window must not exceed 600 s".to_string());
    }
    if settings.silence_duration_seconds > 3600 {
        return Err("The silence window must not exceed 3600 s".to_string());
    }
    if settings.confirmation_timeout_seconds > 3600 {
        return Err("The answer window must not exceed 3600 s".to_string());
    }
    // 0 is a valid cap: it means the hard limit is off.
    if settings.max_recording_minutes > 24 * 60 {
        return Err("The maximum recording length must not exceed 24 hours".to_string());
    }

    let config: DetectionConfig = settings.into();

    SettingsRepository::save_detection_settings(state.db_manager.pool(), &config)
        .await
        .map_err(|e| format!("Writing the detection settings failed: {}", e))
}


/// The user's answer to a stop proposal shown by the frontend.
///
/// `proposal_id` is the id the frontend received on `recording-stop-proposed`
/// and must echo back unchanged. `accept == true` stops the recording;
/// `false` keeps it running, and the next proposal can only come after the
/// meeting resumes and goes quiet again. A stale answer — the id no longer
/// matches the pending proposal because the timeout, a resumed meeting, or a
/// fresh proposal resolved it first — is ignored.
#[tauri::command]
pub async fn api_respond_to_stop_proposal<R: Runtime>(
    app: AppHandle<R>,
    proposal_id: u64,
    accept: bool,
) -> Result<(), String> {
    log_info!(
        "api_respond_to_stop_proposal called: proposal_id={}, accept={}",
        proposal_id,
        accept
    );
    crate::detection::service::respond_to_stop_proposal(app, proposal_id, accept).await;
    Ok(())
}
