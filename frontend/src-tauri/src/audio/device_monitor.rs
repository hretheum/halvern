// Audio device monitoring for disconnect/reconnect detection
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use anyhow::Result;
use log::{debug, info, warn, error};

use super::devices::{AudioDevice, default_input_device, list_audio_devices};

/// The microphone a recording is currently using, shared with whoever can
/// change it.
///
/// The monitor needs this to answer "is the system's default still the device
/// we are recording?", and the answer has to move when a switch succeeds —
/// otherwise the monitor reports the same change forever.
pub type ActiveMicrophone = Arc<std::sync::RwLock<Option<String>>>;

/// Device monitoring events
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// A device that was in use has disconnected
    DeviceDisconnected {
        device_name: String,
        device_type: DeviceMonitorType,
    },
    /// A previously disconnected device has reconnected
    DeviceReconnected {
        device_name: String,
        device_type: DeviceMonitorType,
    },
    /// Device list has changed (new device added or removed)
    DeviceListChanged,
    /// The system's default input is no longer the microphone being recorded.
    ///
    /// Distinct from a disconnect: the device in use is still there and still
    /// working, and something else has become the machine's default — a
    /// headset connecting, most often, which macOS makes the default input the
    /// moment it appears.
    DefaultInputChanged { device_name: String },
}

/// Type of device being monitored
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceMonitorType {
    Microphone,
    SystemAudio,
}

/// Monitor state for a single device
#[derive(Debug, Clone)]
struct MonitoredDevice {
    name: String,
    device_type: DeviceMonitorType,
    consecutive_missing: u32,
    is_bluetooth: bool,
}

impl MonitoredDevice {
    fn new(name: String, device_type: DeviceMonitorType) -> Self {
        // Heuristic: check if device name contains bluetooth-related keywords
        let is_bluetooth = name.to_lowercase().contains("airpods")
            || name.to_lowercase().contains("bluetooth")
            || name.to_lowercase().contains("wireless");

        Self {
            name,
            device_type,
            consecutive_missing: 0,
            is_bluetooth,
        }
    }

    /// Get appropriate disconnect threshold based on device type
    fn disconnect_threshold(&self) -> u32 {
        // Bluetooth devices get more grace period (they can briefly disconnect)
        if self.is_bluetooth {
            3 // 3 polling cycles (6-15 seconds)
        } else {
            2 // 2 polling cycles (4-10 seconds)
        }
    }

    /// Get appropriate reconnect check interval
    #[allow(dead_code)]
    fn reconnect_interval(&self) -> Duration {
        if self.is_bluetooth {
            Duration::from_secs(5) // Check every 5s for Bluetooth
        } else {
            Duration::from_secs(3) // Check every 3s for wired devices
        }
    }
}

/// Audio device monitor that detects disconnects and reconnects
pub struct AudioDeviceMonitor {
    monitor_handle: Option<JoinHandle<()>>,
    event_sender: mpsc::UnboundedSender<DeviceEvent>,
    stop_signal: Arc<tokio::sync::Notify>,
}

impl AudioDeviceMonitor {
    /// Create a new device monitor
    pub fn new() -> (Self, mpsc::UnboundedReceiver<DeviceEvent>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let stop_signal = Arc::new(tokio::sync::Notify::new());

        (
            Self {
                monitor_handle: None,
                event_sender,
                stop_signal,
            },
            event_receiver,
        )
    }

    /// Start monitoring specified devices
    pub fn start_monitoring(
        &mut self,
        microphone: Option<Arc<AudioDevice>>,
        system_audio: Option<Arc<AudioDevice>>,
        active_microphone: Option<ActiveMicrophone>,
    ) -> Result<()> {
        if self.monitor_handle.is_some() {
            warn!("Device monitor already running");
            return Ok(());
        }

        let mut monitored_devices = Vec::new();

        if let Some(mic) = microphone {
            monitored_devices.push(MonitoredDevice::new(
                mic.name.clone(),
                DeviceMonitorType::Microphone,
            ));
            info!("🔍 Monitoring microphone: '{}' (Bluetooth: {})",
                  mic.name, monitored_devices.last().unwrap().is_bluetooth);
        }

        if let Some(sys) = system_audio {
            monitored_devices.push(MonitoredDevice::new(
                sys.name.clone(),
                DeviceMonitorType::SystemAudio,
            ));
            info!("🔍 Monitoring system audio: '{}' (Bluetooth: {})",
                  sys.name, monitored_devices.last().unwrap().is_bluetooth);
        }

        if monitored_devices.is_empty() {
            return Err(anyhow::anyhow!("No devices to monitor"));
        }

        let event_sender = self.event_sender.clone();
        let stop_signal = self.stop_signal.clone();

        let handle = tokio::spawn(async move {
            Self::monitor_loop(monitored_devices, event_sender, stop_signal, active_microphone)
                .await;
        });

        self.monitor_handle = Some(handle);
        info!("✅ Device monitor started");
        Ok(())
    }

    /// Stop monitoring
    pub async fn stop_monitoring(&mut self) {
        info!("Stopping device monitor");
        self.stop_signal.notify_one();

        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.await;
        }

        info!("Device monitor stopped");
    }

    /// Main monitoring loop
    async fn monitor_loop(
        mut monitored_devices: Vec<MonitoredDevice>,
        event_sender: mpsc::UnboundedSender<DeviceEvent>,
        stop_signal: Arc<tokio::sync::Notify>,
        active_microphone: Option<ActiveMicrophone>,
    ) {
        let mut last_device_list = Vec::new();
        // The default already reported, so one headset connecting produces one
        // event rather than one every polling cycle.
        let mut reported_default: Option<String> = None;
        let check_interval = Duration::from_secs(2); // Poll every 2 seconds

        loop {
            // Check for stop signal with timeout
            tokio::select! {
                _ = stop_signal.notified() => {
                    info!("Device monitor received stop signal");
                    break;
                }
                _ = tokio::time::sleep(check_interval) => {
                    // Continue with monitoring check
                }
            }

            // Get current device list
            let current_devices = match list_audio_devices().await {
                Ok(devices) => devices,
                Err(e) => {
                    error!("Failed to list audio devices: {}", e);
                    continue;
                }
            };

            // Check if device list changed
            if current_devices.len() != last_device_list.len() {
                debug!("Device list changed: {} -> {} devices",
                       last_device_list.len(), current_devices.len());
                let _ = event_sender.send(DeviceEvent::DeviceListChanged);
            }
            last_device_list = current_devices.clone();

            // Has something else become the machine's default input? This is
            // not a disconnect — the device in use is still present and still
            // delivering — so it is checked separately from the loop below.
            if let Some(active) = active_microphone.as_ref() {
                let in_use = active.read().ok().and_then(|name| name.clone());
                let system_default = default_input_device().ok().map(|device| device.name);

                match default_change_to_report(
                    system_default.as_deref(),
                    in_use.as_deref(),
                    &current_devices,
                    reported_default.as_deref(),
                ) {
                    DefaultChange::Report(device_name) => {
                        info!(
                            "🎤 System default input is now '{}', recording is on {:?}",
                            device_name, in_use
                        );
                        let _ = event_sender.send(DeviceEvent::DefaultInputChanged {
                            device_name: device_name.clone(),
                        });
                        reported_default = Some(device_name);
                    }
                    DefaultChange::Nothing => {}
                    DefaultChange::Forget => reported_default = None,
                }
            }

            // Check each monitored device
            for monitored in &mut monitored_devices {
                let device_found = current_devices.iter().any(|d| d.name == monitored.name);

                if device_found {
                    // Device is present
                    if monitored.consecutive_missing > 0 {
                        // Device has reconnected!
                        info!("✅ Device '{}' reconnected after {} missing checks",
                              monitored.name, monitored.consecutive_missing);

                        let _ = event_sender.send(DeviceEvent::DeviceReconnected {
                            device_name: monitored.name.clone(),
                            device_type: monitored.device_type.clone(),
                        });

                        monitored.consecutive_missing = 0;
                    }
                } else {
                    // Device is missing
                    monitored.consecutive_missing += 1;

                    debug!("⚠️ Device '{}' missing for {} checks (threshold: {})",
                          monitored.name, monitored.consecutive_missing,
                          monitored.disconnect_threshold());

                    // Only emit disconnect event once when threshold is reached
                    if monitored.consecutive_missing == monitored.disconnect_threshold() {
                        warn!("❌ Device '{}' ({:?}) disconnected!",
                              monitored.name, monitored.device_type);

                        let _ = event_sender.send(DeviceEvent::DeviceDisconnected {
                            device_name: monitored.name.clone(),
                            device_type: monitored.device_type.clone(),
                        });
                    }
                }
            }

            // Adjust check interval based on device states
            // If any device is missing, check more frequently
            let has_missing = monitored_devices.iter().any(|d| d.consecutive_missing > 0);
            let next_interval = if has_missing {
                Duration::from_secs(2) // Fast polling when device missing
            } else {
                Duration::from_secs(5) // Slower polling when all devices present
            };

            if next_interval != check_interval {
                debug!("Adjusting monitor interval to {:?}", next_interval);
            }
        }
    }
}

/// What to do about the system default, having looked at it.
#[derive(Debug, PartialEq)]
enum DefaultChange {
    /// Say so, and remember having said it.
    Report(String),
    /// Nothing to say, and nothing to forget.
    Nothing,
    /// The default agrees with what is in use again, so a later move counts as
    /// new. Without this, unplugging a headset and plugging it back in during
    /// one recording would be reported once and then never again.
    Forget,
}

/// Whether a default that differs from the microphone in use is worth an event.
///
/// Pure, because every condition here is a judgement that has to be right and
/// none of them need a machine to decide: the device has to actually be in the
/// list, since a name in the default slot that nothing can open is not
/// something to switch to; and the same move must not be reported on every
/// polling cycle.
fn default_change_to_report(
    system_default: Option<&str>,
    in_use: Option<&str>,
    available: &[AudioDevice],
    already_reported: Option<&str>,
) -> DefaultChange {
    let (Some(default_name), Some(in_use_name)) = (system_default, in_use) else {
        return DefaultChange::Nothing;
    };

    if default_name == in_use_name {
        return DefaultChange::Forget;
    }

    if already_reported == Some(default_name) {
        return DefaultChange::Nothing;
    }

    if !available.iter().any(|device| device.name == default_name) {
        return DefaultChange::Nothing;
    }

    DefaultChange::Report(default_name.to_string())
}

impl Default for AudioDeviceMonitor {
    fn default() -> Self {
        Self::new().0
    }
}

impl Drop for AudioDeviceMonitor {
    fn drop(&mut self) {
        // Signal stop
        self.stop_signal.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bluetooth_detection() {
        let airpods = MonitoredDevice::new(
            "John's AirPods Pro".to_string(),
            DeviceMonitorType::Microphone,
        );
        assert!(airpods.is_bluetooth);
        assert_eq!(airpods.disconnect_threshold(), 3);

        let builtin = MonitoredDevice::new(
            "Built-in Microphone".to_string(),
            DeviceMonitorType::Microphone,
        );
        assert!(!builtin.is_bluetooth);
        assert_eq!(builtin.disconnect_threshold(), 2);
    }

    fn device(name: &str) -> AudioDevice {
        AudioDevice::new(name.to_string(), super::super::devices::DeviceType::Input)
    }

    #[test]
    fn a_new_default_that_can_be_opened_is_reported_once() {
        let available = vec![device("Mikrofon (MacBook Air)"), device("To moje")];

        assert_eq!(
            default_change_to_report(
                Some("To moje"),
                Some("Mikrofon (MacBook Air)"),
                &available,
                None
            ),
            DefaultChange::Report("To moje".to_string())
        );

        // Same answer on the next polling cycle: already said.
        assert_eq!(
            default_change_to_report(
                Some("To moje"),
                Some("Mikrofon (MacBook Air)"),
                &available,
                Some("To moje")
            ),
            DefaultChange::Nothing
        );
    }

    #[test]
    fn a_default_that_is_not_in_the_device_list_is_not_worth_switching_to() {
        let available = vec![device("Mikrofon (MacBook Air)")];

        assert_eq!(
            default_change_to_report(
                Some("To moje"),
                Some("Mikrofon (MacBook Air)"),
                &available,
                None
            ),
            DefaultChange::Nothing,
            "a name in the default slot that nothing can open is not a device"
        );
    }

    #[test]
    fn agreement_clears_the_memory_so_the_next_move_counts_as_new() {
        let available = vec![device("To moje")];

        assert_eq!(
            default_change_to_report(Some("To moje"), Some("To moje"), &available, Some("To moje")),
            DefaultChange::Forget,
            "unplugging a headset and plugging it back in must be reported both times"
        );
    }

    #[test]
    fn nothing_is_reported_when_either_side_is_unknown() {
        let available = vec![device("To moje")];

        assert_eq!(
            default_change_to_report(None, Some("Mikrofon (MacBook Air)"), &available, None),
            DefaultChange::Nothing
        );
        assert_eq!(
            default_change_to_report(Some("To moje"), None, &available, None),
            DefaultChange::Nothing
        );
    }

    #[tokio::test]
    async fn test_monitor_creation() {
        let (mut monitor, _receiver) = AudioDeviceMonitor::new();
        assert!(monitor.monitor_handle.is_none());

        // Stop should be safe even if not started
        monitor.stop_monitoring().await;
    }
}
