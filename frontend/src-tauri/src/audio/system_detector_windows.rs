//! Which applications are using the audio hardware, on Windows.
//!
//! The macOS half of this lives in [`super::system_detector`] and listens to a
//! Core Audio property. This one asks WASAPI instead, and asks repeatedly
//! rather than being told.
//!
//! # Why polling rather than `IAudioSessionNotification`
//!
//! WASAPI can push session changes, but only through a COM callback object
//! that has to be registered, kept alive, and released on the same apartment
//! it was created in. The policy layer above does not need that precision: it
//! debounces a candidate for `min_duration` seconds and re-confirms with
//! `snapshot_audio_apps()` before it acts. A two-second poll is well inside
//! that tolerance, and it fails in a way a reader can follow.
//!
//! # What counts as "using audio"
//!
//! An audio session belongs to a process and has a state. Only
//! `AudioSessionStateActive` counts — Windows keeps inactive sessions listed
//! for a while after a sound stops, and treating those as live would start a
//! recording every time a notification chimed.
//!
//! Render sessions set `uses_output`, capture sessions set `uses_input`, and
//! the two are merged per process id. That pairing is what separates a meeting
//! from music: a player only renders, a call does both.
//!
//! # `bundle_id` on a platform with no bundles
//!
//! [`DetectedApp::bundle_id`] is a macOS reverse-DNS identifier. Windows has
//! no equivalent, so this fills it with the executable's stem, lowercased:
//! `ms-teams`, `zoom`, `slack`, `chrome`. That keeps the field's contract —
//! a stable, non-localised identity — and lets `detection::policy` match
//! against its configured lists unchanged. `DEFAULT_ALWAYS_MEETING` carries
//! Windows entries beside the macOS ones for exactly this reason.
//!
//! Leaving it `None` would still work, but only as a *generic* candidate:
//! `pick_candidate` can only recognise a known meeting application through
//! this field, and known applications are the ones that may start a recording
//! on output alone, before the microphone joins.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use log::{debug, info, warn};

use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, AudioSessionStateActive, EDataFlow, IAudioSessionControl2,
    IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::system_detector::{BackgroundTask, DetectedApp, SystemAudioCallback, SystemAudioEvent};

/// How often the session list is re-read.
///
/// The policy layer's shortest meaningful window is `min_duration`, which
/// defaults to fifteen seconds, so this only has to be small enough that a
/// call is noticed promptly and large enough that enumerating a handful of
/// COM objects is not something the machine notices.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Per-process audio activity, keyed by process id.
#[derive(Default, Clone, Copy)]
struct Activity {
    input: bool,
    output: bool,
}

/// Reads the executable name for a process id, without its path or extension.
///
/// `PROCESS_QUERY_LIMITED_INFORMATION` is the narrowest right that answers
/// this, and unlike `PROCESS_QUERY_INFORMATION` it is granted for processes
/// running at a higher integrity level. Failure is normal rather than
/// exceptional: the process may have exited between the enumeration and this
/// call, and system sessions have no readable image at all.
fn process_stem(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }

    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buffer = [0u16; MAX_PATH as usize];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(handle);
        result.ok()?;

        let full = String::from_utf16_lossy(&buffer[..length as usize]);
        let file = full.rsplit(['\\', '/']).next()?;
        let stem = file.strip_suffix(".exe").unwrap_or(file);
        if stem.is_empty() {
            None
        } else {
            Some(stem.to_lowercase())
        }
    }
}

/// Collects the process ids with an active session on one direction of the
/// default endpoint.
///
/// Only the default endpoint is inspected, not every device. A meeting plays
/// through whatever the user is listening on, which is the default by
/// definition; walking every endpoint would add devices nobody is using and
/// each one costs a COM activation.
unsafe fn active_pids(enumerator: &IMMDeviceEnumerator, flow: EDataFlow) -> Vec<u32> {
    let device = match enumerator.GetDefaultAudioEndpoint(flow, eConsole) {
        Ok(device) => device,
        // No default endpoint is an ordinary state: a machine with no
        // microphone answers this way, and so does one whose only output was
        // just unplugged.
        Err(e) => {
            debug!("No default audio endpoint for {:?}: {}", flow, e);
            return Vec::new();
        }
    };

    let manager: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
        Ok(manager) => manager,
        Err(e) => {
            warn!(
                "Could not open the audio session manager for {:?}: {}",
                flow, e
            );
            return Vec::new();
        }
    };

    let sessions = match manager.GetSessionEnumerator() {
        Ok(sessions) => sessions,
        Err(e) => {
            warn!("Could not enumerate audio sessions for {:?}: {}", flow, e);
            return Vec::new();
        }
    };

    let count = sessions.GetCount().unwrap_or(0);
    let mut pids = Vec::new();

    for index in 0..count {
        let control = match sessions.GetSession(index) {
            Ok(control) => control,
            Err(_) => continue,
        };

        match control.GetState() {
            Ok(state) if state == AudioSessionStateActive => {}
            // Inactive and expired sessions linger after the sound stops.
            _ => continue,
        }

        let control2: IAudioSessionControl2 = match control.cast() {
            Ok(control2) => control2,
            Err(_) => continue,
        };

        // The system session carries process id 0 and represents Windows'
        // own sounds. It is not an application and must not start a recording.
        if control2.IsSystemSoundsSession().is_ok() {
            continue;
        }

        if let Ok(pid) = control2.GetProcessId() {
            if pid != 0 {
                pids.push(pid);
            }
        }
    }

    pids
}

/// Every application with an active audio session right now.
///
/// Returns an empty list rather than an error on any failure. A detector that
/// reports "nothing is happening" when it cannot see is wrong in the safe
/// direction: it declines to start a recording. One that guessed the other way
/// would record a meeting that was not happening.
pub fn list_audio_sessions() -> Vec<DetectedApp> {
    unsafe {
        // Each call initialises its own apartment, because this runs from a
        // polling thread that does not own one. `RPC_E_CHANGED_MODE` means the
        // thread already has an apartment of a different kind, which is
        // harmless here — the calls below work either way — so it is not
        // treated as a failure.
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let owns_com = com.is_ok();

        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(enumerator) => enumerator,
                Err(e) => {
                    warn!("Could not create the multimedia device enumerator: {}", e);
                    if owns_com {
                        CoUninitialize();
                    }
                    return Vec::new();
                }
            };

        let mut activity: HashMap<u32, Activity> = HashMap::new();
        for pid in active_pids(&enumerator, eRender) {
            activity.entry(pid).or_default().output = true;
        }
        for pid in active_pids(&enumerator, eCapture) {
            activity.entry(pid).or_default().input = true;
        }

        if owns_com {
            CoUninitialize();
        }

        activity
            .into_iter()
            .filter_map(|(pid, seen)| {
                let stem = process_stem(pid)?;
                Some(DetectedApp {
                    bundle_id: Some(stem.clone()),
                    name: stem,
                    uses_input: seen.input,
                    uses_output: seen.output,
                })
            })
            .collect()
    }
}

/// Polls WASAPI and reports transitions in the shape the policy layer expects.
#[derive(Default)]
pub struct WindowsSystemAudioDetector {
    background: BackgroundTask,
}

impl WindowsSystemAudioDetector {
    pub fn start(&mut self, callback: SystemAudioCallback) {
        info!(
            "Starting WASAPI audio session polling every {:?}",
            POLL_INTERVAL
        );

        self.background.start(move |running, mut stop_rx| {
            Box::pin(async move {
                // `false` until the first non-empty poll, so a start event is
                // emitted once per period of activity rather than every tick.
                let mut was_active = false;

                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        _ = tokio::time::sleep(POLL_INTERVAL) => {}
                    }

                    if !running.load(Ordering::SeqCst) {
                        break;
                    }

                    // COM calls block, and blocking the async runtime here
                    // would stall every other task for the duration.
                    let apps = match tokio::task::spawn_blocking(list_audio_sessions).await {
                        Ok(apps) => apps,
                        Err(e) => {
                            warn!("Audio session poll failed to run: {}", e);
                            continue;
                        }
                    };

                    let active = !apps.is_empty();
                    if active {
                        // Re-sent on every tick while activity continues. The
                        // policy layer is built around repeated observations —
                        // it is how a candidate accumulates its duration — so
                        // a single edge-triggered event would starve it.
                        callback(SystemAudioEvent::SystemAudioStarted(apps));
                    } else if was_active {
                        callback(SystemAudioEvent::SystemAudioStopped);
                    }
                    was_active = active;
                }

                info!("WASAPI audio session polling stopped");
            })
        });
    }

    pub fn stop(&mut self) {
        self.background.stop();
    }
}
