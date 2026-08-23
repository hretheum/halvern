//! Markdown export of meetings into an Obsidian vault.
//!
//! Two notes are produced per meeting, `<stem>-transcript.md` and
//! `<stem>-summary.md`, where the stem is `YYYY-MM-DD-<slug>`. Existing files
//! are never overwritten: a numeric suffix is appended instead, because notes
//! in a vault are routinely edited by hand and clobbering them destroys work.

pub mod commands;
pub mod markdown;

use std::path::PathBuf;

use sqlx::{Row, SqlitePool};

pub use markdown::{render_meeting, slugify, ExportBundle, ExportMeeting, ExportSegment};

/// Highest suffix tried before giving up, so a broken caller cannot spin
/// forever creating files.
const MAX_COLLISION_ATTEMPTS: u32 = 999;

/// Where each kind of note is written.
///
/// The two destinations are independent so that summaries can live in a synced
/// vault while transcripts, which carry raw client speech, stay on a local-only
/// disk. When both resolve to the same directory the behaviour is identical to
/// a single-folder export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportTargets {
    pub transcript_dir: PathBuf,
    pub summary_dir: PathBuf,
}

impl ExportTargets {
    /// Builds targets from stored settings, applying the fallback rules:
    /// an empty or absent override means "use the base vault path".
    pub fn resolve(
        vault_path: &str,
        transcript_override: Option<&str>,
        summary_override: Option<&str>,
    ) -> Self {
        let pick = |candidate: Option<&str>| -> PathBuf {
            candidate
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(vault_path))
        };

        Self {
            transcript_dir: pick(transcript_override),
            summary_dir: pick(summary_override),
        }
    }

    /// True when both notes land in the same directory.
    pub fn is_single_directory(&self) -> bool {
        self.transcript_dir == self.summary_dir
    }
}

/// Builds the two candidate filenames for a given suffix index.
/// Index 1 means "no suffix".
fn candidate_names(stem: &str, index: u32) -> (String, String) {
    let suffix = if index <= 1 {
        String::new()
    } else {
        format!("-{}", index)
    };
    (
        format!("{}-transcript{}.md", stem, suffix),
        format!("{}-summary{}.md", stem, suffix),
    )
}

/// Finds the lowest suffix index for which every note this bundle needs is
/// still free, checking each name in its own destination directory.
///
/// Transcript and summary share one index so that a meeting's two notes always
/// carry matching names, even when they live in different folders. Taking the
/// index per directory would let a pair drift apart into `-transcript.md` next
/// to `-summary-2.md`, and nothing would tell you they belong together.
fn resolve_free_index(targets: &ExportTargets, bundle: &ExportBundle) -> Result<u32, String> {
    for index in 1..=MAX_COLLISION_ATTEMPTS {
        let (transcript_name, summary_name) = candidate_names(&bundle.stem, index);

        let transcript_free = !targets.transcript_dir.join(&transcript_name).exists();
        let summary_free = bundle.summary_md.is_none()
            || !targets.summary_dir.join(&summary_name).exists();

        if transcript_free && summary_free {
            return Ok(index);
        }
    }

    Err(format!(
        "No free name found for '{}' after {} attempts",
        bundle.stem, MAX_COLLISION_ATTEMPTS
    ))
}

/// Writes a rendered bundle into its destination directories, creating them if
/// needed.
///
/// Returns the paths actually written, in the order transcript, summary.
pub fn write_bundle(
    targets: &ExportTargets,
    bundle: &ExportBundle,
) -> Result<Vec<PathBuf>, String> {
    if targets.transcript_dir.as_os_str().is_empty()
        || targets.summary_dir.as_os_str().is_empty()
    {
        return Err("The export destination path is empty".to_string());
    }

    for dir in [&targets.transcript_dir, &targets.summary_dir] {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory {}: {}", dir.display(), e))?;
    }

    let index = resolve_free_index(targets, bundle)?;
    let (transcript_name, summary_name) = candidate_names(&bundle.stem, index);

    let mut written = Vec::with_capacity(2);

    let transcript_path = targets.transcript_dir.join(&transcript_name);
    std::fs::write(&transcript_path, &bundle.transcript_md)
        .map_err(|e| format!("Writing {} failed: {}", transcript_path.display(), e))?;
    written.push(transcript_path);

    if let Some(summary) = &bundle.summary_md {
        let summary_path = targets.summary_dir.join(&summary_name);
        std::fs::write(&summary_path, summary)
            .map_err(|e| format!("Writing {} failed: {}", summary_path.display(), e))?;
        written.push(summary_path);
    }

    Ok(written)
}

/// Pulls the Markdown body out of a `summary_processes.result` JSON blob.
///
/// The blob is produced by `summary::service::build_summary_result_json` and
/// carries the rendered note under `markdown`, alongside a translation cache
/// that must not end up in the vault.
pub fn extract_summary_markdown(result_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(result_json).ok()?;
    value
        .get("markdown")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Loads everything the renderer needs for one meeting.
///
/// Reads the `speaker` column directly via raw SQL. Since `71933f2`,
/// `database::models::Transcript` also exposes this column, so the two are
/// equivalent here — the raw query is a historical choice, not a necessity.
pub async fn load_for_export(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<(ExportMeeting, Vec<ExportSegment>, Option<String>), String> {
    let meeting_row = sqlx::query("SELECT id, title, created_at FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Reading the meeting failed: {}", e))?
        .ok_or_else(|| format!("No meeting with id {}", meeting_id))?;

    let created_at: String = meeting_row
        .try_get::<String, _>("created_at")
        .map_err(|e| format!("Nieczytelne created_at: {}", e))?;

    let title: String = meeting_row
        .try_get::<String, _>("title")
        .unwrap_or_else(|_| "Untitled meeting".to_string());

    let segment_rows = sqlx::query(
        "SELECT transcript, speaker, audio_start_time, audio_end_time \
         FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, id",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Reading the transcript failed: {}", e))?;

    let mut segments = Vec::with_capacity(segment_rows.len());
    let mut last_end: Option<f64> = None;

    for row in segment_rows {
        let end: Option<f64> = row.try_get("audio_end_time").unwrap_or(None);
        if let Some(e) = end {
            last_end = Some(last_end.map_or(e, |prev: f64| prev.max(e)));
        }

        segments.push(ExportSegment {
            speaker: row.try_get::<Option<String>, _>("speaker").unwrap_or(None),
            audio_start_time: row.try_get("audio_start_time").unwrap_or(None),
            text: row.try_get::<String, _>("transcript").unwrap_or_default(),
        });
    }

    let summary_markdown = sqlx::query(
        "SELECT result FROM summary_processes WHERE meeting_id = ? AND status = 'completed'",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Reading the summary failed: {}", e))?
    .and_then(|row| row.try_get::<Option<String>, _>("result").ok().flatten())
    .and_then(|json| extract_summary_markdown(&json));

    let meeting = ExportMeeting {
        id: meeting_id.to_string(),
        title,
        created_at,
        duration_min: last_end.map(|s| s / 60.0),
    };

    Ok((meeting, segments, summary_markdown))
}

/// Turns stored settings into concrete destinations.
///
/// Returns `None` when nothing usable is configured: no vault and not both
/// overrides. Callers treat that as "export not set up" rather than an error.
pub fn targets_from_config(
    config: &crate::database::repositories::setting::ObsidianConfig,
) -> Option<ExportTargets> {
    match (
        config.vault_path.as_deref(),
        config.transcript_path.as_deref(),
        config.summary_path.as_deref(),
    ) {
        (Some(vault), transcript, summary) => {
            Some(ExportTargets::resolve(vault, transcript, summary))
        }
        // No vault, but both destinations named explicitly is still complete.
        (None, Some(transcript), Some(summary)) => Some(ExportTargets {
            transcript_dir: PathBuf::from(transcript),
            summary_dir: PathBuf::from(summary),
        }),
        _ => None,
    }
}

/// The language the exported notes should be written in.
///
/// Comes from the summary-language preference stored beside the recording, so a
/// note matches the summary it sits next to. Every failure returns `None`, which
/// renders in English — an export that succeeds in the wrong language beats one
/// that refuses to run.
async fn read_export_language(pool: &SqlitePool, meeting_id: &str) -> Option<String> {
    let meeting = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .ok()
        .flatten()?;
    let folder_path = meeting.folder_path.filter(|p| !p.trim().is_empty())?;

    crate::summary::metadata::read_summary_language_from_metadata(std::path::Path::new(&folder_path))
        .ok()
        .flatten()
}

/// Renders and writes one meeting. Shared by the manual command and the
/// automatic post-summary hook.
pub async fn export_meeting(
    pool: &SqlitePool,
    targets: &ExportTargets,
    meeting_id: &str,
) -> Result<Vec<PathBuf>, String> {
    let (meeting, segments, summary) = load_for_export(pool, meeting_id).await?;
    let language = read_export_language(pool, meeting_id).await;
    let bundle = render_meeting(&meeting, &segments, summary.as_deref(), language.as_deref());
    write_bundle(targets, &bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn bundle_with_summary(stem: &str) -> ExportBundle {
        ExportBundle {
            stem: stem.to_string(),
            transcript_md: "transkrypt".to_string(),
            summary_md: Some("podsumowanie".to_string()),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("halvern-export-test-{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Both notes into one directory, the pre-split behaviour.
    fn single(dir: &Path) -> ExportTargets {
        ExportTargets {
            transcript_dir: dir.to_path_buf(),
            summary_dir: dir.to_path_buf(),
        }
    }

    #[test]
    fn writes_both_notes_and_creates_the_directory() {
        let dir = temp_dir("basic");
        let written = write_bundle(&single(&dir), &bundle_with_summary("2026-08-11-test")).unwrap();

        assert_eq!(written.len(), 2);
        assert!(dir.join("2026-08-11-test-transcript.md").exists());
        assert!(dir.join("2026-08-11-test-summary.md").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collision_produces_suffix_2_then_3_without_overwriting() {
        let dir = temp_dir("collision");
        let targets = single(&dir);
        let bundle = bundle_with_summary("2026-08-11-test");

        write_bundle(&targets, &bundle).unwrap();
        let original = dir.join("2026-08-11-test-transcript.md");
        std::fs::write(&original, "EDITED BY HAND").unwrap();

        let second = write_bundle(&targets, &bundle).unwrap();
        assert!(second[0].ends_with("2026-08-11-test-transcript-2.md"));
        assert!(second[1].ends_with("2026-08-11-test-summary-2.md"));

        let third = write_bundle(&targets, &bundle).unwrap();
        assert!(third[0].ends_with("2026-08-11-test-transcript-3.md"));

        assert_eq!(
            std::fs::read_to_string(&original).unwrap(),
            "EDITED BY HAND",
            "the original file must not be overwritten"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transcript_and_summary_share_one_suffix_index() {
        let dir = temp_dir("shared-index");
        std::fs::create_dir_all(&dir).unwrap();
        // Only the summary name is taken; the pair must still move to -2 together.
        std::fs::write(dir.join("2026-08-11-test-summary.md"), "taken").unwrap();

        let written = write_bundle(&single(&dir), &bundle_with_summary("2026-08-11-test")).unwrap();
        assert!(written[0].ends_with("2026-08-11-test-transcript-2.md"));
        assert!(written[1].ends_with("2026-08-11-test-summary-2.md"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn meeting_without_summary_writes_one_note() {
        let dir = temp_dir("no-summary");
        let bundle = ExportBundle {
            stem: "2026-08-11-test".to_string(),
            transcript_md: "transkrypt".to_string(),
            summary_md: None,
        };

        let written = write_bundle(&single(&dir), &bundle).unwrap();
        assert_eq!(written.len(), 1);
        assert!(!dir.join("2026-08-11-test-summary.md").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_destination_is_rejected() {
        let err = write_bundle(&single(Path::new("")), &bundle_with_summary("x")).unwrap_err();
        assert!(err.contains("empty"), "got: {}", err);
    }

    #[test]
    fn split_directories_receive_their_own_note() {
        let base = temp_dir("split");
        let targets = ExportTargets {
            transcript_dir: base.join("transkrypty"),
            summary_dir: base.join("notatki"),
        };

        let written = write_bundle(&targets, &bundle_with_summary("2026-08-11-test")).unwrap();

        assert_eq!(written.len(), 2);
        assert!(base.join("transkrypty/2026-08-11-test-transcript.md").exists());
        assert!(base.join("notatki/2026-08-11-test-summary.md").exists());
        assert!(
            !base.join("transkrypty/2026-08-11-test-summary.md").exists(),
            "the summary must not land in the transcript directory"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn shared_index_holds_across_split_directories() {
        let base = temp_dir("split-index");
        let targets = ExportTargets {
            transcript_dir: base.join("transkrypty"),
            summary_dir: base.join("notatki"),
        };
        std::fs::create_dir_all(&targets.summary_dir).unwrap();
        // Collision exists only in the summary directory. The transcript, whose
        // own directory is empty, must still move to -2 so the pair stays matched.
        std::fs::write(targets.summary_dir.join("2026-08-11-test-summary.md"), "taken").unwrap();

        let written = write_bundle(&targets, &bundle_with_summary("2026-08-11-test")).unwrap();
        assert!(written[0].ends_with("transkrypty/2026-08-11-test-transcript-2.md"));
        assert!(written[1].ends_with("notatki/2026-08-11-test-summary-2.md"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_falls_back_to_vault_for_blank_overrides() {
        let both_default = ExportTargets::resolve("/vault", None, Some("   "));
        assert_eq!(both_default.transcript_dir, PathBuf::from("/vault"));
        assert_eq!(both_default.summary_dir, PathBuf::from("/vault"));
        assert!(both_default.is_single_directory());

        let split = ExportTargets::resolve("/vault", Some("/local/raw"), Some("/vault/notes"));
        assert_eq!(split.transcript_dir, PathBuf::from("/local/raw"));
        assert_eq!(split.summary_dir, PathBuf::from("/vault/notes"));
        assert!(!split.is_single_directory());
    }

    #[test]
    fn summary_markdown_is_extracted_and_cache_ignored() {
        // r##"…"## because the payload itself contains the `"#` sequence,
        // which would terminate a single-hash raw string early.
        let json = r##"{"markdown":"# Note\n\nbody","english_cache":{"markdown":"other"}}"##;
        assert_eq!(
            extract_summary_markdown(json).unwrap(),
            "# Note\n\nbody"
        );

        assert!(extract_summary_markdown("{}").is_none());
        assert!(extract_summary_markdown(r#"{"markdown":"  "}"#).is_none());
        assert!(extract_summary_markdown("not json").is_none());
    }
}
