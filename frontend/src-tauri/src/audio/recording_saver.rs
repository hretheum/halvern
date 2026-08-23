use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use anyhow::Result;
use log::{info, warn, error};
use tauri::{AppHandle, Runtime, Emitter};
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};
use std::path::{Path, PathBuf};

use super::recording_state::AudioChunk;
use super::audio_processing::create_meeting_folder;
use super::incremental_saver::IncrementalAudioSaver;

/// Structured transcript segment for JSON export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub audio_start_time: f64, // Seconds from recording start
    pub audio_end_time: f64,   // Seconds from recording start
    pub duration: f64,          // Segment duration in seconds
    pub display_time: String,   // Formatted time for display like "[02:15]"
    pub confidence: f32,
    pub sequence_id: u64,
    /// Which source the words came from: `mic`, `system`, or absent for material
    /// predating the source split (imports, older recordings).
    #[serde(default)]
    pub source: Option<String>,
}

/// Meeting metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub version: String,
    pub meeting_id: Option<String>,
    pub meeting_name: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub devices: DeviceInfo,
    pub audio_file: String,
    pub transcript_file: String,
    pub sample_rate: u32,
    pub status: String,  // "recording", "completed", "error"
    /// Server-side identifier of the calendar event this recording was matched
    /// to. Lets the same meeting be recognised again later.
    ///
    /// All three calendar fields carry `#[serde(default)]` so metadata written
    /// before the calendar existed still parses — recordings on disk outlive
    /// any single shape of this struct.
    #[serde(default)]
    pub calendar_event_id: Option<String>,
    /// Who was invited. **Not** a list of who spoke — that follows from the
    /// transcript, not from the calendar.
    #[serde(default)]
    pub participants: Vec<String>,
    /// The invitation body. It usually carries the purpose of the meeting, so it
    /// is worth keeping as context for the summary.
    #[serde(default)]
    pub agenda: Option<String>,
    /// How this recording started: `manual`, `auto` or `imported`.
    ///
    /// Written here rather than passed through the save call because the
    /// frontend, which triggers the database write, has no way of knowing —
    /// the meeting detector starts recordings without it. `None` on recordings
    /// made before this field existed.
    #[serde(default)]
    pub source: Option<String>,
    /// Application the audio was captured from, e.g. `Microsoft Teams`. Only
    /// detector-started recordings can establish this.
    #[serde(default)]
    pub app_name: Option<String>,
}

/// How a recording came to exist, and what started it.
///
/// Travels from whichever entry point began the recording down to the metadata
/// file, so the library can filter by source and name the app without guessing
/// from the meeting title — the detector bakes the app into a fallback title
/// like "Microsoft Teams — auto", which is a display string, not data.
#[derive(Debug, Clone)]
pub struct RecordingOrigin {
    pub source: RecordingSource,
    pub app_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingSource {
    /// The user pressed record.
    Manual,
    /// The meeting detector started it.
    Auto,
    /// Created from an existing audio file rather than captured live.
    Imported,
}

impl RecordingSource {
    /// The value stored in `meetings.source` and in metadata.json. Kept in one
    /// place so the database, the metadata file and the library filter cannot
    /// drift apart.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Imported => "imported",
        }
    }
}

impl Default for RecordingOrigin {
    fn default() -> Self {
        Self {
            source: RecordingSource::Manual,
            app_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub microphone: Option<String>,
    pub system_audio: Option<String>,
}

/// New recording saver using incremental saving strategy
pub struct RecordingSaver {
    incremental_saver: Option<Arc<AsyncMutex<IncrementalAudioSaver>>>,
    meeting_folder: Option<PathBuf>,
    meeting_name: Option<String>,
    /// Where recordings are written. `None` means the platform default.
    recordings_root: Option<PathBuf>,
    calendar_match: Option<crate::calendar::CalendarEvent>,
    origin: RecordingOrigin,
    metadata: Option<MeetingMetadata>,
    transcript_segments: Arc<Mutex<Vec<TranscriptSegment>>>,
    chunk_receiver: Option<mpsc::UnboundedReceiver<AudioChunk>>,
    is_saving: Arc<Mutex<bool>>,
}

impl RecordingSaver {
    pub fn new() -> Self {
        Self {
            incremental_saver: None,
            meeting_folder: None,
            meeting_name: None,
            recordings_root: None,
            calendar_match: None,
            origin: RecordingOrigin::default(),
            metadata: None,
            transcript_segments: Arc::new(Mutex::new(Vec::new())),
            chunk_receiver: None,
            is_saving: Arc::new(Mutex::new(false)),
        }
    }

    /// Set the meeting name for this recording session
    pub fn set_meeting_name(&mut self, name: Option<String>) {
        self.meeting_name = name;
    }

    /// Sets the directory recordings are written under.
    ///
    /// Kept here rather than read from preferences on the spot because
    /// `initialize_meeting_folder` is synchronous and the preference store needs an
    /// `AppHandle` and an await. The caller has both and already loads preferences.
    pub fn set_recordings_root(&mut self, root: Option<PathBuf>) {
        self.recordings_root = root;
    }

    /// Remember the calendar event this recording belongs to, if any.
    ///
    /// Stored rather than applied immediately because the metadata does not
    /// exist yet — it is created later, when the meeting folder is initialised.
    pub fn set_calendar_match(
        &mut self,
        matched: Option<crate::calendar::CalendarEvent>,
    ) {
        self.calendar_match = matched;
    }

    /// Record how this recording started. Same timing as the calendar match:
    /// stored now, written when the meeting folder gets its metadata.
    pub fn set_origin(&mut self, origin: RecordingOrigin) {
        self.origin = origin;
    }

    /// Set device information in metadata
    pub fn set_device_info(&mut self, mic_name: Option<String>, sys_name: Option<String>) {
        if let Some(ref mut metadata) = self.metadata {
            metadata.devices.microphone = mic_name;
            metadata.devices.system_audio = sys_name;

            // Write updated metadata to disk if folder exists
            if let Some(folder) = &self.meeting_folder {
                let metadata_clone = metadata.clone();
                if let Err(e) = self.write_metadata(folder, &metadata_clone) {
                    warn!("Failed to update metadata with device info: {}", e);
                }
            }
        }
    }

    /// Add or update a structured transcript segment (upserts based on sequence_id)
    /// Also saves incrementally to disk
    pub fn add_transcript_segment(&self, segment: TranscriptSegment) {
        if let Ok(mut segments) = self.transcript_segments.lock() {
            // Check if segment with same sequence_id exists (update it)
            if let Some(existing) = segments.iter_mut().find(|s| s.sequence_id == segment.sequence_id) {
                *existing = segment.clone();
                info!("Updated transcript segment {} (seq: {}) - total segments: {}",
                      segment.id, segment.sequence_id, segments.len());
            } else {
                // New segment, add it
                segments.push(segment.clone());
                info!("Added new transcript segment {} (seq: {}) - total segments: {}",
                      segment.id, segment.sequence_id, segments.len());
            }
        } else {
            error!("Failed to lock transcript segments for adding segment {}", segment.id);
        }

        // NEW: Save incrementally to disk
        if let Some(folder) = &self.meeting_folder {
            if let Err(e) = self.write_transcripts_json(folder) {
                warn!("Failed to write incremental transcript update: {}", e);
            }
        }
    }

    /// Legacy method for backward compatibility - converts text to basic segment
    pub fn add_transcript_chunk(&self, text: String) {
        // add_transcript_segment upserts by sequence_id, so a fixed id here
        // would make every legacy chunk after the first overwrite the one
        // before it instead of accumulating. One past the current maximum
        // keeps each call additive, matching what the structured path does.
        let sequence_id = self
            .transcript_segments
            .lock()
            .map(|segments| segments.iter().map(|s| s.sequence_id).max().map_or(0, |m| m + 1))
            .unwrap_or(0);

        let segment = TranscriptSegment {
            id: format!("seg_{}", chrono::Utc::now().timestamp_millis()),
            text,
            audio_start_time: 0.0,
            audio_end_time: 0.0,
            duration: 0.0,
            display_time: "[00:00]".to_string(),
            confidence: 1.0,
            sequence_id,
            source: None,
        };
        self.add_transcript_segment(segment);
    }

    /// Start accumulation with optional incremental saving
    ///
    /// # Arguments
    /// * `auto_save` - If true, creates checkpoints and enables saving. If false, audio chunks are discarded.
    pub fn start_accumulation(&mut self, auto_save: bool) -> mpsc::UnboundedSender<AudioChunk> {
        if auto_save {
            info!("Initializing incremental audio saver for recording (auto-save ENABLED)");
        } else {
            info!("Starting recording without audio saving (auto-save DISABLED - transcripts only)");
        }

        // Create channel for receiving audio chunks
        let (sender, receiver) = mpsc::unbounded_channel::<AudioChunk>();
        self.chunk_receiver = Some(receiver);

        // Initialize meeting folder and incremental saver ONLY if auto_save is enabled
        if auto_save {
            if let Some(name) = self.meeting_name.clone() {
                match self.initialize_meeting_folder(&name, true) {
                    Ok(()) => info!("Successfully initialized meeting folder with checkpoints"),
                    Err(e) => {
                        error!("Failed to initialize meeting folder: {}", e);
                        // Continue anyway - will use fallback flat structure
                    }
                }
            }
        } else {
            // When auto_save is false, still create meeting folder for transcripts/metadata
            // but skip .checkpoints directory
            if let Some(name) = self.meeting_name.clone() {
                match self.initialize_meeting_folder(&name, false) {
                    Ok(()) => info!("Successfully initialized meeting folder (transcripts only)"),
                    Err(e) => {
                        error!("Failed to initialize meeting folder: {}", e);
                    }
                }
            }
        }

        // Start accumulation task
        let is_saving_clone = self.is_saving.clone();
        let incremental_saver_arc = self.incremental_saver.clone();
        let save_audio = auto_save;

        if let Some(mut receiver) = self.chunk_receiver.take() {
            tokio::spawn(async move {
                info!("Recording saver accumulation task started (save_audio: {})", save_audio);

                while let Some(chunk) = receiver.recv().await {
                    // Check if we should continue
                    let should_continue = if let Ok(is_saving) = is_saving_clone.lock() {
                        *is_saving
                    } else {
                        false
                    };

                    if !should_continue {
                        break;
                    }

                    // Only process audio chunks if auto_save is enabled
                    if save_audio {
                        // Add chunk to incremental saver
                        if let Some(saver_arc) = &incremental_saver_arc {
                            let mut saver_guard = saver_arc.lock().await;
                            if let Err(e) = saver_guard.add_chunk(chunk) {
                                error!("Failed to add chunk to incremental saver: {}", e);
                            }
                        } else {
                            error!("Incremental saver not available while accumulating");
                        }
                    } else {
                        // auto_save is false: discard audio chunk (no-op)
                        // Transcription already happened in the pipeline before this point
                    }
                }

                info!("Recording saver accumulation task ended");
            });
        }

        // Set saving flag
        if let Ok(mut is_saving) = self.is_saving.lock() {
            *is_saving = true;
        }

        sender
    }

    /// Initialize meeting folder structure and metadata
    ///
    /// # Arguments
    /// * `meeting_name` - Name of the meeting
    /// * `create_checkpoints` - Whether to create .checkpoints/ directory and IncrementalAudioSaver
    fn initialize_meeting_folder(&mut self, meeting_name: &str, create_checkpoints: bool) -> Result<()> {
        // Load preferences to get base recordings folder
        // The configured folder wins; without one, fall back to the platform default.
        // Before this, the preference was stored, its directory was created and the
        // "open folder" button pointed at it — while recordings were written elsewhere.
        let base_folder = self
            .recordings_root
            .clone()
            .unwrap_or_else(super::recording_preferences::get_default_recordings_folder);

        // Create meeting folder structure (with or without .checkpoints/ subdirectory)
        let meeting_folder = create_meeting_folder(&base_folder, meeting_name, create_checkpoints)?;

        // Only initialize incremental saver if checkpoints are needed (auto_save is true)
        if create_checkpoints {
            let incremental_saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000)?;
            self.incremental_saver = Some(Arc::new(AsyncMutex::new(incremental_saver)));
            info!("✅ Incremental audio saver initialized for meeting: {}", meeting_name);
        } else {
            info!("⚠️  Skipped incremental audio saver (auto-save disabled)");
        }

        // Create initial metadata
        let metadata = MeetingMetadata {
            version: "1.0".to_string(),
            meeting_id: None,  // Will be set by backend
            meeting_name: Some(meeting_name.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            duration_seconds: None,
            devices: DeviceInfo {
                microphone: None,  // Could be enhanced to store actual device names
                system_audio: None,
            },
            audio_file: if create_checkpoints { "audio.mp4".to_string() } else { "".to_string() },
            transcript_file: "transcripts.json".to_string(),
            sample_rate: 48000,
            status: "recording".to_string(),
            calendar_event_id: self.calendar_match.as_ref().and_then(|m| m.external_id.clone()),
            participants: self
                .calendar_match
                .as_ref()
                .map(|m| m.participants.clone())
                .unwrap_or_default(),
            agenda: self.calendar_match.as_ref().and_then(|m| m.agenda.clone()),
            source: Some(self.origin.source.as_str().to_string()),
            app_name: self.origin.app_name.clone(),
        };

        // Write initial metadata.json
        self.write_metadata(&meeting_folder, &metadata)?;

        self.meeting_folder = Some(meeting_folder);
        self.metadata = Some(metadata);

        Ok(())
    }

    /// Write metadata.json to disk (atomic write with temp file)
    fn write_metadata(&self, folder: &Path, metadata: &MeetingMetadata) -> Result<()> {
        let metadata_path = folder.join("metadata.json");
        let temp_path = folder.join(".metadata.json.tmp");

        let json_string = serde_json::to_string_pretty(metadata)?;
        std::fs::write(&temp_path, json_string)?;
        std::fs::rename(&temp_path, &metadata_path)?;  // Atomic

        Ok(())
    }

    /// Write transcripts.json to disk (atomic write with temp file and validation)
    fn write_transcripts_json(&self, folder: &Path) -> Result<()> {
        // Clone segments to avoid holding lock during I/O
        let segments_clone = if let Ok(segments) = self.transcript_segments.lock() {
            segments.clone()
        } else {
            error!("Failed to lock transcript segments for writing");
            return Err(anyhow::anyhow!("Failed to lock transcript segments"));
        };

        info!("Writing {} transcript segments to JSON", segments_clone.len());

        let transcript_path = folder.join("transcripts.json");
        let temp_path = folder.join(".transcripts.json.tmp");

        // Create JSON structure
        let json = serde_json::json!({
            "version": "1.0",
            "segments": segments_clone,
            "last_updated": chrono::Utc::now().to_rfc3339(),
            "total_segments": segments_clone.len()
        });

        // Serialize to pretty JSON string
        let json_string = serde_json::to_string_pretty(&json)
            .map_err(|e| {
                error!("Failed to serialize transcripts to JSON: {}", e);
                anyhow::anyhow!("JSON serialization failed: {}", e)
            })?;

        // Write to temp file with error handling
        std::fs::write(&temp_path, &json_string)
            .map_err(|e| {
                error!("Failed to write transcript temp file to {}: {}", temp_path.display(), e);
                anyhow::anyhow!("Failed to write temp file: {}", e)
            })?;

        // Verify temp file was written correctly
        if !temp_path.exists() {
            error!("Temp transcript file does not exist after write: {}", temp_path.display());
            return Err(anyhow::anyhow!("Temp file verification failed"));
        }

        // Atomic rename
        std::fs::rename(&temp_path, &transcript_path)
            .map_err(|e| {
                error!("Failed to rename transcript file from {} to {}: {}",
                       temp_path.display(), transcript_path.display(), e);
                anyhow::anyhow!("Failed to rename transcript file: {}", e)
            })?;

        info!("✅ Successfully wrote transcripts.json with {} segments", segments_clone.len());
        Ok(())
    }

    // in frontend/src-tauri/src/audio/recording_saver.rs
    pub fn get_stats(&self) -> (usize, u32) {
        if let Some(ref saver) = self.incremental_saver {
            if let Ok(guard) = saver.try_lock() {
                (guard.get_checkpoint_count() as usize, 48000)
            } else {
                (0, 48000)
            }
        } else {
            (0, 48000)
        }
    }

    /// Stop and save using incremental saving approach
    ///
    /// # Arguments
    /// * `app` - Tauri app handle for emitting events
    /// * `recording_duration` - Actual recording duration in seconds (from RecordingState)
    pub async fn stop_and_save<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        recording_duration: Option<f64>
    ) -> Result<Option<String>, String> {
        info!("Stopping recording saver");

        // Stop accumulation
        if let Ok(mut is_saving) = self.is_saving.lock() {
            *is_saving = false;
        }

        // Give time for final chunks
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Check if incremental saver exists (indicates auto_save was enabled)
        let should_save_audio = self.incremental_saver.is_some();

        if !should_save_audio {
            info!("⚠️  No audio saver initialized (auto-save was disabled) - skipping audio finalization");
            info!("✅ Transcripts and metadata already saved incrementally");
            return Ok(None);
        }

        // Finalize incremental saver (merge checkpoints into final audio.mp4)
        let final_audio_path = if let Some(saver_arc) = &self.incremental_saver {
            let mut saver = saver_arc.lock().await;
            match saver.finalize().await {
                Ok(path) => {
                    info!("✅ Successfully finalized audio: {}", path.display());
                    path
                }
                Err(e) => {
                    error!("❌ Failed to finalize incremental saver: {}", e);
                    return Err(format!("Failed to finalize audio: {}", e));
                }
            }
        } else {
            error!("No incremental saver initialized - cannot save recording");
            return Err("No incremental saver initialized".to_string());
        };

        // Save final transcripts.json with validation
        if let Some(folder) = &self.meeting_folder {
            if let Err(e) = self.write_transcripts_json(folder) {
                error!("❌ Failed to write final transcripts: {}", e);
                return Err(format!("Failed to save transcripts: {}", e));
            }

            // Verify transcripts were written correctly
            let transcript_path = folder.join("transcripts.json");
            if !transcript_path.exists() {
                error!("❌ Transcript file was not created at: {}", transcript_path.display());
                return Err("Transcript file verification failed".to_string());
            }
            info!("✅ Transcripts saved and verified at: {}", transcript_path.display());
        }

        // Update metadata to completed status with actual recording duration
        if let (Some(folder), Some(mut metadata)) = (&self.meeting_folder, self.metadata.clone()) {
            metadata.status = "completed".to_string();
            metadata.completed_at = Some(chrono::Utc::now().to_rfc3339());

            // Use actual recording duration from RecordingState (more accurate than transcript segments)
            // Falls back to last transcript segment if duration not provided
            metadata.duration_seconds = recording_duration.or_else(|| {
                if let Ok(segments) = self.transcript_segments.lock() {
                    segments.last().map(|seg| seg.audio_end_time)
                } else {
                    None
                }
            });

            if let Err(e) = self.write_metadata(folder, &metadata) {
                error!("❌ Failed to update metadata to completed: {}", e);
                return Err(format!("Failed to update metadata: {}", e));
            }

            info!("✅ Metadata updated with duration: {:?}s", metadata.duration_seconds);
        }

        // Emit save event with audio and transcript paths
        let save_event = serde_json::json!({
            "audio_file": final_audio_path.to_string_lossy(),
            "transcript_file": self.meeting_folder.as_ref()
                .map(|f| f.join("transcripts.json").to_string_lossy().to_string()),
            "meeting_name": self.meeting_name,
            "meeting_folder": self.meeting_folder.as_ref()
                .map(|f| f.to_string_lossy().to_string())
        });

        if let Err(e) = app.emit("recording-saved", &save_event) {
            warn!("Failed to emit recording-saved event: {}", e);
        }

        // Clean up transcript segments
        if let Ok(mut segments) = self.transcript_segments.lock() {
            segments.clear();
        }

        Ok(Some(final_audio_path.to_string_lossy().to_string()))
    }

    /// Get the meeting folder path (for passing to backend)
    pub fn get_meeting_folder(&self) -> Option<&PathBuf> {
        self.meeting_folder.as_ref()
    }

    /// Get accumulated transcript segments (for reload sync)
    pub fn get_transcript_segments(&self) -> Vec<TranscriptSegment> {
        if let Ok(segments) = self.transcript_segments.lock() {
            segments.clone()
        } else {
            Vec::new()
        }
    }

    /// Get meeting name (for reload sync)
    pub fn get_meeting_name(&self) -> Option<String> {
        self.meeting_name.clone()
    }
}

impl Default for RecordingSaver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    /// A saver pointed at a temporary directory, so nothing here can reach the
    /// user's real recordings folder.
    fn saver_in(root: &TempDir, name: &str) -> RecordingSaver {
        let mut saver = RecordingSaver::new();
        saver.set_meeting_name(Some(name.to_string()));
        saver.set_recordings_root(Some(root.path().to_path_buf()));
        saver
    }

    fn read_json(path: &std::path::Path) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("file readable");
        serde_json::from_str(&raw).expect("valid JSON")
    }

    #[tokio::test]
    async fn transcript_only_recording_creates_folder_and_metadata_without_checkpoints() {
        let root = TempDir::new().unwrap();
        let mut saver = saver_in(&root, "Weekly sync");

        let _sender = saver.start_accumulation(false);

        let folder = saver.get_meeting_folder().expect("folder initialised").clone();
        assert!(folder.starts_with(root.path()), "recordings stay under the configured root");
        assert!(!folder.join(".checkpoints").exists(), "auto-save off means no checkpoints");

        let metadata = read_json(&folder.join("metadata.json"));
        assert_eq!(metadata["status"], "recording");
        assert_eq!(metadata["meeting_name"], "Weekly sync");
        assert_eq!(metadata["sample_rate"], 48000);
        assert_eq!(metadata["transcript_file"], "transcripts.json");
        assert_eq!(metadata["audio_file"], "", "no audio file is promised when none will be written");

        assert!(
            !folder.join("transcripts.json").exists(),
            "transcripts.json appears with the first segment, not at initialisation"
        );
    }

    #[tokio::test]
    async fn auto_save_recording_promises_audio_and_creates_checkpoints() {
        let root = TempDir::new().unwrap();
        let mut saver = saver_in(&root, "Recorded call");

        let _sender = saver.start_accumulation(true);

        let folder = saver.get_meeting_folder().expect("folder initialised").clone();
        assert!(folder.join(".checkpoints").is_dir());

        let metadata = read_json(&folder.join("metadata.json"));
        assert_eq!(metadata["audio_file"], "audio.mp4");
    }

    #[tokio::test]
    async fn meeting_name_with_path_hostile_characters_still_lands_under_the_root() {
        let root = TempDir::new().unwrap();
        let mut saver = saver_in(&root, "Q3 / plans: review?");

        let _sender = saver.start_accumulation(false);

        let folder = saver.get_meeting_folder().expect("folder initialised").clone();
        assert!(folder.starts_with(root.path()), "sanitisation must not escape the root");
        assert!(folder.join("metadata.json").exists());
    }

    #[tokio::test]
    async fn same_name_in_the_same_minute_gets_distinct_folders() {
        // Regression test: the folder name carries a minute-resolution
        // timestamp, so two recordings with one name inside a minute used to
        // collapse onto the same folder and the second metadata write
        // overwrote the first (audio_processing::unique_folder fixes this).
        let root = TempDir::new().unwrap();
        let mut first = saver_in(&root, "Collision");
        let mut second = saver_in(&root, "Collision");

        let _s1 = first.start_accumulation(false);
        let _s2 = second.start_accumulation(false);

        let first_folder = first.get_meeting_folder().expect("first folder initialised").clone();
        let second_folder = second.get_meeting_folder().expect("second folder initialised").clone();
        assert_ne!(first_folder, second_folder, "same-name recordings no longer share a folder");

        // And each metadata.json still names the right recording.
        let first_json = read_json(&first_folder.join("metadata.json"));
        let second_json = read_json(&second_folder.join("metadata.json"));
        assert_eq!(first_json["meeting_name"], "Collision");
        assert_eq!(second_json["meeting_name"], "Collision");
    }

    #[tokio::test]
    async fn segments_upsert_by_sequence_id_and_reach_disk() {
        let root = TempDir::new().unwrap();
        let mut saver = saver_in(&root, "Segments");
        let _sender = saver.start_accumulation(false);
        let folder = saver.get_meeting_folder().unwrap().clone();

        saver.add_transcript_segment(segment(1, "first"));
        saver.add_transcript_segment(segment(2, "second"));
        saver.add_transcript_segment(segment(1, "first, corrected"));

        let segments = saver.get_transcript_segments();
        assert_eq!(segments.len(), 2, "an existing sequence_id updates in place");
        assert_eq!(segments[0].text, "first, corrected");
        assert_eq!(segments[1].text, "second");

        let json = read_json(&folder.join("transcripts.json"));
        assert_eq!(json["total_segments"], 2);
        assert_eq!(json["segments"][0]["text"], "first, corrected");
        assert_eq!(json["segments"][0]["source"], "mic");
        assert!(
            !folder.join(".transcripts.json.tmp").exists(),
            "the atomic-write temp file must not be left behind"
        );
    }

    #[tokio::test]
    async fn segments_accumulate_in_memory_even_without_a_folder() {
        // No meeting name, so no folder was initialised; the disk write is
        // skipped but the in-memory list still grows and can be saved later.
        let saver = RecordingSaver::new();
        saver.add_transcript_segment(segment(1, "kept in memory"));
        assert_eq!(saver.get_transcript_segments().len(), 1);
    }

    #[tokio::test]
    async fn legacy_chunks_accumulate_instead_of_overwriting() {
        // Regression test: add_transcript_chunk used to build sequence_id 0
        // every time, and add_transcript_segment upserts by sequence_id, so
        // consecutive legacy chunks silently overwrote one another.
        let saver = RecordingSaver::new();
        saver.add_transcript_chunk("first chunk".to_string());
        saver.add_transcript_chunk("second chunk".to_string());
        saver.add_transcript_chunk("third chunk".to_string());

        let segments = saver.get_transcript_segments();
        assert_eq!(segments.len(), 3, "each legacy chunk keeps its own segment");
        assert_eq!(segments[0].text, "first chunk");
        assert_eq!(segments[1].text, "second chunk");
        assert_eq!(segments[2].text, "third chunk");
    }

    #[tokio::test]
    async fn legacy_chunks_interleave_safely_with_structured_segments() {
        // Structured segments arrive with their own sequence_id from the
        // transcription pipeline; a legacy chunk mixed in must not collide
        // with one already taken.
        let saver = RecordingSaver::new();
        saver.add_transcript_segment(segment(5, "structured"));
        saver.add_transcript_chunk("legacy".to_string());

        let segments = saver.get_transcript_segments();
        assert_eq!(segments.len(), 2);
        assert!(
            segments.iter().any(|s| s.sequence_id == 6 && s.text == "legacy"),
            "the legacy chunk lands past the highest sequence_id already present"
        );
    }

    #[tokio::test]
    async fn calendar_match_flows_into_metadata() {
        let root = TempDir::new().unwrap();
        let mut saver = saver_in(&root, "Planning");
        saver.set_calendar_match(Some(crate::calendar::CalendarEvent {
            title: "Q3 planning".to_string(),
            participants: vec!["Ala".to_string(), "Ola".to_string()],
            agenda: Some("Budget and headcount".to_string()),
            external_id: Some("event-42".to_string()),
        }));

        let _sender = saver.start_accumulation(false);
        let folder = saver.get_meeting_folder().unwrap().clone();

        let metadata = read_json(&folder.join("metadata.json"));
        assert_eq!(metadata["calendar_event_id"], "event-42");
        assert_eq!(metadata["participants"][1], "Ola");
        assert_eq!(metadata["agenda"], "Budget and headcount");
    }

    #[tokio::test]
    async fn device_info_updates_metadata_on_disk() {
        let root = TempDir::new().unwrap();
        let mut saver = saver_in(&root, "Devices");
        let _sender = saver.start_accumulation(false);
        let folder = saver.get_meeting_folder().unwrap().clone();

        saver.set_device_info(Some("Built-in Microphone".to_string()), Some("BlackHole 2ch".to_string()));

        let metadata = read_json(&folder.join("metadata.json"));
        assert_eq!(metadata["devices"]["microphone"], "Built-in Microphone");
        assert_eq!(metadata["devices"]["system_audio"], "BlackHole 2ch");
    }

    #[tokio::test]
    async fn device_info_before_initialisation_is_dropped() {
        // Without metadata there is nowhere to put the names; the call is a
        // documented no-op rather than an error.
        let mut saver = RecordingSaver::new();
        saver.set_device_info(Some("Mic".to_string()), None);
        let _sender = saver.start_accumulation(false);
        assert!(saver.get_meeting_folder().is_none(), "no name, no folder");
    }

    #[tokio::test]
    async fn stats_without_an_audio_saver_report_zero_checkpoints() {
        let saver = RecordingSaver::new();
        assert_eq!(saver.get_stats(), (0, 48000));
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    #[test]
    fn source_values_match_what_the_database_and_filters_expect() {
        // These strings are the contract between metadata.json, the meetings
        // table and the library's source filter. Changing one without the
        // others silently empties a filter.
        assert_eq!(RecordingSource::Manual.as_str(), "manual");
        assert_eq!(RecordingSource::Auto.as_str(), "auto");
        assert_eq!(RecordingSource::Imported.as_str(), "imported");
    }

    #[test]
    fn default_origin_is_manual_without_an_app() {
        // Every entry point except the detector goes through the default, and
        // only the detector can honestly name an application.
        let origin = RecordingOrigin::default();
        assert_eq!(origin.source, RecordingSource::Manual);
        assert!(origin.app_name.is_none());
    }

    #[test]
    fn metadata_carries_origin_through_serialisation() {
        let json = r#"{"source":"auto","app_name":"Microsoft Teams"}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value.get("source").unwrap(), "auto");
        assert_eq!(value.get("app_name").unwrap(), "Microsoft Teams");
    }
}
