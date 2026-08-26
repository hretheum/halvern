//! Whether audio is actually arriving, per source, and whether it is silence.
//!
//! On 26 August a recording ran for three minutes and forty-nine seconds while
//! the screen said "Listening for speech…". The log later showed why: the first
//! microphone sample reached the pipeline three minutes and twenty-three
//! seconds after the streams were opened, and two later recordings received
//! samples immediately whose every value was zero. Both were only visible
//! afterwards, by reading timestamps in a log file, and neither was visible to
//! the person in the meeting.
//!
//! Those are two different failures and the interface has to tell them apart:
//! *nothing is arriving* is a stream that never delivered, and *silence is
//! arriving* is a stream delivering zeroes. So this records both — how many
//! samples a source has produced, and how many of them carried any signal.
//!
//! Counters rather than timestamps, and atomics rather than a lock, because
//! this is updated from the audio callback. The caller polls and takes
//! differences; deciding what a gap *means* is a question for the interface,
//! not for the hot path.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use serde::Serialize;

use super::recording_state::DeviceType;

/// Below this a chunk is silence rather than a quiet room.
///
/// A real microphone in an empty room still delivers a noise floor several
/// orders of magnitude above this; what the failing recordings delivered was
/// exact zero. The threshold sits just above zero on purpose — it is here to
/// tolerate a denormal, not to judge whether a room is quiet.
const SIGNAL_FLOOR: f32 = 1e-5;

/// One source's counters. Everything is monotonic within a recording.
#[derive(Debug, Default)]
struct SourceCounters {
    /// Samples the device has delivered since the recording started.
    samples: AtomicU64,
    /// Of those, samples in chunks that carried something above the floor.
    signal_samples: AtomicU64,
    /// Highest absolute sample seen, in thousandths, so it fits an integer.
    peak_milli: AtomicU32,
}

impl SourceCounters {
    fn record(&self, sample_count: usize, peak: f32) {
        self.samples
            .fetch_add(sample_count as u64, Ordering::Relaxed);

        if peak > SIGNAL_FLOOR {
            self.signal_samples
                .fetch_add(sample_count as u64, Ordering::Relaxed);
        }

        let peak_milli = (peak * 1000.0).round().clamp(0.0, u32::MAX as f32) as u32;
        self.peak_milli.fetch_max(peak_milli, Ordering::Relaxed);
    }

    fn reset(&self) {
        self.samples.store(0, Ordering::Relaxed);
        self.signal_samples.store(0, Ordering::Relaxed);
        self.peak_milli.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> SourceActivity {
        SourceActivity {
            samples: self.samples.load(Ordering::Relaxed),
            signal_samples: self.signal_samples.load(Ordering::Relaxed),
            peak: self.peak_milli.load(Ordering::Relaxed) as f32 / 1000.0,
        }
    }
}

static MICROPHONE: SourceCounters = SourceCounters {
    samples: AtomicU64::new(0),
    signal_samples: AtomicU64::new(0),
    peak_milli: AtomicU32::new(0),
};

static SYSTEM: SourceCounters = SourceCounters {
    samples: AtomicU64::new(0),
    signal_samples: AtomicU64::new(0),
    peak_milli: AtomicU32::new(0),
};

fn counters(device_type: &DeviceType) -> &'static SourceCounters {
    match device_type {
        DeviceType::Microphone => &MICROPHONE,
        DeviceType::System => &SYSTEM,
    }
}

/// What one source has produced. Sent to the interface as it is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceActivity {
    pub samples: u64,
    pub signal_samples: u64,
    pub peak: f32,
}

/// Both sources, as of now, with the devices they were opened on.
///
/// The names are carried because the message this feeds has to name the thing
/// that is not working. "No audio is arriving" sends someone to check
/// permissions; "No audio is arriving from 'To moje'" sends them to their
/// headset, which on 26 August was the answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputActivity {
    pub microphone: SourceActivity,
    pub system: SourceActivity,
    pub microphone_device: Option<String>,
    pub system_device: Option<String>,
}

/// Called from the audio callback, once per chunk, before any processing.
///
/// `peak` is the largest absolute sample in the chunk. It is taken as an
/// argument rather than computed here because the caller has already walked
/// the buffer.
pub fn record_chunk(device_type: &DeviceType, sample_count: usize, peak: f32) {
    counters(device_type).record(sample_count, peak);
}

/// The devices this recording opened. Written once per recording, read once a
/// second, so a lock costs nothing and keeps the names honest.
static DEVICE_NAMES: std::sync::RwLock<(Option<String>, Option<String>)> =
    std::sync::RwLock::new((None, None));

/// Called when a recording starts, so counts describe this recording only.
pub fn reset() {
    MICROPHONE.reset();
    SYSTEM.reset();
}

/// Called when a recording starts, with whatever devices it managed to open.
pub fn set_devices(microphone: Option<String>, system: Option<String>) {
    if let Ok(mut names) = DEVICE_NAMES.write() {
        *names = (microphone, system);
    }
}

/// Everything both sources have produced since the last reset.
pub fn snapshot() -> InputActivity {
    let (microphone_device, system_device) = DEVICE_NAMES
        .read()
        .map(|names| names.clone())
        .unwrap_or((None, None));

    InputActivity {
        microphone: MICROPHONE.snapshot(),
        system: SYSTEM.snapshot(),
        microphone_device,
        system_device,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The statics are process-wide, so the tests take them one at a time.
    /// Each resets first and asserts only on what it wrote.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        reset();
        guard
    }

    #[test]
    fn a_stream_that_never_delivered_is_distinguishable_from_one_delivering_silence() {
        let _guard = guard();

        // Nothing has arrived at all.
        let nothing = snapshot();
        assert_eq!(nothing.microphone.samples, 0);
        assert_eq!(nothing.microphone.signal_samples, 0);

        // Zeroes have arrived — the case the log showed as `RMS 0.0%`.
        record_chunk(&DeviceType::Microphone, 480, 0.0);
        let silence = snapshot();
        assert_eq!(silence.microphone.samples, 480);
        assert_eq!(
            silence.microphone.signal_samples, 0,
            "silence must count as arrived, so the interface can say the stream is alive and empty"
        );
    }

    #[test]
    fn a_chunk_above_the_floor_counts_as_signal_and_raises_the_peak() {
        let _guard = guard();

        record_chunk(&DeviceType::Microphone, 480, 0.25);
        let activity = snapshot();

        assert_eq!(activity.microphone.samples, 480);
        assert_eq!(activity.microphone.signal_samples, 480);
        assert!((activity.microphone.peak - 0.25).abs() < 0.001);
    }

    #[test]
    fn the_peak_is_the_loudest_seen_not_the_most_recent() {
        let _guard = guard();

        record_chunk(&DeviceType::Microphone, 480, 0.8);
        record_chunk(&DeviceType::Microphone, 480, 0.1);

        assert!((snapshot().microphone.peak - 0.8).abs() < 0.001);
    }

    #[test]
    fn the_two_sources_are_counted_apart() {
        let _guard = guard();

        record_chunk(&DeviceType::Microphone, 480, 0.5);
        record_chunk(&DeviceType::System, 960, 0.0);

        let activity = snapshot();
        assert_eq!(activity.microphone.samples, 480);
        assert_eq!(activity.microphone.signal_samples, 480);
        assert_eq!(activity.system.samples, 960);
        assert_eq!(
            activity.system.signal_samples, 0,
            "a silent system tap must not borrow the microphone's signal"
        );
    }

    #[test]
    fn a_denormal_is_not_signal() {
        let _guard = guard();

        record_chunk(&DeviceType::Microphone, 480, 1e-9);

        let activity = snapshot();
        assert_eq!(activity.microphone.samples, 480);
        assert_eq!(
            activity.microphone.signal_samples, 0,
            "the floor exists to tolerate numerical dust, not to judge a quiet room"
        );
    }

    #[test]
    fn the_snapshot_names_the_devices_the_recording_opened() {
        let _guard = guard();
        set_devices(Some("To moje".to_string()), None);

        let activity = snapshot();
        assert_eq!(activity.microphone_device.as_deref(), Some("To moje"));
        assert_eq!(
            activity.system_device, None,
            "a recording with no system tap must not invent one"
        );
    }

    #[test]
    fn reset_clears_both_sources_so_counts_describe_one_recording() {
        let _guard = guard();

        record_chunk(&DeviceType::Microphone, 480, 0.5);
        record_chunk(&DeviceType::System, 480, 0.5);
        reset();

        let activity = snapshot();
        assert_eq!(activity.microphone.samples, 0);
        assert_eq!(activity.microphone.peak, 0.0);
        assert_eq!(activity.system.samples, 0);
        assert_eq!(activity.system.peak, 0.0);
    }
}
