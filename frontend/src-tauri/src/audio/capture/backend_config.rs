// Backend configuration for system audio capture
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use once_cell::sync::Lazy;
use log::info;

/// Available audio capture backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioCaptureBackend {
    /// ScreenCaptureKit backend (macOS default)
    /// Uses CPAL with ScreenCaptureKit host for system audio
    ScreenCaptureKit,

    /// Core Audio backend (macOS only)
    /// Uses direct Core Audio API with aggregate device + tap
    #[cfg(target_os = "macos")]
    CoreAudio,
}

impl AudioCaptureBackend {
    /// Human-readable label. **No callers.**
    ///
    /// `Display` used to call this, which is the only reason it looked live;
    /// `Display` now emits the lowercase wire form instead, because that is
    /// what `from_string` can read back. The label and `description` below it
    /// are for a backend picker in the interface, and there is no picker:
    /// `available_backends` returns exactly one entry on every platform.
    ///
    /// Kept rather than deleted — they are the wording someone chose for that
    /// screen, and writing it again is the expensive part. `pub` on a `pub`
    /// enum, so the dead-code lint will never say any of this for you.
    pub fn name(&self) -> &'static str {
        match self {
            AudioCaptureBackend::ScreenCaptureKit => "ScreenCaptureKit",
            #[cfg(target_os = "macos")]
            AudioCaptureBackend::CoreAudio => "Core Audio",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            AudioCaptureBackend::ScreenCaptureKit => {
                "Apple's ScreenCaptureKit framework - Higher level API with good compatibility"
            }
            #[cfg(target_os = "macos")]
            AudioCaptureBackend::CoreAudio => {
                "Direct Core Audio API - Lower latency, more control over audio pipeline"
            }
        }
    }

    /// Get backend from string
    ///
    /// On macOS a stored `screencapturekit` value is deliberately resolved to
    /// Core Audio. This fork no longer offers the ScreenCaptureKit path, and
    /// mapping the old value keeps a previously saved configuration working
    /// instead of failing to parse and falling back to nothing.
    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            #[cfg(target_os = "macos")]
            "screencapturekit" | "coreaudio" | "core_audio" => Some(AudioCaptureBackend::CoreAudio),
            #[cfg(not(target_os = "macos"))]
            "screencapturekit" => Some(AudioCaptureBackend::ScreenCaptureKit),
            _ => None,
        }
    }

    /// Get all available backends for current platform
    ///
    /// macOS exposes Core Audio only. The ScreenCaptureKit variant stays in the
    /// enum so that stored settings keep parsing, but it is not offered as a
    /// choice: it captures through CPAL on a pinned unreleased revision, needs
    /// the broad screen-recording permission rather than the narrower audio
    /// capture one, and `audio/devices/fallback.rs` exists solely to work around
    /// its Bluetooth timing problems.
    pub fn available_backends() -> Vec<Self> {
        #[cfg(target_os = "macos")]
        {
            vec![AudioCaptureBackend::CoreAudio]
        }

        #[cfg(not(target_os = "macos"))]
        {
            vec![AudioCaptureBackend::ScreenCaptureKit]
        }
    }

}

impl Default for AudioCaptureBackend {
    /// Get default backend for current platform
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        return AudioCaptureBackend::CoreAudio;

        #[cfg(not(target_os = "macos"))]
        return AudioCaptureBackend::ScreenCaptureKit;
    }
}

/// Displays the same lowercase form the removed inherent `to_string` used to
/// return (e.g. "coreaudio"), not `name()`'s human-readable one ("Core
/// Audio"). `recording_preferences.rs` round-trips this string through
/// `from_string`, which only recognizes the lowercase form — formatting the
/// human-readable name here would silently break persisted backend selection.
impl std::fmt::Display for AudioCaptureBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AudioCaptureBackend::ScreenCaptureKit => "screencapturekit",
            #[cfg(target_os = "macos")]
            AudioCaptureBackend::CoreAudio => "coreaudio",
        };
        write!(f, "{}", s)
    }
}

/// Global backend configuration
pub struct BackendConfig {
    current_backend: RwLock<AudioCaptureBackend>,
}

impl BackendConfig {
    fn new() -> Self {
        Self {
            current_backend: RwLock::new(AudioCaptureBackend::default()),
        }
    }

    /// Get current backend
    pub fn get(&self) -> AudioCaptureBackend {
        *self.current_backend.read().unwrap()
    }

    /// Set current backend
    pub fn set(&self, backend: AudioCaptureBackend) {
        info!("Switching audio capture backend to: {:?}", backend);
        *self.current_backend.write().unwrap() = backend;
    }

    /// Get available backends
    pub fn available(&self) -> Vec<AudioCaptureBackend> {
        AudioCaptureBackend::available_backends()
    }

    /// Reset to default
    pub fn reset(&self) {
        self.set(AudioCaptureBackend::default());
    }
}

/// Global backend configuration instance
pub static BACKEND_CONFIG: Lazy<Arc<BackendConfig>> = Lazy::new(|| {
    Arc::new(BackendConfig::new())
});

/// Get current backend
pub fn get_current_backend() -> AudioCaptureBackend {
    BACKEND_CONFIG.get()
}

/// Set current backend
pub fn set_current_backend(backend: AudioCaptureBackend) {
    BACKEND_CONFIG.set(backend);
}

/// Get available backends
pub fn get_available_backends() -> Vec<AudioCaptureBackend> {
    BACKEND_CONFIG.available()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_to_string() {
        assert_eq!(AudioCaptureBackend::ScreenCaptureKit.to_string(), "screencapturekit");
        #[cfg(target_os = "macos")]
        assert_eq!(AudioCaptureBackend::CoreAudio.to_string(), "coreaudio");
    }

    #[test]
    fn test_backend_from_string() {
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            AudioCaptureBackend::from_string("screencapturekit"),
            Some(AudioCaptureBackend::ScreenCaptureKit)
        );

        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                AudioCaptureBackend::from_string("coreaudio"),
                Some(AudioCaptureBackend::CoreAudio)
            );
            assert_eq!(
                AudioCaptureBackend::from_string("core_audio"),
                Some(AudioCaptureBackend::CoreAudio)
            );
            // A configuration saved before ScreenCaptureKit was withdrawn must
            // keep parsing, resolving to the backend this fork actually uses.
            assert_eq!(
                AudioCaptureBackend::from_string("screencapturekit"),
                Some(AudioCaptureBackend::CoreAudio),
                "a previously stored 'screencapturekit' must resolve to Core Audio"
            );
        }

        assert_eq!(AudioCaptureBackend::from_string("nonsense"), None);
    }

    #[test]
    fn macos_offers_core_audio_only() {
        let backends = AudioCaptureBackend::available_backends();

        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                backends,
                vec![AudioCaptureBackend::CoreAudio],
                "macOS must expose Core Audio only"
            );
        }

        #[cfg(not(target_os = "macos"))]
        assert_eq!(backends, vec![AudioCaptureBackend::ScreenCaptureKit]);
    }

    #[test]
    fn test_available_backends() {
        let backends = AudioCaptureBackend::available_backends();

        #[cfg(target_os = "macos")]
        {
            assert!(backends.contains(&AudioCaptureBackend::CoreAudio));
            // ScreenCaptureKit is deliberately no longer offered on macOS,
            // see the rationale on `available_backends`.
            assert!(!backends.contains(&AudioCaptureBackend::ScreenCaptureKit));
        }

        #[cfg(not(target_os = "macos"))]
        assert!(backends.contains(&AudioCaptureBackend::ScreenCaptureKit));
    }

    #[test]
    fn test_default_backend() {
        #[cfg(target_os = "macos")]
        assert_eq!(AudioCaptureBackend::default(), AudioCaptureBackend::CoreAudio);

        #[cfg(not(target_os = "macos"))]
        assert_eq!(AudioCaptureBackend::default(), AudioCaptureBackend::ScreenCaptureKit);
    }

    #[test]
    fn test_backend_config() {
        let config = BackendConfig::new();

        // Should start with default
        #[cfg(target_os = "macos")]
        assert_eq!(config.get(), AudioCaptureBackend::CoreAudio);

        #[cfg(not(target_os = "macos"))]
        assert_eq!(config.get(), AudioCaptureBackend::ScreenCaptureKit);

        #[cfg(target_os = "macos")]
        {
            // Test setting CoreAudio
            config.set(AudioCaptureBackend::CoreAudio);
            assert_eq!(config.get(), AudioCaptureBackend::CoreAudio);
        }

        // Test reset
        config.reset();
        #[cfg(target_os = "macos")]
        assert_eq!(config.get(), AudioCaptureBackend::CoreAudio);

        #[cfg(not(target_os = "macos"))]
        assert_eq!(config.get(), AudioCaptureBackend::ScreenCaptureKit);
    }
}