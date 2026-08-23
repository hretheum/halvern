use std::sync::Arc;
use tokio::sync::mpsc;
use anyhow::Result;
use log::{debug, error, info, warn};

use super::devices::{AudioDevice, list_audio_devices};

#[cfg(target_os = "macos")]

#[cfg(not(target_os = "macos"))]
use super::devices::{default_input_device, default_output_device};
use super::device_detection::InputDeviceKind;
use super::recording_state::{RecordingState, AudioChunk, DeviceType as RecordingDeviceType};
use super::pipeline::AudioPipelineManager;
use super::stream::AudioStreamManager;
use super::recording_saver::RecordingSaver;
use super::device_monitor::{AudioDeviceMonitor, DeviceEvent, DeviceMonitorType};

/// Stream manager type enumeration
pub enum StreamManagerType {
    Standard(AudioStreamManager),
}

/// Simplified recording manager that coordinates all audio components
pub struct RecordingManager {
    state: Arc<RecordingState>,
    stream_manager: AudioStreamManager,
    pipeline_manager: AudioPipelineManager,
    recording_saver: RecordingSaver,
    device_monitor: Option<AudioDeviceMonitor>,
    device_event_receiver: Option<mpsc::UnboundedReceiver<DeviceEvent>>,
}

// SAFETY: RecordingManager contains types that we've marked as Send
unsafe impl Send for RecordingManager {}

impl RecordingManager {
    /// Create a new recording manager
    pub fn new() -> Self {
        let state = RecordingState::new();
        let stream_manager = AudioStreamManager::new(state.clone());
        let pipeline_manager = AudioPipelineManager::new();
        let (device_monitor, device_event_receiver) = AudioDeviceMonitor::new();

        Self {
            state,
            stream_manager,
            pipeline_manager,
            recording_saver: RecordingSaver::new(),
            device_monitor: Some(device_monitor),
            device_event_receiver: Some(device_event_receiver),
        }
    }

    // Remove app handle storage for now - will be passed directly when saving

    /// Start recording with specified devices
    ///
    /// # Arguments
    /// * `microphone_device` - Optional microphone device to use
    /// * `system_device` - Optional system audio device to use
    /// * `auto_save` - Whether to save audio checkpoints (true) or just transcripts/metadata (false)
    pub async fn start_recording(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
        auto_save: bool,
        save_raw_sources: bool,
    ) -> Result<mpsc::UnboundedReceiver<AudioChunk>> {
        info!(
            "Starting recording manager (auto_save: {}, raw sources: {})",
            auto_save, save_raw_sources
        );

        // Set up transcription channel
        let (transcription_sender, transcription_receiver) = mpsc::unbounded_channel::<AudioChunk>();

        // CRITICAL FIX: Create recording sender for pre-mixed audio from pipeline
        // Pipeline will mix mic + system audio professionally and send to this channel
        // Pass auto_save to control whether audio checkpoints are created
        let recording_sender = self.recording_saver.start_accumulation(auto_save);

        // The per-source raw tap is diagnostic: it lets a bad recording be
        // replayed through changed code instead of waiting for another live
        // meeting. It also writes 5.6 MB per minute per source, so it stays off
        // unless someone is actually investigating something.
        if save_raw_sources {
            match self.recording_saver.get_meeting_folder() {
                Some(folder) => super::raw_tap::begin(folder),
                // The folder exists whether or not auto-save is on — the saver
                // creates it for the transcript either way — so the only way
                // to get here is a recording started without a meeting name.
                None => info!("Raw tap not armed: no meeting folder (recording has no meeting name)"),
            }
        }

        // Start recording state first
        self.state.start_recording()?;

        // Get device information for adaptive mixing
        // The pipeline uses device kind (Bluetooth vs Wired) to apply adaptive buffering:
        // - Bluetooth: Larger buffers (80-200ms) to handle jitter
        // - Wired: Smaller buffers (20-50ms) for low latency
        let (mic_name, mic_kind) =
            describe_capture_source(microphone_device.as_ref(), "No Microphone");
        let (sys_name, sys_kind) =
            describe_capture_source(system_device.as_ref(), "No System Audio");

        // Update recording metadata with device information
        self.recording_saver.set_device_info(
            microphone_device.as_ref().map(|d| d.name.clone()),
            system_device.as_ref().map(|d| d.name.clone())
        );

        // Start the audio processing pipeline with FFmpeg adaptive mixer
        // Pipeline will: 1) Mix mic+system audio with adaptive buffering, 2) Send mixed to recording_sender,
        // 3) Apply VAD and send speech segments to transcription
        self.pipeline_manager.start(
            self.state.clone(),
            transcription_sender,
            0, // Ignored - using dynamic sizing internally
            48000, // 48kHz sample rate
            Some(recording_sender), // CRITICAL: Pass recording sender to receive pre-mixed audio
            mic_name,
            mic_kind,
            sys_name,
            sys_kind,
        )?;

        // Give the pipeline a moment to fully initialize before starting streams
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Start audio streams - they send RAW unmixed chunks to pipeline for mixing
        // Pipeline handles mixing and distribution to both recording and transcription
        self.stream_manager.start_streams(microphone_device.clone(), system_device.clone()).await?;

        // Start device monitoring to detect disconnects
        if let Some(ref mut monitor) = self.device_monitor {
            if let Err(e) = monitor.start_monitoring(microphone_device, system_device) {
                warn!("Failed to start device monitoring: {}", e);
                // Non-fatal - continue without monitoring
            } else {
                info!("✅ Device monitoring started");
            }
        }

        info!("Recording manager started successfully with {} active streams",
               self.stream_manager.active_stream_count());

        Ok(transcription_receiver)
    }


    /// Stop recording streams without saving (for use when waiting for transcription)
    pub async fn stop_streams_only(&mut self) -> Result<()> {
        info!("Stopping recording streams only");

        // Stop device monitoring
        if let Some(ref mut monitor) = self.device_monitor {
            monitor.stop_monitoring().await;
        }

        // Stop recording state first
        self.state.stop_recording();

        // Stop audio streams
        if let Err(e) = self.stream_manager.stop_streams() {
            error!("Error stopping audio streams: {}", e);
        }

        // Stop audio pipeline
        if let Err(e) = self.pipeline_manager.stop().await {
            error!("Error stopping audio pipeline: {}", e);
        }

        debug!("Recording streams stopped successfully");
        Ok(())
    }

    /// Stop streams and force immediate pipeline flush to process all accumulated audio
    pub async fn stop_streams_and_force_flush(&mut self) -> Result<()> {
        info!("🚀 Stopping recording streams with IMMEDIATE pipeline flush");

        // CRITICAL: Stop device monitor FIRST to prevent continuous WASAPI polling on Windows
        // This fixes the slow shutdown issue where device enumeration runs for 90+ seconds
        if let Some(ref mut monitor) = self.device_monitor {
            info!("Stopping device monitor first...");
            monitor.stop_monitoring().await;
        }

        // Stop recording state first - this clears device references
        self.state.stop_recording();

        // Stop audio streams immediately
        if let Err(e) = self.stream_manager.stop_streams() {
            error!("Error stopping audio streams: {}", e);
        }

        // CRITICAL: Force pipeline to flush ALL accumulated audio before stopping
        debug!("💨 Forcing pipeline to flush accumulated audio immediately");
        if let Err(e) = self.pipeline_manager.force_flush_and_stop().await {
            error!("Error during force flush: {}", e);
        }

        // CRITICAL: Full cleanup to release all Arc references and resources
        // This ensures microphone is released even if Drop is delayed
        self.state.cleanup();

        info!("✅ Recording streams stopped with immediate flush completed");
        Ok(())
    }

    /// Save recording after transcription is complete
    pub async fn save_recording_only<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        debug!("Saving recording with transcript chunks");

        // Get actual recording duration from state
        let recording_duration = self.state.get_active_recording_duration();
        info!("Recording duration from state: {:?}s", recording_duration);

        // Save the recording with actual duration
        match self.recording_saver.stop_and_save(app, recording_duration).await {
            Ok(Some(file_path)) => {
                info!("Recording saved successfully to: {}", file_path);
            }
            Ok(None) => {
                debug!("Recording not saved (auto-save disabled or no audio data)");
            }
            Err(e) => {
                error!("Failed to save recording: {}", e);
                // Don't fail the stop operation if saving fails
            }
        }

        debug!("Recording save operation completed");
        Ok(())
    }

    /// Stop recording and save audio (legacy method)
    pub async fn stop_recording<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        info!("Stopping recording manager");

        // Close the raw files and report their durations. Two sources that came
        // out different lengths is the clearest single sign of stream drift.
        super::raw_tap::finish();

        // Get recording duration BEFORE stopping (important!)
        let recording_duration = self.state.get_active_recording_duration();
        info!("Recording duration before stop: {:?}s", recording_duration);

        // Stop recording state first
        self.state.stop_recording();

        // Stop audio streams
        if let Err(e) = self.stream_manager.stop_streams() {
            error!("Error stopping audio streams: {}", e);
        }

        // Stop audio pipeline
        if let Err(e) = self.pipeline_manager.stop().await {
            error!("Error stopping audio pipeline: {}", e);
        }

        // Save the recording with actual duration
        match self.recording_saver.stop_and_save(app, recording_duration).await {
            Ok(Some(file_path)) => {
                info!("Recording saved successfully to: {}", file_path);
            }
            Ok(None) => {
                info!("Recording not saved (auto-save disabled or no audio data)");
            }
            Err(e) => {
                error!("Failed to save recording: {}", e);
                // Don't fail the stop operation if saving fails
            }
        }

        info!("Recording manager stopped");
        Ok(())
    }

    /// Get recording stats from the saver
    pub fn get_recording_stats(&self) -> (usize, u32) {
        self.recording_saver.get_stats()
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.state.is_recording()
    }

    /// Pause the current recording session
    pub fn pause_recording(&self) -> Result<()> {
        info!("Pausing recording");
        self.state.pause_recording()
    }

    /// Resume the current recording session
    pub fn resume_recording(&self) -> Result<()> {
        info!("Resuming recording");
        self.state.resume_recording()
    }

    /// Check if recording is currently paused
    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }

    /// Check if recording is active (recording and not paused)
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// Get recording statistics
    pub fn get_stats(&self) -> super::recording_state::RecordingStats {
        self.state.get_stats()
    }

    /// Get recording duration
    pub fn get_recording_duration(&self) -> Option<f64> {
        self.state.get_recording_duration()
    }

    /// Get active recording duration (excluding pauses)
    pub fn get_active_recording_duration(&self) -> Option<f64> {
        self.state.get_active_recording_duration()
    }

    /// Get total pause duration
    pub fn get_total_pause_duration(&self) -> f64 {
        self.state.get_total_pause_duration()
    }

    /// Get current pause duration if paused
    pub fn get_current_pause_duration(&self) -> Option<f64> {
        self.state.get_current_pause_duration()
    }

    /// Get error information
    pub fn get_error_info(&self) -> (u32, Option<super::recording_state::AudioError>) {
        (self.state.get_error_count(), self.state.get_last_error())
    }

    /// Get active stream count
    pub fn active_stream_count(&self) -> usize {
        self.stream_manager.active_stream_count()
    }

    /// Set error callback for handling errors
    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&super::recording_state::AudioError) + Send + Sync + 'static,
    {
        self.state.set_error_callback(callback);
    }

    /// Check if there's a fatal error
    pub fn has_fatal_error(&self) -> bool {
        self.state.has_fatal_error()
    }

    /// Set the meeting name for this recording session
    pub fn set_meeting_name(&mut self, name: Option<String>) {
        self.recording_saver.set_meeting_name(name);
    }

    /// Sets the directory recordings are written under; `None` uses the default.
    pub fn set_recordings_root(&mut self, root: Option<std::path::PathBuf>) {
        self.recording_saver.set_recordings_root(root);
    }

    /// Record which calendar event this recording belongs to, if any.
    ///
    /// Separate from the name because the two can disagree: the user may have
    /// typed their own title while the calendar still knows the participants and
    /// the agenda.
    pub fn set_calendar_match(
        &mut self,
        matched: Option<crate::calendar::CalendarEvent>,
    ) {
        self.recording_saver.set_calendar_match(matched);
    }

    /// Record how this recording started, so the meetings list can filter by
    /// source and name the capturing application.
    pub fn set_origin(&mut self, origin: super::recording_saver::RecordingOrigin) {
        self.recording_saver.set_origin(origin);
    }

    /// Add a structured transcript segment to be saved later
    pub fn add_transcript_segment(&self, segment: super::recording_saver::TranscriptSegment) {
        self.recording_saver.add_transcript_segment(segment);
    }

    /// Add a transcript chunk to be saved later (legacy method)
    pub fn add_transcript_chunk(&self, text: String) {
        self.recording_saver.add_transcript_chunk(text);
    }

    /// Get accumulated transcript segments from current recording session
    /// Used for syncing frontend state after page reload during active recording
    pub fn get_transcript_segments(&self) -> Vec<super::recording_saver::TranscriptSegment> {
        self.recording_saver.get_transcript_segments()
    }

    /// Get meeting name from current recording session
    /// Used for syncing frontend state after page reload during active recording
    pub fn get_meeting_name(&self) -> Option<String> {
        self.recording_saver.get_meeting_name()
    }

    /// Cleanup all resources without saving
    pub async fn cleanup_without_save(&mut self) {
        if self.is_recording() {
            debug!("Stopping recording without saving during cleanup");

            // Stop recording state first
            self.state.stop_recording();

            // Stop audio streams
            if let Err(e) = self.stream_manager.stop_streams() {
                error!("Error stopping audio streams during cleanup: {}", e);
            }

            // Stop audio pipeline
            if let Err(e) = self.pipeline_manager.stop().await {
                error!("Error stopping audio pipeline during cleanup: {}", e);
            }
        }
        self.state.cleanup();
    }

    /// Get the meeting folder path (if available)
    /// Returns None if no meeting name was set or folder structure not initialized
    pub fn get_meeting_folder(&self) -> Option<std::path::PathBuf> {
        self.recording_saver.get_meeting_folder().cloned()
    }

    /// Check for device events (disconnects/reconnects)
    /// Returns Some(DeviceEvent) if an event occurred, None otherwise
    pub fn poll_device_events(&mut self) -> Option<DeviceEvent> {
        if let Some(ref mut receiver) = self.device_event_receiver {
            receiver.try_recv().ok()
        } else {
            None
        }
    }

    /// Attempt to reconnect a disconnected device
    /// Returns true if reconnection successful
    pub async fn attempt_device_reconnect(&mut self, device_name: &str, device_type: DeviceMonitorType) -> Result<bool> {
        info!("🔄 Attempting to reconnect device: {} ({:?})", device_name, device_type);

        // List current devices
        let available_devices = list_audio_devices().await?;

        let Some(plan) = plan_reconnect(
            &available_devices,
            device_name,
            device_type,
            self.state.get_microphone_device(),
            self.state.get_system_device(),
        ) else {
            warn!("❌ Device '{}' not yet available", device_name);
            return Ok(false);
        };

        info!("✅ Device '{}' found, recreating stream...", device_name);
        let ReconnectPlan { microphone, system, reconnected, role } = plan;

        // Both streams go down and come back up together — the stream manager
        // has no way to replace one of them on its own.
        self.stream_manager.stop_streams()?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        self.stream_manager.start_streams(microphone, system).await?;

        match role {
            DeviceMonitorType::Microphone => {
                self.state.set_microphone_device(reconnected);
                info!("✅ Microphone reconnected successfully");
            }
            DeviceMonitorType::SystemAudio => {
                self.state.set_system_device(reconnected);
                info!("✅ System audio reconnected successfully");
            }
        }

        Ok(true)
    }

    /// Handle a device disconnect event
    /// Pauses recording and attempts reconnection
    pub async fn handle_device_disconnect(&mut self, device_name: String, device_type: DeviceMonitorType) {
        warn!("📱 Device disconnected: {} ({:?})", device_name, device_type);

        // Mark state as reconnecting (keeps recording alive but in waiting state)
        let device = match device_type {
            DeviceMonitorType::Microphone => self.state.get_microphone_device(),
            DeviceMonitorType::SystemAudio => self.state.get_system_device(),
        };

        if let Some(device) = device {
            let recording_device_type = match device_type {
                DeviceMonitorType::Microphone => RecordingDeviceType::Microphone,
                DeviceMonitorType::SystemAudio => RecordingDeviceType::System,
            };
            self.state.start_reconnecting(device, recording_device_type);
        }
    }

    /// Handle a device reconnect event
    pub async fn handle_device_reconnect(&mut self, device_name: String, device_type: DeviceMonitorType) -> Result<()> {
        info!("📱 Device reconnected: {} ({:?})", device_name, device_type);

        // Attempt to reconnect the device
        match self.attempt_device_reconnect(&device_name, device_type).await {
            Ok(true) => {
                info!("✅ Successfully reconnected device: {}", device_name);
                self.state.stop_reconnecting();
                Ok(())
            }
            Ok(false) => {
                warn!("Device reconnect attempt failed (device not yet available)");
                Err(anyhow::anyhow!("Device not available"))
            }
            Err(e) => {
                error!("Device reconnect failed: {}", e);
                Err(e)
            }
        }
    }

    /// Check if currently attempting to reconnect
    pub fn is_reconnecting(&self) -> bool {
        self.state.is_reconnecting()
    }

    /// Get reference to recording state for external access
    pub fn get_state(&self) -> &Arc<RecordingState> {
        &self.state
    }
}

/// Names a capture source for the pipeline and works out how it wants to be
/// buffered.
///
/// This pair is everything the pipeline learns about the hardware: it sizes its
/// mixing windows from the kind — wide and forgiving for a Bluetooth source,
/// tight for a wired one — and uses the name only to say which is which. A
/// source that is not part of this recording keeps `absent_label` and the
/// `Unknown` kind, the conservative end of that scale.
fn describe_capture_source(
    device: Option<&Arc<AudioDevice>>,
    absent_label: &str,
) -> (String, InputDeviceKind) {
    match device {
        Some(device) => {
            // The buffer size and sample rate handed to detection are nominal,
            // not measured from the device; what that costs a device the name
            // heuristics do not recognise is frozen in the tests below.
            let kind = InputDeviceKind::detect(&device.name, 512, 48000);
            (device.name.clone(), kind)
        }
        None => (absent_label.to_string(), InputDeviceKind::Unknown),
    }
}

/// What the stream manager should be restarted with after a device came back.
///
/// Reconnecting one source restarts *both* streams, so the source that never
/// went away has to be carried across from the session's current state or it
/// would quietly vanish along with the restart.
struct ReconnectPlan {
    /// The microphone the restarted streams should use, if any.
    microphone: Option<Arc<AudioDevice>>,
    /// The system audio source the restarted streams should use, if any.
    system: Option<Arc<AudioDevice>>,
    /// The device that came back.
    reconnected: Arc<AudioDevice>,
    /// The role that device fills, and so which slot of the state it updates.
    role: DeviceMonitorType,
}

/// Builds the restart plan for `device_name`, or `None` while the device is
/// still missing from `available` — in which case the caller reports the
/// attempt as unsuccessful and the monitor asks again later.
fn plan_reconnect(
    available: &[AudioDevice],
    device_name: &str,
    device_type: DeviceMonitorType,
    current_microphone: Option<Arc<AudioDevice>>,
    current_system: Option<Arc<AudioDevice>>,
) -> Option<ReconnectPlan> {
    let reconnected = Arc::new(available.iter().find(|d| d.name == device_name)?.clone());

    Some(match device_type {
        DeviceMonitorType::Microphone => ReconnectPlan {
            microphone: Some(reconnected.clone()),
            system: current_system,
            reconnected,
            role: DeviceMonitorType::Microphone,
        },
        DeviceMonitorType::SystemAudio => ReconnectPlan {
            microphone: current_microphone,
            system: Some(reconnected.clone()),
            reconnected,
            role: DeviceMonitorType::SystemAudio,
        },
    })
}

impl Default for RecordingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RecordingManager {
    fn drop(&mut self) {
        // Note: Can't call async cleanup in Drop, but streams have their own Drop implementations
        self.state.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::devices::DeviceType as DeviceDirection;
    use crate::audio::recording_saver::TranscriptSegment;
    use crate::audio::recording_state::AudioError;
    use tempfile::TempDir;

    fn device(name: &str) -> Arc<AudioDevice> {
        Arc::new(AudioDevice::new(name.to_string(), DeviceDirection::Input))
    }

    fn listed(names: &[&str]) -> Vec<AudioDevice> {
        names
            .iter()
            .map(|n| AudioDevice::new(n.to_string(), DeviceDirection::Input))
            .collect()
    }

    fn segment(seq: u64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: format!("seg-{}", seq),
            text: text.to_string(),
            audio_start_time: seq as f64,
            audio_end_time: seq as f64 + 1.0,
            duration: 1.0,
            display_time: "[00:00]".to_string(),
            confidence: 0.9,
            sequence_id: seq,
            source: Some("mic".to_string()),
        }
    }

    /// A manager whose saver is pointed at a temporary directory, so nothing
    /// here can reach the user's real recordings folder.
    fn manager_in(root: &TempDir) -> RecordingManager {
        let mut manager = RecordingManager::new();
        manager.set_recordings_root(Some(root.path().to_path_buf()));
        manager
    }

    // Describing the capture sources -----------------------------------------
    //
    // Characterization tests for the pair start_recording hands the pipeline for
    // each source, written against the two inline blocks that produced it before
    // the extraction.

    #[test]
    fn an_absent_source_takes_its_label_and_the_conservative_kind() {
        let (name, kind) = describe_capture_source(None, "No Microphone");
        assert_eq!(name, "No Microphone");
        assert_eq!(kind, InputDeviceKind::Unknown, "unknown buys the widest buffers");

        let (name, kind) = describe_capture_source(None, "No System Audio");
        assert_eq!(name, "No System Audio");
        assert_eq!(kind, InputDeviceKind::Unknown);
    }

    #[test]
    fn a_present_source_is_named_by_the_device_itself() {
        let mic = device("Yeti Stereo Microphone");
        let (name, _) = describe_capture_source(Some(&mic), "No Microphone");
        assert_eq!(name, "Yeti Stereo Microphone");
    }

    #[test]
    fn a_device_the_name_heuristics_know_is_classified_by_name() {
        let mic = device("AirPods Pro");
        let (_, kind) = describe_capture_source(Some(&mic), "No Microphone");
        assert_eq!(kind, InputDeviceKind::Bluetooth);
    }

    #[test]
    fn an_unrecognised_device_is_called_wired_because_the_probe_is_nominal() {
        // Today's behaviour, and arguably wrong. Detection is handed a fixed
        // 512-frame buffer at 48 kHz rather than anything the device reported,
        // and 512 frames at 48 kHz is 10.7 ms — which its buffer-size heuristic
        // reads as "wired". So a device no name pattern recognises gets the
        // tight 20-50 ms mixing window, while the *same* device being absent
        // would have got the conservative 80-180 ms one. Frozen here rather
        // than fixed: changing it changes live mixing behaviour.
        let mic = device("Some Unbranded Headset");
        let (_, kind) = describe_capture_source(Some(&mic), "No Microphone");
        assert_eq!(kind, InputDeviceKind::Wired);
        assert_ne!(kind, describe_capture_source(None, "No Microphone").1);
    }

    // Planning a reconnect ----------------------------------------------------
    //
    // Characterization tests for the device lookup and companion-device choice
    // attempt_device_reconnect made inline before the extraction.

    #[test]
    fn reconnecting_the_microphone_keeps_the_system_source_attached() {
        let plan = plan_reconnect(
            &listed(&["Built-in Microphone", "BlackHole 2ch"]),
            "Built-in Microphone",
            DeviceMonitorType::Microphone,
            None,
            Some(device("BlackHole 2ch")),
        )
        .expect("the device is back on the list");

        assert_eq!(plan.microphone.as_ref().unwrap().name, "Built-in Microphone");
        assert_eq!(
            plan.system.as_ref().unwrap().name,
            "BlackHole 2ch",
            "the restart must not drop the source that never went away"
        );
        assert_eq!(plan.reconnected.name, "Built-in Microphone");
        assert_eq!(plan.role, DeviceMonitorType::Microphone);
    }

    #[test]
    fn reconnecting_system_audio_keeps_the_microphone_attached() {
        let plan = plan_reconnect(
            &listed(&["Built-in Microphone", "BlackHole 2ch"]),
            "BlackHole 2ch",
            DeviceMonitorType::SystemAudio,
            Some(device("Built-in Microphone")),
            None,
        )
        .expect("the device is back on the list");

        assert_eq!(plan.microphone.as_ref().unwrap().name, "Built-in Microphone");
        assert_eq!(plan.system.as_ref().unwrap().name, "BlackHole 2ch");
        assert_eq!(plan.reconnected.name, "BlackHole 2ch");
        assert_eq!(plan.role, DeviceMonitorType::SystemAudio);
    }

    #[test]
    fn the_returning_device_replaces_the_one_the_session_was_holding() {
        let plan = plan_reconnect(
            &listed(&["AirPods Pro"]),
            "AirPods Pro",
            DeviceMonitorType::Microphone,
            Some(device("Built-in Microphone")),
            None,
        )
        .expect("the device is back on the list");

        assert_eq!(plan.microphone.as_ref().unwrap().name, "AirPods Pro");
    }

    #[test]
    fn a_device_that_has_not_come_back_yet_has_no_plan() {
        assert!(plan_reconnect(
            &listed(&["Built-in Microphone"]),
            "AirPods Pro",
            DeviceMonitorType::Microphone,
            None,
            None,
        )
        .is_none());
    }

    #[test]
    fn the_returning_device_is_matched_by_its_exact_name() {
        // The monitor reports the bare device name and the device list carries
        // the same field, so the match is a plain equality — no trimming, no
        // case folding, and no tolerance for the "(input)" suffix that other
        // parts of the device layer attach for display.
        assert!(plan_reconnect(
            &listed(&["Built-in Microphone (input)"]),
            "Built-in Microphone",
            DeviceMonitorType::Microphone,
            None,
            None,
        )
        .is_none());
    }

    // Delegation to the state and the saver ------------------------------------

    #[test]
    fn a_new_manager_is_idle() {
        let manager = RecordingManager::new();

        assert!(!manager.is_recording());
        assert!(!manager.is_paused());
        assert!(!manager.is_active());
        assert!(!manager.is_reconnecting());
        assert!(!manager.has_fatal_error());
        assert_eq!(manager.active_stream_count(), 0);
        assert_eq!(manager.get_recording_duration(), None);
        assert_eq!(manager.get_active_recording_duration(), None);
        assert_eq!(manager.get_total_pause_duration(), 0.0);
        assert_eq!(manager.get_current_pause_duration(), None);
        assert_eq!(manager.get_recording_stats(), (0, 48000));
        assert!(manager.get_transcript_segments().is_empty());
        assert!(manager.get_meeting_name().is_none());
        assert!(manager.get_meeting_folder().is_none());
    }

    #[test]
    fn pausing_and_resuming_travel_through_to_the_recording_state() {
        let manager = RecordingManager::new();

        assert!(
            manager.pause_recording().is_err(),
            "there is nothing to pause before recording starts"
        );

        manager.get_state().start_recording().unwrap();
        assert!(manager.is_active());

        manager.pause_recording().unwrap();
        assert!(manager.is_paused());
        assert!(!manager.is_active(), "a paused recording is not an active one");
        assert!(manager.get_current_pause_duration().is_some());
        assert!(manager.pause_recording().is_err(), "pausing twice is refused");

        manager.resume_recording().unwrap();
        assert!(!manager.is_paused());
        assert!(manager.is_active());
        assert!(manager.get_current_pause_duration().is_none());
        assert!(manager.resume_recording().is_err(), "resuming twice is refused");
    }

    #[test]
    fn the_manager_reports_the_errors_the_state_collected() {
        let manager = RecordingManager::new();
        let (count, last) = manager.get_error_info();
        assert_eq!(count, 0);
        assert!(last.is_none());

        manager.get_state().report_error(AudioError::StreamFailed);
        let (count, last) = manager.get_error_info();
        assert_eq!(count, 1);
        assert!(matches!(last, Some(AudioError::StreamFailed)));
        assert!(
            !manager.has_fatal_error(),
            "a stream failure is recoverable, so it is not fatal"
        );

        manager.get_state().report_error(AudioError::PermissionDenied);
        assert!(manager.has_fatal_error());
    }

    #[test]
    fn the_error_callback_is_handed_to_the_state() {
        let manager = RecordingManager::new();
        let seen = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let counter = seen.clone();
        manager.set_error_callback(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        manager.get_state().report_error(AudioError::BufferOverflow);

        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn the_meeting_name_and_its_transcript_reach_the_saver() {
        let root = TempDir::new().unwrap();
        let mut manager = manager_in(&root);

        manager.set_meeting_name(Some("Weekly sync".to_string()));
        assert_eq!(manager.get_meeting_name().as_deref(), Some("Weekly sync"));
        assert!(
            manager.get_meeting_folder().is_none(),
            "the folder is cut when recording starts, not when the name is set"
        );

        manager.add_transcript_segment(segment(1, "first"));
        manager.add_transcript_chunk("second".to_string());

        let segments = manager.get_transcript_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "first");
        assert_eq!(segments[1].text, "second");
    }

    // Teardown paths ----------------------------------------------------------
    //
    // These run against a manager that has no streams and no pipeline: what they
    // pin down is the state each path leaves behind, which is where the four
    // teardown methods actually differ.

    #[tokio::test]
    async fn stopping_the_streams_leaves_the_session_readable() {
        let mut manager = RecordingManager::new();
        manager.get_state().start_recording().unwrap();

        manager.stop_streams_only().await.unwrap();

        assert!(!manager.is_recording());
        assert!(
            manager.get_recording_duration().is_some(),
            "the duration survives, because saving still needs to ask for it"
        );
    }

    #[tokio::test]
    async fn the_flushing_stop_clears_the_session_as_well() {
        let mut manager = RecordingManager::new();
        manager.get_state().start_recording().unwrap();

        manager.stop_streams_and_force_flush().await.unwrap();

        assert!(!manager.is_recording());
        assert!(
            manager.get_recording_duration().is_none(),
            "this path runs the full cleanup, so the duration is gone with it"
        );
    }

    #[tokio::test]
    async fn cleanup_without_save_resets_an_idle_manager_too() {
        let mut manager = RecordingManager::new();
        manager.get_state().start_recording().unwrap();
        manager.get_state().report_error(AudioError::StreamFailed);

        manager.cleanup_without_save().await;

        assert!(!manager.is_recording());
        assert_eq!(manager.get_error_info().0, 0);
        assert!(manager.get_recording_duration().is_none());
    }

    // Device events -----------------------------------------------------------

    #[test]
    fn a_quiet_monitor_yields_no_device_events() {
        let mut manager = RecordingManager::new();
        assert!(manager.poll_device_events().is_none());
    }

    #[tokio::test]
    async fn a_disconnected_microphone_puts_the_session_into_reconnecting() {
        let mut manager = RecordingManager::new();
        manager.get_state().set_microphone_device(device("AirPods Pro"));

        manager
            .handle_device_disconnect("AirPods Pro".to_string(), DeviceMonitorType::Microphone)
            .await;

        assert!(manager.is_reconnecting());
        let (marked, role) = manager
            .get_state()
            .get_disconnected_device()
            .expect("the vanished device is on record");
        assert_eq!(marked.name, "AirPods Pro");
        assert_eq!(role, RecordingDeviceType::Microphone);
    }

    #[tokio::test]
    async fn a_disconnected_system_source_puts_the_session_into_reconnecting() {
        let mut manager = RecordingManager::new();
        manager.get_state().set_system_device(device("BlackHole 2ch"));

        manager
            .handle_device_disconnect("BlackHole 2ch".to_string(), DeviceMonitorType::SystemAudio)
            .await;

        assert!(manager.is_reconnecting());
        let (marked, role) = manager.get_state().get_disconnected_device().unwrap();
        assert_eq!(marked.name, "BlackHole 2ch");
        assert_eq!(role, RecordingDeviceType::System);
    }

    #[tokio::test]
    async fn the_reported_device_name_is_ignored_in_favour_of_the_one_on_record() {
        // Today's behaviour, and a trap. The name the monitor reports is used
        // for the log line only; what gets marked as disconnected is whatever
        // the state currently holds for that role. If the two ever disagree,
        // the reconnect flow goes looking for the wrong device.
        let mut manager = RecordingManager::new();
        manager.get_state().set_microphone_device(device("Built-in Microphone"));

        manager
            .handle_device_disconnect("AirPods Pro".to_string(), DeviceMonitorType::Microphone)
            .await;

        let (marked, _) = manager.get_state().get_disconnected_device().unwrap();
        assert_eq!(marked.name, "Built-in Microphone");
    }

    #[tokio::test]
    async fn a_disconnect_with_no_device_on_record_never_starts_reconnecting() {
        // Today's behaviour, and arguably wrong: a device vanished, and the
        // session neither enters the reconnecting state nor records what it
        // lost, so nothing will drive a reconnect attempt. Reachable when the
        // disconnect arrives after the state was cleared — a stop racing a
        // yanked cable.
        let mut manager = RecordingManager::new();

        manager
            .handle_device_disconnect("AirPods Pro".to_string(), DeviceMonitorType::Microphone)
            .await;

        assert!(!manager.is_reconnecting());
        assert!(manager.get_state().get_disconnected_device().is_none());
    }
}