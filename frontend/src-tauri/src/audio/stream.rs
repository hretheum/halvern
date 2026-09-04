use std::sync::Arc;
use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, SupportedStreamConfig};
use log::{error, info, warn};

use super::devices::{AudioDevice, get_device_and_config};
use super::pipeline::AudioCapture;
use super::recording_state::{RecordingState, DeviceType};
use super::capture::{AudioCaptureBackend, get_current_backend};

#[cfg(target_os = "macos")]
use super::capture::CoreAudioCapture;

/// Stream backend implementation
pub enum StreamBackend {
    /// CPAL-based stream (ScreenCaptureKit or default)
    Cpal(Stream),
    /// Core Audio direct implementation (macOS only)
    #[cfg(target_os = "macos")]
    CoreAudio {
        task: Option<tokio::task::JoinHandle<()>>,
    },
}

// SAFETY: While Stream doesn't implement Send, we ensure it's only accessed
// from the same thread context by using spawn_blocking for operations that cross thread boundaries
unsafe impl Send for StreamBackend {}

/// Which capture path a device actually takes, once the configured backend has
/// been weighed against what the device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamPath {
    /// CPAL, whichever host it is built against on this platform.
    Cpal,
    /// Core Audio directly, for system audio on macOS.
    #[cfg(target_os = "macos")]
    CoreAudio,
}

/// Picks the capture path for a device.
///
/// The configured backend only ever decides how *system* audio is captured. A
/// microphone goes through CPAL whatever the setting says, because the Core
/// Audio path here is a process tap, not a device it could open.
#[cfg(target_os = "macos")]
fn choose_stream_path(device_type: &DeviceType, backend: AudioCaptureBackend) -> StreamPath {
    if *device_type == DeviceType::System && backend == AudioCaptureBackend::CoreAudio {
        StreamPath::CoreAudio
    } else {
        StreamPath::Cpal
    }
}

/// Off macOS there is no second path to pick: everything is captured by CPAL.
#[cfg(not(target_os = "macos"))]
fn choose_stream_path(_device_type: &DeviceType, _backend: AudioCaptureBackend) -> StreamPath {
    StreamPath::Cpal
}

/// Simplified audio stream wrapper with multi-backend support
pub struct AudioStream {
    device: Arc<AudioDevice>,
    backend: StreamBackend,
}

// SAFETY: AudioStream contains StreamBackend which we've marked as Send
unsafe impl Send for AudioStream {}

impl AudioStream {
    /// Create a new audio stream for the given device
    pub async fn create(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
    ) -> Result<Self> {
        // Get current backend from global config
        let backend_type = get_current_backend();
        Self::create_with_backend(device, state, device_type, backend_type).await
    }

    /// Create a new audio stream with explicit backend selection
    pub async fn create_with_backend(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        backend_type: AudioCaptureBackend,
    ) -> Result<Self> {
        info!("🎵 Stream: Creating audio stream for device: {} with backend: {:?}, device_type: {:?}",
              device.name, backend_type, device_type);

        let path = choose_stream_path(&device_type, backend_type);
        info!("🎵 Stream: capture path for {:?} on backend {:?} is {:?}",
              device_type, backend_type, path);

        #[cfg(target_os = "macos")]
        if path == StreamPath::CoreAudio {
            info!("🎵 Stream: Using Core Audio backend (cidre) for system audio");
            return Self::create_core_audio_stream(device, state, device_type).await;
        }

        // Default path: use CPAL
        #[cfg(target_os = "macos")]
        let backend_name = if backend_type == AudioCaptureBackend::ScreenCaptureKit {
            "ScreenCaptureKit"
        } else {
            "CPAL (default)"
        };

        #[cfg(not(target_os = "macos"))]
        let backend_name = "CPAL";

        info!("🎵 Stream: Using CPAL backend ({}) for device: {}", backend_name, device.name);
        Self::create_cpal_stream(device, state, device_type).await
    }

    /// Create a CPAL-based stream (ScreenCaptureKit on macOS)
    async fn create_cpal_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
    ) -> Result<Self> {
        info!("Creating CPAL stream for device: {}", device.name);

        // Get the underlying cpal device and config
        let (cpal_device, config) = get_device_and_config(&device).await?;

        info!("Audio config - Sample rate: {}, Channels: {}, Format: {:?}",
              config.sample_rate().0, config.channels(), config.sample_format());

        // Create audio capture processor
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            config.sample_rate().0,
            config.channels(),
            device_type,
        );

        // Build the appropriate stream based on sample format
        let stream = Self::build_stream(&cpal_device, &config, capture.clone())?;

        // Start the stream
        stream.play()?;
        info!("CPAL stream started for device: {}", device.name);

        Ok(Self {
            device,
            backend: StreamBackend::Cpal(stream),
        })
    }

    /// Create a Core Audio stream (macOS only)
    #[cfg(target_os = "macos")]
    async fn create_core_audio_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
    ) -> Result<Self> {
        info!("🔊 Stream: Creating Core Audio stream for device: {}", device.name);

        // Create Core Audio capture
        info!("🔊 Stream: Calling CoreAudioCapture::new()...");
        let process_ids = crate::audio::system_detector::scoped_process_object_ids();
        let capture_impl = CoreAudioCapture::new(&process_ids)
            .map_err(|e| {
                error!("❌ Stream: CoreAudioCapture::new() failed: {}", e);
                anyhow::anyhow!("Failed to create Core Audio capture: {}", e)
            })?;

        info!("✅ Stream: CoreAudioCapture created, calling stream()...");
        let core_stream = capture_impl.stream()
            .map_err(|e| {
                error!("❌ Stream: capture_impl.stream() failed: {}", e);
                anyhow::anyhow!("Failed to create Core Audio stream: {}", e)
            })?;

        let sample_rate = core_stream.sample_rate();
        info!("✅ Stream: Core Audio stream created with sample rate: {} Hz", sample_rate);

        // Create audio capture processor for pipeline integration
        // CRITICAL: Core Audio tap is MONO (with_mono_global_tap_excluding_processes)
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            sample_rate,
            1, // Core Audio tap is MONO (not stereo!)
            device_type,
        );

        // Spawn task to process Core Audio stream samples
        // The stream needs to be polled continuously to produce samples
        let device_name = device.name.clone();
        info!("🔊 Stream: Spawning tokio task to poll Core Audio stream...");
        let task = tokio::spawn({
            let capture = capture.clone();
            let mut stream = core_stream;

            async move {
                use futures_util::StreamExt;

                let frames_per_chunk = 1024; // Process in chunks of 1024 samples
                let mut chunker = SampleChunker::new(frames_per_chunk);

                info!("✅ Stream: Core Audio processing task started for {}", device_name);

                let mut _sample_count = 0u64;
                while let Some(sample) = stream.next().await {
                    _sample_count += 1;
                    // if _sample_count % 48000 == 0 {
                    //     info!("📊 Stream: Received {} samples from Core Audio stream", _sample_count);
                    // }

                    chunker.push(sample, |chunk| capture.process_audio_data(chunk));
                }

                // Process any remaining samples
                chunker.flush(|chunk| capture.process_audio_data(chunk));

                info!("⚠️ Stream: Core Audio processing task ended for {}", device_name);
            }
        });

        info!("✅ Stream: Core Audio stream fully initialized for device: {}", device.name);

        Ok(Self {
            device: device.clone(),
            backend: StreamBackend::CoreAudio {
                task: Some(task),
            },
        })
    }

    /// Build stream based on sample format
    fn build_stream(
        device: &Device,
        config: &SupportedStreamConfig,
        capture: AudioCapture,
    ) -> Result<Stream> {
        let config_copy = config.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        capture.process_audio_data(data);
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        capture.process_audio_data(&i16_samples_to_f32(data));
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I32 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        capture.process_audio_data(&i32_samples_to_f32(data));
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            cpal::SampleFormat::I8 => {
                let capture_clone = capture.clone();
                device.build_input_stream(
                    &config_copy.into(),
                    move |data: &[i8], _: &cpal::InputCallbackInfo| {
                        capture.process_audio_data(&i8_samples_to_f32(data));
                    },
                    move |err| {
                        capture_clone.handle_stream_error(err);
                    },
                    None,
                )?
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported sample format: {:?}", config.sample_format()));
            }
        };

        Ok(stream)
    }

    /// Get device info
    pub fn device(&self) -> &AudioDevice {
        &self.device
    }

    /// Stop the stream
    pub fn stop(self) -> Result<()> {
        info!("Stopping audio stream for device: {}", self.device.name);

        match self.backend {
            StreamBackend::Cpal(stream) => {
                // CRITICAL: Pause the stream first to stop callbacks immediately
                // This ensures closures stop executing before we drop the stream,
                // allowing Arc references captured in callbacks to be released
                if let Err(e) = stream.pause() {
                    warn!("Failed to pause stream before drop: {}", e);
                }
                info!("Stream paused, now dropping to release callbacks");
                drop(stream);
            }
            #[cfg(target_os = "macos")]
            StreamBackend::CoreAudio { task } => {
                // Abort the processing task and wait briefly for cleanup
                if let Some(task_handle) = task {
                    info!("Aborting Core Audio task...");
                    task_handle.abort();
                    // Give the runtime a moment to clean up the aborted task
                    // This helps ensure Arc references in the closure are dropped
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    info!("Core Audio task aborted");
                }
            }
        }

        // Diagnostic, not a guard. Three times on 13 Aug 2026 a stopped
        // recording's error callback fired minutes later, when the Bluetooth
        // headset was physically disconnected — so something outlives this
        // teardown. A count above one here names the moment: references to the
        // device are still held after the stream is gone. Anything at one, and
        // the late callback is CPAL's own, not a leak of ours.
        //
        // Left in deliberately: the question only shows up on real hardware
        // after a real meeting, which is exactly when nobody is attached to a
        // debugger.
        let remaining = Arc::strong_count(&self.device) - 1;
        if remaining > 0 {
            warn!(
                "Device '{}' still has {} reference(s) after its stream was dropped",
                self.device.name, remaining
            );
        } else {
            info!("Device '{}' held no further references", self.device.name);
        }

        // Explicitly drop self.device Arc reference
        drop(self.device);
        info!("Audio stream stopped and device reference dropped");
        Ok(())
    }
}

/// Scales 16-bit samples into the -1.0..1.0 range the pipeline works in.
///
/// The divisor is the format's *positive* full scale, which is one step short
/// of the negative one, so the most negative sample lands just outside the
/// range; the tests below say by how much.
fn i16_samples_to_f32(data: &[i16]) -> Vec<f32> {
    data.iter()
        .map(|&sample| sample as f32 / i16::MAX as f32)
        .collect()
}

/// Scales 32-bit samples into the -1.0..1.0 range, as [`i16_samples_to_f32`].
fn i32_samples_to_f32(data: &[i32]) -> Vec<f32> {
    data.iter()
        .map(|&sample| sample as f32 / i32::MAX as f32)
        .collect()
}

/// Scales 8-bit samples into the -1.0..1.0 range, as [`i16_samples_to_f32`].
fn i8_samples_to_f32(data: &[i8]) -> Vec<f32> {
    data.iter()
        .map(|&sample| sample as f32 / i8::MAX as f32)
        .collect()
}

/// Gathers single samples into fixed-size blocks.
///
/// The Core Audio tap yields one sample at a time while the pipeline wants
/// blocks, so the samples pile up here until a block is full. The part worth
/// having a name for is the tail: when the stream ends part-way through a block
/// those samples are still recorded audio, and `flush` is what keeps them from
/// being dropped on the floor.
#[cfg(target_os = "macos")]
struct SampleChunker {
    buffer: Vec<f32>,
    frames_per_chunk: usize,
}

#[cfg(target_os = "macos")]
impl SampleChunker {
    fn new(frames_per_chunk: usize) -> Self {
        Self {
            buffer: Vec::new(),
            frames_per_chunk,
        }
    }

    /// Takes one sample, handing `emit` the block as soon as it is full.
    fn push(&mut self, sample: f32, emit: impl FnOnce(&[f32])) {
        self.buffer.push(sample);
        if self.buffer.len() >= self.frames_per_chunk {
            emit(&self.buffer);
            self.buffer.clear();
        }
    }

    /// Hands `emit` whatever is left over, if anything is.
    fn flush(&mut self, emit: impl FnOnce(&[f32])) {
        if !self.buffer.is_empty() {
            emit(&self.buffer);
            self.buffer.clear();
        }
    }
}

/// Audio stream manager for handling multiple streams
pub struct AudioStreamManager {
    microphone_stream: Option<AudioStream>,
    system_stream: Option<AudioStream>,
    state: Arc<RecordingState>,
}

// SAFETY: AudioStreamManager contains AudioStream which we've marked as Send
unsafe impl Send for AudioStreamManager {}

impl AudioStreamManager {
    pub fn new(state: Arc<RecordingState>) -> Self {
        Self {
            microphone_stream: None,
            system_stream: None,
            state,
        }
    }

    /// Start audio streams for the given devices
    pub async fn start_streams(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
    ) -> Result<()> {
        use super::capture::get_current_backend;
        let backend = get_current_backend();
        info!("🎙️ Starting audio streams with backend: {:?}", backend);

        // Start microphone stream
        if let Some(mic_device) = microphone_device {
            info!("🎤 Creating microphone stream: {} (always uses CPAL)", mic_device.name);
            match AudioStream::create(mic_device.clone(), self.state.clone(), DeviceType::Microphone).await {
                Ok(stream) => {
                    self.state.set_microphone_device(mic_device);
                    self.microphone_stream = Some(stream);
                    info!("✅ Microphone stream created successfully");
                }
                Err(e) => {
                    error!("❌ Failed to create microphone stream: {}", e);
                    return Err(e);
                }
            }
        } else {
            info!("ℹ️ No microphone device specified, skipping microphone stream");
        }

        // Start system audio stream
        if let Some(sys_device) = system_device {
            // `backend` is the *configured preference*, not what is used. On
            // Windows and Linux the preference stays at its ScreenCaptureKit
            // default and the capture path resolves to CPAL, so printing the
            // preference alone put "ScreenCaptureKit" in a Windows log — an
            // Apple API on a machine that has never had one. Say which path is
            // actually taken; the preference is only interesting beside it.
            info!(
                "🔊 Creating system audio stream: {} (path: {:?}, configured backend: {:?})",
                sys_device.name,
                choose_stream_path(&DeviceType::System, backend),
                backend
            );
            match AudioStream::create(sys_device.clone(), self.state.clone(), DeviceType::System).await {
                Ok(stream) => {
                    self.state.set_system_device(sys_device);
                    self.system_stream = Some(stream);
                    info!(
                        "✅ System audio stream created via {:?}",
                        choose_stream_path(&DeviceType::System, backend)
                    );
                }
                Err(e) => {
                    warn!("⚠️ Failed to create system audio stream: {}", e);
                    // Don't fail if only system audio fails
                }
            }
        } else {
            info!("ℹ️ No system device specified, skipping system audio stream");
        }

        // Ensure at least one stream was created
        if self.microphone_stream.is_none() && self.system_stream.is_none() {
            return Err(anyhow::anyhow!("No audio streams could be created"));
        }

        Ok(())
    }

    /// Stop all audio streams
    pub fn stop_streams(&mut self) -> Result<()> {
        info!("Stopping all audio streams");

        let mut errors = Vec::new();

        // Stop microphone stream
        if let Some(mic_stream) = self.microphone_stream.take() {
            if let Err(e) = mic_stream.stop() {
                error!("Failed to stop microphone stream: {}", e);
                errors.push(e);
            }
        }

        // Stop system stream
        if let Some(sys_stream) = self.system_stream.take() {
            if let Err(e) = sys_stream.stop() {
                error!("Failed to stop system stream: {}", e);
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            Err(anyhow::anyhow!("Failed to stop some streams: {:?}", errors))
        } else {
            info!("All audio streams stopped successfully");
            Ok(())
        }
    }

    /// Get stream count
    pub fn active_stream_count(&self) -> usize {
        let mut count = 0;
        if self.microphone_stream.is_some() {
            count += 1;
        }
        if self.system_stream.is_some() {
            count += 1;
        }
        count
    }

    /// Check if any streams are active
    pub fn has_active_streams(&self) -> bool {
        self.microphone_stream.is_some() || self.system_stream.is_some()
    }
}

impl Drop for AudioStreamManager {
    fn drop(&mut self) {
        if let Err(e) = self.stop_streams() {
            error!("Error stopping streams during drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Choosing the capture path -----------------------------------------------
    //
    // Characterization tests for the `use_core_audio` condition create_with_backend
    // evaluated inline before the extraction.

    #[test]
    fn a_microphone_always_takes_the_cpal_path() {
        // Including on macOS with Core Audio configured, which is the default
        // there: the Core Audio backend taps other processes' output, so it has
        // nothing to offer an input device.
        assert_eq!(
            choose_stream_path(&DeviceType::Microphone, AudioCaptureBackend::default()),
            StreamPath::Cpal
        );
    }

    #[test]
    fn system_audio_takes_the_cpal_path_on_screencapturekit() {
        assert_eq!(
            choose_stream_path(&DeviceType::System, AudioCaptureBackend::ScreenCaptureKit),
            StreamPath::Cpal
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_audio_takes_the_core_audio_path_when_that_backend_is_chosen() {
        assert_eq!(
            choose_stream_path(&DeviceType::System, AudioCaptureBackend::CoreAudio),
            StreamPath::CoreAudio
        );
    }

    // Sample format conversion --------------------------------------------------

    #[test]
    fn silence_survives_every_conversion() {
        assert_eq!(i16_samples_to_f32(&[0, 0]), vec![0.0, 0.0]);
        assert_eq!(i32_samples_to_f32(&[0, 0]), vec![0.0, 0.0]);
        assert_eq!(i8_samples_to_f32(&[0, 0]), vec![0.0, 0.0]);
    }

    #[test]
    fn positive_full_scale_becomes_exactly_one() {
        assert_eq!(i16_samples_to_f32(&[i16::MAX]), vec![1.0]);
        assert_eq!(i32_samples_to_f32(&[i32::MAX]), vec![1.0]);
        assert_eq!(i8_samples_to_f32(&[i8::MAX]), vec![1.0]);
    }

    #[test]
    fn half_scale_becomes_roughly_half() {
        let converted = i16_samples_to_f32(&[i16::MAX / 2]);
        assert!((converted[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn an_empty_buffer_converts_to_an_empty_buffer() {
        assert!(i16_samples_to_f32(&[]).is_empty());
        assert!(i32_samples_to_f32(&[]).is_empty());
        assert!(i8_samples_to_f32(&[]).is_empty());
    }

    #[test]
    fn the_negative_extreme_lands_just_outside_the_range() {
        // Today's behaviour, and arguably wrong. Dividing by positive full
        // scale — one step short of the negative one — pushes the most negative
        // sample past -1.0, so a genuinely full-scale recording hands the
        // pipeline samples slightly outside the range mixing and VAD assume.
        // The overshoot is small (0.003% at 16-bit, 0.8% at 8-bit) and frozen
        // here rather than fixed: rescaling changes every recorded sample.
        assert!(i16_samples_to_f32(&[i16::MIN])[0] < -1.0);
        assert!(i8_samples_to_f32(&[i8::MIN])[0] < -1.0);

        // 32-bit is spared, but only by accident: i32::MAX has no exact f32,
        // and the value it rounds to is the negative extreme's magnitude.
        assert_eq!(i32_samples_to_f32(&[i32::MIN]), vec![-1.0]);
    }

    // Chunking the Core Audio tap ------------------------------------------------

    #[cfg(target_os = "macos")]
    mod chunking {
        use super::*;

        /// Runs `samples` through a chunker and returns every block it emitted,
        /// the closing flush included.
        fn blocks_of(frames_per_chunk: usize, samples: &[f32]) -> Vec<Vec<f32>> {
            let mut chunker = SampleChunker::new(frames_per_chunk);
            let mut emitted = Vec::new();
            for &sample in samples {
                chunker.push(sample, |chunk| emitted.push(chunk.to_vec()));
            }
            chunker.flush(|chunk| emitted.push(chunk.to_vec()));
            emitted
        }

        #[test]
        fn samples_are_handed_over_a_full_block_at_a_time() {
            let blocks = blocks_of(4, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
            assert_eq!(blocks, vec![vec![1.0, 2.0, 3.0, 4.0], vec![5.0, 6.0, 7.0, 8.0]]);
        }

        #[test]
        fn a_stream_that_ends_mid_block_still_delivers_its_tail() {
            let blocks = blocks_of(4, &[1.0, 2.0, 3.0, 4.0, 5.0]);
            assert_eq!(
                blocks,
                vec![vec![1.0, 2.0, 3.0, 4.0], vec![5.0]],
                "the last fraction of a second is recorded audio too"
            );
        }

        #[test]
        fn a_stream_ending_on_a_block_boundary_flushes_nothing() {
            let blocks = blocks_of(4, &[1.0, 2.0, 3.0, 4.0]);
            assert_eq!(blocks, vec![vec![1.0, 2.0, 3.0, 4.0]]);
        }

        #[test]
        fn a_stream_that_never_filled_a_block_delivers_it_on_flush() {
            assert_eq!(blocks_of(4, &[1.0, 2.0]), vec![vec![1.0, 2.0]]);
        }

        #[test]
        fn a_stream_that_produced_nothing_delivers_nothing() {
            assert!(blocks_of(4, &[]).is_empty());
        }

        #[test]
        fn flushing_twice_does_not_repeat_the_tail() {
            let mut chunker = SampleChunker::new(4);
            let mut emitted = Vec::new();
            chunker.push(1.0, |chunk| emitted.push(chunk.to_vec()));
            chunker.flush(|chunk| emitted.push(chunk.to_vec()));
            chunker.flush(|chunk| emitted.push(chunk.to_vec()));
            assert_eq!(emitted, vec![vec![1.0]]);
        }
    }

    // Managing the pair of streams -------------------------------------------------
    //
    // Only the paths that never reach the audio hardware: opening a device needs
    // a real one, so anything past that is out of reach here.

    #[test]
    fn a_new_manager_holds_no_streams() {
        let manager = AudioStreamManager::new(RecordingState::new());
        assert_eq!(manager.active_stream_count(), 0);
        assert!(!manager.has_active_streams());
    }

    #[test]
    fn stopping_streams_that_were_never_started_is_not_an_error() {
        let mut manager = AudioStreamManager::new(RecordingState::new());
        assert!(manager.stop_streams().is_ok());
        assert_eq!(manager.active_stream_count(), 0);
    }

    #[tokio::test]
    async fn a_recording_with_neither_source_refuses_to_start() {
        let state = RecordingState::new();
        let mut manager = AudioStreamManager::new(state.clone());

        let error = manager
            .start_streams(None, None)
            .await
            .expect_err("a recording with nothing to record is refused");

        assert!(error.to_string().contains("No audio streams could be created"));
        assert!(!manager.has_active_streams());
        assert!(
            state.get_microphone_device().is_none() && state.get_system_device().is_none(),
            "a refused start leaves no device on record"
        );
    }
}