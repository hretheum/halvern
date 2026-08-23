//! Writes every capture source to its own WAV file, before any processing.
//!
//! Purpose is diagnostic, not archival. Everything downstream — resampling,
//! noise suppression, mixing, VAD, transcription — becomes a pure function of
//! these files, so a fault can be reproduced offline instead of requiring
//! another live meeting. A night was spent asking the user to place real calls
//! because this did not exist.
//!
//! Deliberately global state. Threading a path through `start_streams`,
//! `create_cpal_stream`, `create_core_audio_stream` and `AudioCapture::new`
//! would touch four signatures and two call sites for a diagnostic aid; the
//! surrounding module already keeps recording state in statics.
//!
//! Failures are logged and swallowed. A tap that cannot write must never
//! disturb the recording it is observing.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{info, warn};
use once_cell::sync::Lazy;

use super::recording_state::DeviceType;

/// 16-bit PCM mono. At 48 kHz that is 5.6 MB per minute per source.
const BITS_PER_SAMPLE: u16 = 16;
const WAV_HEADER_BYTES: u32 = 44;

struct RawWav {
    path: PathBuf,
    writer: BufWriter<File>,
    sample_rate: u32,
    frames: u32,
    /// Level reporting, so "we are recording silence" is visible within seconds
    /// instead of after the meeting. Two recordings were lost to exactly that.
    frames_since_report: u32,
    peak_since_report: f32,
    energy_since_report: f64,
    reports: u32,
}

impl RawWav {
    fn create(path: PathBuf, sample_rate: u32) -> std::io::Result<Self> {
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);

        // Sizes are patched in `finish`; a truncated file still plays in most
        // tools because the header is structurally valid from the start.
        write_header(&mut writer, sample_rate, 0)?;

        Ok(Self {
            path,
            writer,
            sample_rate,
            frames: 0,
            frames_since_report: 0,
            peak_since_report: 0.0,
            energy_since_report: 0.0,
            reports: 0,
        })
    }

    fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let pcm = (clamped * i16::MAX as f32) as i16;
            self.writer.write_all(&pcm.to_le_bytes())?;

            self.peak_since_report = self.peak_since_report.max(clamped.abs());
            self.energy_since_report += (clamped as f64) * (clamped as f64);
        }

        self.frames += samples.len() as u32;
        self.frames_since_report += samples.len() as u32;
        self.report_level_if_due();
        Ok(())
    }

    /// Every five seconds, states plainly whether this source carries audio.
    fn report_level_if_due(&mut self) {
        let interval = self.sample_rate.max(1) * 5;
        if self.frames_since_report < interval {
            return;
        }

        let rms = (self.energy_since_report / self.frames_since_report as f64).sqrt();
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if self.peak_since_report == 0.0 {
            // Not "quiet" — every sample was exactly zero. Core Audio reports a
            // denied capture permission this way, and so does a device that has
            // gone away.
            warn!(
                "🔇 {} has delivered PURE DIGITAL SILENCE for 5 s ({} s in total). \
                 Check the capture permission and whether the device is still connected.",
                name,
                self.frames / self.sample_rate.max(1)
            );
        } else {
            info!(
                "🎧 {}: peak {:.3}, RMS {:.4} over the last 5 s",
                name, self.peak_since_report, rms
            );
        }

        self.frames_since_report = 0;
        self.peak_since_report = 0.0;
        self.energy_since_report = 0.0;
        self.reports += 1;
    }

    fn finish(mut self) -> std::io::Result<(PathBuf, f64)> {
        self.writer.flush()?;
        let mut file = self.writer.into_inner()?;

        let data_bytes = self.frames * (BITS_PER_SAMPLE as u32 / 8);
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&(WAV_HEADER_BYTES - 8 + data_bytes).to_le_bytes())?;
        file.seek(SeekFrom::Start(40))?;
        file.write_all(&data_bytes.to_le_bytes())?;
        file.flush()?;

        let seconds = self.frames as f64 / self.sample_rate.max(1) as f64;
        Ok((self.path, seconds))
    }
}

fn write_header<W: Write>(w: &mut W, sample_rate: u32, data_bytes: u32) -> std::io::Result<()> {
    let channels: u16 = 1;
    let byte_rate = sample_rate * channels as u32 * (BITS_PER_SAMPLE as u32 / 8);
    let block_align = channels * (BITS_PER_SAMPLE / 8);

    w.write_all(b"RIFF")?;
    w.write_all(&(WAV_HEADER_BYTES - 8 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // PCM chunk size
    w.write_all(&1u16.to_le_bytes())?; // format: PCM
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&BITS_PER_SAMPLE.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

#[derive(Default)]
struct Taps {
    dir: Option<PathBuf>,
    mic: Option<RawWav>,
    system: Option<RawWav>,
}

static TAPS: Lazy<Mutex<Taps>> = Lazy::new(|| Mutex::new(Taps::default()));

/// Arms the tap for a recording. Files appear on first sample from each source,
/// because only then is the device sample rate known.
pub fn begin(meeting_folder: &Path) {
    match TAPS.lock() {
        Ok(mut taps) => {
            *taps = Taps {
                dir: Some(meeting_folder.to_path_buf()),
                mic: None,
                system: None,
            };
            info!("🎧 Raw tap armed in {}", meeting_folder.display());
        }
        Err(e) => warn!("Raw tap could not be armed: {}", e),
    }
}

/// Records one block exactly as the device delivered it, after channel
/// downmix but before resampling, filtering, noise suppression and mixing.
pub fn write(device_type: &DeviceType, sample_rate: u32, samples: &[f32]) {
    if samples.is_empty() {
        return;
    }

    let Ok(mut taps) = TAPS.lock() else { return };
    let Some(dir) = taps.dir.clone() else { return };

    let (slot, name) = match device_type {
        DeviceType::Microphone => (&mut taps.mic, "raw-microphone.wav"),
        DeviceType::System => (&mut taps.system, "raw-system.wav"),
    };

    if slot.is_none() {
        match RawWav::create(dir.join(name), sample_rate) {
            Ok(w) => {
                info!("🎧 Raw tap writing {} at {} Hz", name, sample_rate);
                *slot = Some(w);
            }
            Err(e) => {
                warn!("Raw tap could not create {}: {}", name, e);
                return;
            }
        }
    }

    if let Some(w) = slot.as_mut() {
        if let Err(e) = w.write(samples) {
            warn!("Raw tap write failed for {}: {}", name, e);
        }
    }
}

/// Closes both files and patches their headers. Logs the resulting durations,
/// which is the cheapest way to see that two sources drifted apart.
pub fn finish() {
    let Ok(mut taps) = TAPS.lock() else { return };

    let mic = taps.mic.take();
    let system = taps.system.take();
    taps.dir = None;

    let mut report = Vec::new();

    for wav in [mic, system].into_iter().flatten() {
        let rate = wav.sample_rate;
        match wav.finish() {
            Ok((path, seconds)) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                report.push(format!("{} = {:.2}s @ {}Hz", name, seconds, rate));
            }
            Err(e) => warn!("Raw tap could not close a file: {}", e),
        }
    }

    if !report.is_empty() {
        // Unequal durations here mean the sources drifted, which is exactly the
        // condition that used to be invisible until a transcript came out empty.
        info!("🎧 Raw tap closed: {}", report.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_patched_with_the_real_length() {
        let dir = std::env::temp_dir().join("halvern-rawtap-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.wav");

        let mut wav = RawWav::create(path.clone(), 16_000).unwrap();
        wav.write(&vec![0.5; 16_000]).unwrap(); // exactly one second
        let (_, seconds) = wav.finish().unwrap();

        assert!((seconds - 1.0).abs() < 1e-6, "duration: {}", seconds);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");

        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data_len, 32_000, "16-bit mono, 16000 samples");
        assert_eq!(bytes.len() as u32, WAV_HEADER_BYTES + data_len);

        let riff_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(riff_len, WAV_HEADER_BYTES - 8 + data_len);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn samples_are_clamped_rather_than_wrapped() {
        let dir = std::env::temp_dir().join("halvern-rawtap-clamp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clamp.wav");

        let mut wav = RawWav::create(path.clone(), 16_000).unwrap();
        // Values beyond ±1.0 must saturate, not wrap around to the opposite sign.
        wav.write(&[2.0, -2.0]).unwrap();
        wav.finish().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let first = i16::from_le_bytes(bytes[44..46].try_into().unwrap());
        let second = i16::from_le_bytes(bytes[46..48].try_into().unwrap());
        assert!(first > 32_000, "positive clipping must stay positive");
        assert!(second < -32_000, "negative must stay negative");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
