use tauri::{command, AppHandle, Emitter, Runtime, State};
use crate::audio::{
    start_system_audio_capture, list_system_audio_devices, check_system_audio_permissions,
    SystemAudioDetector, SystemAudioEvent, new_system_audio_callback
};
use std::sync::{Arc, Mutex};
use anyhow::Result;

// Global state for system audio detector.
// Public so that meeting detection can start monitoring at application startup
// without going through the frontend.
pub type SystemAudioDetectorState = Arc<Mutex<Option<SystemAudioDetector>>>;

/// Start system audio capture (for capturing system output audio)
#[command]
pub async fn start_system_audio_capture_command() -> Result<String, String> {
    match start_system_audio_capture().await {
        Ok(_stream) => {
            // TODO: Store the stream in global state if needed for management
            Ok("System audio capture started successfully".to_string())
        }
        Err(e) => Err(format!("Failed to start system audio capture: {}", e))
    }
}

/// List available system audio devices
pub async fn list_system_audio_devices_command() -> Result<Vec<String>, String> {
    list_system_audio_devices()
        .map_err(|e| format!("Failed to list system audio devices: {}", e))
}

/// Check if the app has permission to access system audio
pub async fn check_system_audio_permissions_command() -> bool {
    check_system_audio_permissions()
}

/// Start monitoring system audio usage by other applications
pub async fn start_system_audio_monitoring<R: Runtime>(
    app_handle: AppHandle<R>,
    detector_state: State<'_, SystemAudioDetectorState>
) -> Result<(), String> {
    let mut detector_guard = detector_state.lock()
        .map_err(|e| format!("Failed to acquire detector lock: {}", e))?;

    if detector_guard.is_some() {
        return Err("System audio monitoring is already active".to_string());
    }

    let mut detector = SystemAudioDetector::new();

    // Emit to the frontend and hand the same observation to meeting detection.
    // The callback is synchronous and runs on a Core Audio listener thread, so
    // the decision work is moved onto the async runtime rather than blocking it.
    let callback = new_system_audio_callback(move |event| {
        match event {
            SystemAudioEvent::SystemAudioStarted(apps) => {
                tracing::info!("System audio started by apps: {:?}", apps);
                let _ = app_handle.emit("system-audio-started", apps.clone());

                let handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    crate::detection::service::on_audio_started(handle, apps).await;
                });
            }
            SystemAudioEvent::SystemAudioStopped => {
                let _ = app_handle.emit("system-audio-stopped", ());
                tracing::info!("System audio stopped");
                crate::detection::service::on_audio_stopped();
            }
        }
    });

    detector.start(callback);
    *detector_guard = Some(detector);

    Ok(())
}

/// Initialize the system audio detector state in Tauri app
pub fn init_system_audio_state() -> SystemAudioDetectorState {
    Arc::new(Mutex::new(None))
}

// Event payload types for frontend
#[derive(serde::Serialize, Clone)]
pub struct SystemAudioStartedPayload {
    pub apps: Vec<String>,
}

#[derive(serde::Serialize, Clone)]
pub struct SystemAudioStoppedPayload;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_system_audio_devices() {
        let devices = list_system_audio_devices_command().await;
        match devices {
            Ok(device_list) => {
                println!("System audio devices: {:?}", device_list);
                assert!(device_list.len() >= 0); // Should at least not crash
            }
            Err(e) => {
                println!("Error listing devices: {}", e);
                // This might fail on CI or systems without audio
            }
        }
    }

    #[tokio::test]
    async fn test_check_permissions() {
        let has_permission = check_system_audio_permissions_command().await;
        println!("Has system audio permissions: {}", has_permission);
        // This is mainly a smoke test to ensure it doesn't crash
    }
}