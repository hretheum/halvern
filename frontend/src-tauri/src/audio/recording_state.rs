use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;
use anyhow::Result;

use super::devices::AudioDevice;
use super::buffer_pool::AudioBufferPool;

/// Device type for audio chunks
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Microphone,
    System,
}

/// Audio chunk with metadata for processing
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: f64,
    pub chunk_id: u64,
    pub device_type: DeviceType,
}

/// Processed audio chunk (post-VAD) for recording
#[derive(Debug, Clone)]
pub struct ProcessedAudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: f64,
    pub device_type: DeviceType,
}

/// Comprehensive error types for audio system
#[derive(Debug, Clone)]
pub enum AudioError {
    DeviceDisconnected,
    StreamFailed,
    ProcessingFailed,
    TranscriptionFailed,
    ChannelClosed,
    InitializationFailed,
    ConfigurationError,
    PermissionDenied,
    BufferOverflow,
    SampleRateUnsupported,
}

impl AudioError {
    /// Check if error is recoverable (can attempt reconnection)
    pub fn is_recoverable(&self) -> bool {
        match self {
            // Device disconnect is now recoverable - we can attempt reconnection
            AudioError::DeviceDisconnected => true,
            AudioError::StreamFailed => true,
            AudioError::ProcessingFailed => true,
            AudioError::TranscriptionFailed => true,
            AudioError::ChannelClosed => false,
            AudioError::InitializationFailed => false,
            AudioError::ConfigurationError => false,
            AudioError::PermissionDenied => false,
            AudioError::BufferOverflow => true,
            AudioError::SampleRateUnsupported => false,
        }
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> &'static str {
        match self {
            AudioError::DeviceDisconnected => "Audio device was disconnected",
            AudioError::StreamFailed => "Audio stream encountered an error",
            AudioError::ProcessingFailed => "Audio processing failed",
            AudioError::TranscriptionFailed => "Speech transcription failed",
            AudioError::ChannelClosed => "Audio channel was closed unexpectedly",
            AudioError::InitializationFailed => "Failed to initialize audio system",
            AudioError::ConfigurationError => "Audio configuration error",
            AudioError::PermissionDenied => "Microphone permission denied",
            AudioError::BufferOverflow => "Audio buffer overflow",
            AudioError::SampleRateUnsupported => "Audio sample rate not supported",
        }
    }
}

/// Recording statistics
#[derive(Debug, Default)]
pub struct RecordingStats {
    pub chunks_processed: u64,
    pub total_duration: f64,
    pub last_activity: Option<Instant>,
}

/// Unified state management for audio recording
pub struct RecordingState {
    // Core recording state
    is_recording: AtomicBool,
    is_paused: AtomicBool,
    is_reconnecting: AtomicBool,  // NEW: Attempting to reconnect to device

    // Audio devices
    microphone_device: Mutex<Option<Arc<AudioDevice>>>,
    system_device: Mutex<Option<Arc<AudioDevice>>>,
    // Track which device is disconnected for reconnection attempts
    disconnected_device: Mutex<Option<(Arc<AudioDevice>, DeviceType)>>,

    // Audio pipeline
    audio_sender: Mutex<Option<mpsc::UnboundedSender<AudioChunk>>>,

    // Memory optimization
    buffer_pool: AudioBufferPool,

    // Error handling
    error_count: AtomicU32,
    recoverable_error_count: AtomicU32,
    last_error: Mutex<Option<AudioError>>,
    /// Sticky: set by the first non-recoverable error and cleared only when a
    /// new recording starts. Derived state would be wrong here — a fatal error
    /// followed by an ordinary recoverable one must still read as fatal,
    /// because nothing about the later error resolves the earlier condition.
    fatal_error: AtomicBool,
    error_callback: Mutex<Option<Box<dyn Fn(&AudioError) + Send + Sync>>>,

    // Statistics
    stats: Mutex<RecordingStats>,

    // Recording start time for accurate timestamps
    recording_start: Mutex<Option<Instant>>,
    // Pause time tracking
    pause_start: Mutex<Option<Instant>>,
    total_pause_duration: Mutex<std::time::Duration>,
}

impl RecordingState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            is_recording: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            is_reconnecting: AtomicBool::new(false),
            microphone_device: Mutex::new(None),
            system_device: Mutex::new(None),
            disconnected_device: Mutex::new(None),
            audio_sender: Mutex::new(None),
            buffer_pool: AudioBufferPool::new(16, 48000), // Pool of 16 buffers with 48kHz samples capacity
            error_count: AtomicU32::new(0),
            recoverable_error_count: AtomicU32::new(0),
            last_error: Mutex::new(None),
            fatal_error: AtomicBool::new(false),
            error_callback: Mutex::new(None),
            stats: Mutex::new(RecordingStats::default()),
            recording_start: Mutex::new(None),
            pause_start: Mutex::new(None),
            total_pause_duration: Mutex::new(std::time::Duration::ZERO),
        })
    }

    // Recording control
    pub fn start_recording(&self) -> Result<()> {
        self.is_recording.store(true, Ordering::SeqCst);
        *self.recording_start.lock().unwrap() = Some(Instant::now());
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
        *self.last_error.lock().unwrap() = None;
        self.fatal_error.store(false, Ordering::SeqCst);
        // A recording that begins carries none of the previous one's timing.
        // stop_recording clears the pause flag but deliberately leaves the
        // accumulated total alone, so without this a second recording on the
        // same state would subtract the first one's pauses from its own
        // active duration.
        *self.total_pause_duration.lock().unwrap() = std::time::Duration::ZERO;
        Ok(())
    }

    pub fn stop_recording(&self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.is_paused.store(false, Ordering::SeqCst);
        // Clear pause tracking when stopping
        *self.pause_start.lock().unwrap() = None;
        // CRITICAL: Clear audio sender to close the pipeline channel
        // This ensures the pipeline loop exits properly after processing all chunks
        *self.audio_sender.lock().unwrap() = None;
        // CRITICAL: Clear device references to release microphone/speaker
        // Without this, Arc<AudioDevice> references persist and keep the mic active
        *self.microphone_device.lock().unwrap() = None;
        *self.system_device.lock().unwrap() = None;
        *self.disconnected_device.lock().unwrap() = None;
        log::info!("Recording stopped, device references cleared");
    }

    pub fn pause_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot pause when not recording"));
        }
        if self.is_paused() {
            return Err(anyhow::anyhow!("Recording is already paused"));
        }

        self.is_paused.store(true, Ordering::SeqCst);
        *self.pause_start.lock().unwrap() = Some(Instant::now());
        log::info!("Recording paused");
        Ok(())
    }

    pub fn resume_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot resume when not recording"));
        }
        if !self.is_paused() {
            return Err(anyhow::anyhow!("Recording is not paused"));
        }

        // Calculate pause duration and add to total
        if let Some(pause_start) = self.pause_start.lock().unwrap().take() {
            let pause_duration = pause_start.elapsed();
            *self.total_pause_duration.lock().unwrap() += pause_duration;
            log::info!("Recording resumed after pause of {:.2}s", pause_duration.as_secs_f64());
        }

        self.is_paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        self.is_recording() && !self.is_paused()
    }

    // Reconnection state management
    pub fn start_reconnecting(&self, device: Arc<AudioDevice>, device_type: DeviceType) {
        self.is_reconnecting.store(true, Ordering::SeqCst);
        *self.disconnected_device.lock().unwrap() = Some((device, device_type));
        log::info!("Started reconnection attempt for device");
    }

    pub fn stop_reconnecting(&self) {
        self.is_reconnecting.store(false, Ordering::SeqCst);
        *self.disconnected_device.lock().unwrap() = None;
        log::info!("Stopped reconnection attempt");
    }

    pub fn is_reconnecting(&self) -> bool {
        self.is_reconnecting.load(Ordering::SeqCst)
    }

    pub fn get_disconnected_device(&self) -> Option<(Arc<AudioDevice>, DeviceType)> {
        self.disconnected_device.lock().unwrap().clone()
    }

    // Device management
    pub fn set_microphone_device(&self, device: Arc<AudioDevice>) {
        *self.microphone_device.lock().unwrap() = Some(device);
    }

    pub fn set_system_device(&self, device: Arc<AudioDevice>) {
        *self.system_device.lock().unwrap() = Some(device);
    }

    pub fn get_microphone_device(&self) -> Option<Arc<AudioDevice>> {
        self.microphone_device.lock().unwrap().clone()
    }

    pub fn get_system_device(&self) -> Option<Arc<AudioDevice>> {
        self.system_device.lock().unwrap().clone()
    }

    // Audio pipeline management
    pub fn set_audio_sender(&self, sender: mpsc::UnboundedSender<AudioChunk>) {
        *self.audio_sender.lock().unwrap() = Some(sender);
    }

    pub fn send_audio_chunk(&self, chunk: AudioChunk) -> Result<()> {
        // Don't send audio chunks when paused
        if self.is_paused() {
            return Ok(()); // Silently discard chunks while paused
        }

        if let Some(sender) = self.audio_sender.lock().unwrap().as_ref() {
            sender.send(chunk).map_err(|_| anyhow::anyhow!("Failed to send audio chunk"))?;

            // Update statistics
            let mut stats = self.stats.lock().unwrap();
            stats.chunks_processed += 1;
            stats.last_activity = Some(Instant::now());
            Ok(())
        } else {
            // Return an error when no sender is available (pipeline not ready)
            Err(anyhow::anyhow!("Audio pipeline not ready - no sender available"))
        }
    }

    // Error handling
    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&AudioError) + Send + Sync + 'static,
    {
        *self.error_callback.lock().unwrap() = Some(Box::new(callback));
    }

    pub fn report_error(&self, error: AudioError) {
        let count = self.error_count.fetch_add(1, Ordering::SeqCst) + 1;

        // Track recoverable vs non-recoverable errors separately
        if error.is_recoverable() {
            let recoverable_count = self.recoverable_error_count.fetch_add(1, Ordering::SeqCst) + 1;
            log::warn!("Recoverable audio error ({}): {:?}", recoverable_count, error);

            // Allow more recoverable errors before stopping
            if recoverable_count >= 10 {
                log::error!("Too many recoverable errors ({}), stopping recording", recoverable_count);
                self.stop_recording();
            }
        } else {
            log::error!("Non-recoverable audio error: {:?}", error);
            self.fatal_error.store(true, Ordering::SeqCst);
            // Stop immediately for non-recoverable errors
            self.stop_recording();
        }

        *self.last_error.lock().unwrap() = Some(error.clone());

        // Call error callback if set
        if let Some(callback) = self.error_callback.lock().unwrap().as_ref() {
            callback(&error);
        }

        // Fallback: stop recording after too many total errors
        if count >= 15 {
            log::error!("Too many total audio errors ({}), stopping recording", count);
            self.stop_recording();
        }
    }

    pub fn get_error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }

    pub fn get_recoverable_error_count(&self) -> u32 {
        self.recoverable_error_count.load(Ordering::SeqCst)
    }

    pub fn get_last_error(&self) -> Option<AudioError> {
        self.last_error.lock().unwrap().clone()
    }

    pub fn has_fatal_error(&self) -> bool {
        self.fatal_error.load(Ordering::SeqCst)
    }

    // Statistics
    pub fn get_stats(&self) -> RecordingStats {
        self.stats.lock().unwrap().clone()
    }

    pub fn get_recording_duration(&self) -> Option<f64> {
        self.recording_start
            .lock()
            .unwrap()
            .map(|start| start.elapsed().as_secs_f64())
    }

    pub fn get_active_recording_duration(&self) -> Option<f64> {
        self.recording_start.lock().unwrap().map(|start| {
            let total_duration = start.elapsed().as_secs_f64();
            let pause_duration = self.get_total_pause_duration();
            let current_pause = if self.is_paused() {
                self.pause_start
                    .lock()
                    .unwrap()
                    .map(|p| p.elapsed().as_secs_f64())
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            total_duration - pause_duration - current_pause
        })
    }

    pub fn get_total_pause_duration(&self) -> f64 {
        self.total_pause_duration.lock().unwrap().as_secs_f64()
    }

    pub fn get_current_pause_duration(&self) -> Option<f64> {
        if self.is_paused() {
            self.pause_start
                .lock()
                .unwrap()
                .map(|start| start.elapsed().as_secs_f64())
        } else {
            None
        }
    }

    // Memory management
    pub fn get_buffer_pool(&self) -> AudioBufferPool {
        self.buffer_pool.clone()
    }

    // Cleanup
    pub fn cleanup(&self) {
        self.stop_recording();
        self.stop_reconnecting();
        *self.microphone_device.lock().unwrap() = None;
        *self.system_device.lock().unwrap() = None;
        *self.disconnected_device.lock().unwrap() = None;
        *self.audio_sender.lock().unwrap() = None;
        *self.last_error.lock().unwrap() = None;
        *self.error_callback.lock().unwrap() = None;
        *self.stats.lock().unwrap() = RecordingStats::default();
        *self.recording_start.lock().unwrap() = None;
        *self.pause_start.lock().unwrap() = None;
        *self.total_pause_duration.lock().unwrap() = std::time::Duration::ZERO;
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
        self.fatal_error.store(false, Ordering::SeqCst);

        // Clear buffer pool to free memory
        self.buffer_pool.clear();
    }
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            is_recording: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            is_reconnecting: AtomicBool::new(false),
            microphone_device: Mutex::new(None),
            system_device: Mutex::new(None),
            disconnected_device: Mutex::new(None),
            audio_sender: Mutex::new(None),
            buffer_pool: AudioBufferPool::new(16, 48000), // Pool of 16 buffers with 48kHz samples capacity
            error_count: AtomicU32::new(0),
            recoverable_error_count: AtomicU32::new(0),
            last_error: Mutex::new(None),
            fatal_error: AtomicBool::new(false),
            error_callback: Mutex::new(None),
            stats: Mutex::new(RecordingStats::default()),
            recording_start: Mutex::new(None),
            pause_start: Mutex::new(None),
            total_pause_duration: Mutex::new(std::time::Duration::ZERO),
        }
    }
}

// Thread-safe cloning for RecordingStats
impl Clone for RecordingStats {
    fn clone(&self) -> Self {
        Self {
            chunks_processed: self.chunks_processed,
            total_duration: self.total_duration,
            last_activity: self.last_activity,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<RecordingState> {
        RecordingState::new()
    }

    fn mic() -> Arc<AudioDevice> {
        Arc::new(AudioDevice::new(
            "Test Microphone".to_string(),
            super::super::devices::DeviceType::Input,
        ))
    }

    // Error taxonomy ---------------------------------------------------------

    #[test]
    fn the_error_taxonomy_separates_recoverable_from_fatal() {
        // The split drives two different shutdown paths in report_error, so
        // a variant landing on the wrong side changes when recordings die.
        for e in [
            AudioError::DeviceDisconnected,
            AudioError::StreamFailed,
            AudioError::ProcessingFailed,
            AudioError::TranscriptionFailed,
            AudioError::BufferOverflow,
        ] {
            assert!(e.is_recoverable(), "{e:?} should allow a retry");
        }
        for e in [
            AudioError::ChannelClosed,
            AudioError::InitializationFailed,
            AudioError::ConfigurationError,
            AudioError::PermissionDenied,
            AudioError::SampleRateUnsupported,
        ] {
            assert!(!e.is_recoverable(), "{e:?} should stop the recording");
        }
    }

    // Lifecycle --------------------------------------------------------------

    #[test]
    fn a_fresh_state_is_idle() {
        let s = state();
        assert!(!s.is_recording());
        assert!(!s.is_paused());
        assert!(!s.is_active());
        assert!(!s.is_reconnecting());
        assert_eq!(s.get_recording_duration(), None);
    }

    #[test]
    fn starting_marks_recording_and_resets_error_counters() {
        let s = state();
        s.report_error(AudioError::StreamFailed);
        assert_eq!(s.get_error_count(), 1);

        s.start_recording().expect("start succeeds");
        assert!(s.is_recording());
        assert!(s.is_active());
        assert_eq!(s.get_error_count(), 0, "a new recording starts with a clean slate");
        assert_eq!(s.get_recoverable_error_count(), 0);
        assert_eq!(s.get_last_error().map(|e| format!("{e:?}")), None);
        assert!(s.get_recording_duration().is_some(), "the clock starts with the recording");
    }

    #[test]
    fn stopping_clears_the_pipeline_and_the_devices() {
        // Releasing the device references is what actually lets go of the
        // microphone; holding them past stop keeps the input indicator on.
        let s = state();
        s.start_recording().unwrap();
        s.set_microphone_device(mic());
        let (tx, _rx) = mpsc::unbounded_channel();
        s.set_audio_sender(tx);
        s.pause_recording().unwrap();

        s.stop_recording();

        assert!(!s.is_recording());
        assert!(!s.is_paused(), "stop clears a pending pause");
        assert!(s.get_microphone_device().is_none());
        assert!(s.get_system_device().is_none());
        let chunk = AudioChunk {
            data: vec![0.0],
            sample_rate: 48000,
            timestamp: 0.0,
            chunk_id: 1,
            device_type: DeviceType::Microphone,
        };
        assert!(
            s.send_audio_chunk(chunk).is_err(),
            "the pipeline channel is gone after stop"
        );
    }

    // Pause / resume ---------------------------------------------------------

    #[test]
    fn pause_and_resume_demand_the_right_starting_state() {
        let s = state();
        assert!(s.pause_recording().is_err(), "cannot pause when idle");
        assert!(s.resume_recording().is_err(), "cannot resume when idle");

        s.start_recording().unwrap();
        assert!(s.resume_recording().is_err(), "cannot resume when not paused");
        s.pause_recording().expect("pause while recording");
        assert!(s.pause_recording().is_err(), "cannot pause twice");
        assert!(!s.is_active(), "paused means not active");
        assert!(s.get_current_pause_duration().is_some());

        s.resume_recording().expect("resume while paused");
        assert!(s.is_active());
        assert_eq!(s.get_current_pause_duration(), None);
    }

    #[test]
    fn a_completed_pause_moves_into_the_total() {
        let s = state();
        s.start_recording().unwrap();
        s.pause_recording().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        s.resume_recording().unwrap();

        let total = s.get_total_pause_duration();
        assert!(total > 0.0, "the pause left a trace in the total");
        let active = s.get_active_recording_duration().expect("recording runs");
        let wall = s.get_recording_duration().expect("recording runs");
        assert!(
            active < wall,
            "active duration excludes the pause: active={active}, wall={wall}"
        );
    }

    #[test]
    fn pause_totals_do_not_leak_into_the_next_recording() {
        // Regression test: stop_recording clears the pause flag but leaves the
        // accumulated total alone, and start_recording used not to reset it
        // either — only cleanup() did. A second recording on the same state
        // without an intervening cleanup therefore subtracted the previous
        // recording's pauses from its own active duration, wrong from the
        // first second.
        let s = state();
        s.start_recording().unwrap();
        s.pause_recording().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        s.resume_recording().unwrap();
        assert!(s.get_total_pause_duration() > 0.0, "the first recording did pause");
        s.stop_recording();

        s.start_recording().unwrap();
        assert_eq!(
            s.get_total_pause_duration(),
            0.0,
            "the new recording starts its own pause accounting"
        );
        let active = s.get_active_recording_duration().expect("recording runs");
        assert!(active >= 0.0, "active duration is not skewed by the old pause");
    }

    // Audio pipeline ---------------------------------------------------------

    #[test]
    fn chunks_flow_to_the_sender_and_are_counted() {
        let s = state();
        let (tx, mut rx) = mpsc::unbounded_channel();
        s.set_audio_sender(tx);
        s.start_recording().unwrap();

        let chunk = AudioChunk {
            data: vec![0.1, 0.2],
            sample_rate: 48000,
            timestamp: 1.0,
            chunk_id: 7,
            device_type: DeviceType::System,
        };
        s.send_audio_chunk(chunk).expect("send succeeds");

        let received = rx.try_recv().expect("the chunk reached the channel");
        assert_eq!(received.chunk_id, 7);
        assert_eq!(received.device_type, DeviceType::System);
        assert_eq!(s.get_stats().chunks_processed, 1);
    }

    #[test]
    fn chunks_are_dropped_silently_while_paused() {
        // The capture callbacks keep running during a pause; dropping here is
        // what keeps paused audio out of the recording and the stats alike.
        let s = state();
        let (tx, mut rx) = mpsc::unbounded_channel();
        s.set_audio_sender(tx);
        s.start_recording().unwrap();
        s.pause_recording().unwrap();

        let chunk = AudioChunk {
            data: vec![0.5],
            sample_rate: 48000,
            timestamp: 2.0,
            chunk_id: 8,
            device_type: DeviceType::Microphone,
        };
        s.send_audio_chunk(chunk).expect("a paused send is not an error");
        assert!(rx.try_recv().is_err(), "nothing reached the channel");
        assert_eq!(s.get_stats().chunks_processed, 0, "dropped chunks are not counted");
    }

    #[test]
    fn sending_without_a_pipeline_is_an_error() {
        let s = state();
        let chunk = AudioChunk {
            data: vec![],
            sample_rate: 48000,
            timestamp: 0.0,
            chunk_id: 0,
            device_type: DeviceType::Microphone,
        };
        assert!(s.send_audio_chunk(chunk).is_err());
    }

    // Error thresholds -------------------------------------------------------

    #[test]
    fn a_fatal_error_stops_the_recording_at_once() {
        let s = state();
        s.start_recording().unwrap();

        s.report_error(AudioError::PermissionDenied);

        assert!(!s.is_recording());
        assert!(s.has_fatal_error());
        assert_eq!(s.get_error_count(), 1);
        assert_eq!(s.get_recoverable_error_count(), 0);
    }

    #[test]
    fn recoverable_errors_are_tolerated_up_to_ten() {
        let s = state();
        s.start_recording().unwrap();

        for _ in 0..9 {
            s.report_error(AudioError::StreamFailed);
        }
        assert!(s.is_recording(), "nine recoverable errors keep the recording alive");
        assert!(!s.has_fatal_error());

        s.report_error(AudioError::StreamFailed);
        assert!(!s.is_recording(), "the tenth stops it");
        assert_eq!(s.get_recoverable_error_count(), 10);
    }

    #[test]
    fn the_error_callback_hears_every_report() {
        let s = state();
        let seen = Arc::new(AtomicU32::new(0));
        let seen_clone = seen.clone();
        s.set_error_callback(move |_| {
            seen_clone.fetch_add(1, Ordering::SeqCst);
        });

        s.report_error(AudioError::StreamFailed);
        s.report_error(AudioError::PermissionDenied);
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_recoverable_error_after_a_fatal_one_does_not_hide_the_fatality() {
        // Regression test: has_fatal_error used to read only the *last* error,
        // so an ordinary recoverable error arriving afterwards flipped the
        // answer back to false while the fatal condition stood unresolved.
        // The live reader is the end-of-meeting analytics in
        // recording_commands.rs, which therefore under-reported failures.
        let s = state();
        s.report_error(AudioError::PermissionDenied);
        assert!(s.has_fatal_error());

        s.report_error(AudioError::StreamFailed);
        assert!(
            s.has_fatal_error(),
            "a later recoverable error resolves nothing about the fatal one"
        );

        // Only a fresh recording clears it.
        s.start_recording().unwrap();
        assert!(!s.has_fatal_error(), "a new recording starts with a clean slate");
    }

    // Reconnection -----------------------------------------------------------

    #[test]
    fn reconnection_remembers_which_device_dropped() {
        let s = state();
        s.start_reconnecting(mic(), DeviceType::Microphone);

        assert!(s.is_reconnecting());
        let (device, device_type) = s.get_disconnected_device().expect("device recorded");
        assert_eq!(device.name, "Test Microphone");
        assert_eq!(device_type, DeviceType::Microphone);

        s.stop_reconnecting();
        assert!(!s.is_reconnecting());
        assert!(s.get_disconnected_device().is_none());
    }

    // Cleanup ----------------------------------------------------------------

    #[test]
    fn cleanup_returns_the_state_to_factory_settings() {
        let s = state();
        s.start_recording().unwrap();
        s.set_microphone_device(mic());
        let (tx, _rx) = mpsc::unbounded_channel();
        s.set_audio_sender(tx);
        s.report_error(AudioError::StreamFailed);
        s.start_reconnecting(mic(), DeviceType::Microphone);

        s.cleanup();

        assert!(!s.is_recording());
        assert!(!s.is_reconnecting());
        assert!(s.get_microphone_device().is_none());
        assert!(s.get_last_error().is_none());
        assert_eq!(s.get_error_count(), 0);
        assert_eq!(s.get_stats().chunks_processed, 0);
        assert_eq!(s.get_recording_duration(), None);
        assert_eq!(s.get_total_pause_duration(), 0.0);
    }
}
