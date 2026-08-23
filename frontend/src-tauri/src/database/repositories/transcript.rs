use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // Recording length, taken from the last segment that carries a
        // timestamp. Computed here rather than passed in because this is the
        // one place that already has every segment; the meetings list needs it
        // to show duration without opening each meeting.
        let duration_seconds: Option<f64> = transcripts
            .iter()
            .filter_map(|s| s.audio_end_time)
            .fold(None, |acc: Option<f64>, end| {
                Some(acc.map_or(end, |m: f64| m.max(end)))
            });

        // How the recording started, read back from the metadata file the
        // recorder already wrote into the meeting folder.
        //
        // Taking it from disk rather than as a parameter is deliberate: the
        // database write is triggered by the frontend, which never learns
        // whether the meeting detector or the user started this. The recorder
        // does know, and has written it down before the first sample landed.
        let (source, app_name) = folder_path
            .as_deref()
            .map(std::path::Path::new)
            .and_then(read_recording_origin)
            .unwrap_or((None, None));

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path, duration_seconds, source, app_name)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .bind(duration_seconds)
        .bind(source)
        .bind(app_name)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, duration)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(&segment.speaker)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    ///
    /// The case-insensitive comparison happens in Rust rather than through
    /// SQL's `LOWER()`, which SQLite folds for ASCII only — a transcript
    /// containing an uppercase non-ASCII letter (e.g. "BUDŻET") was
    /// unreachable by any query before this. The `LIKE` clause could not
    /// use an index either way (a leading `%` defeats one), so this costs
    /// no more than the scan SQLite was already doing.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id",
        )
        .fetch_all(pool)
        .await?;

        let query_lower = query.to_lowercase();
        let results = rows
            .into_iter()
            .filter(|(_, _, transcript, _)| transcript.to_lowercase().contains(&query_lower))
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Snaps `index` down to the nearest UTF-8 char boundary in `s`, so a
    /// slice starting there never panics.
    fn floor_char_boundary(s: &str, index: usize) -> usize {
        let mut i = index.min(s.len());
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// Snaps `index` up to the nearest UTF-8 char boundary in `s`, so a
    /// slice ending there never panics.
    fn ceil_char_boundary(s: &str, index: usize) -> usize {
        let mut i = index.min(s.len());
        while i < s.len() && !s.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                // match_index is a byte offset found in transcript_lower; for
                // every letter this application's transcripts actually use
                // (Polish and English), lowercasing preserves byte length, so
                // the offset lines up with transcript too. What it does NOT
                // guarantee is landing on a char boundary once ±100 bytes are
                // added — a multi-byte character straddling that boundary
                // used to panic the slice below. Snapping both ends fixes
                // exactly that, without changing the common-case output.
                let start_index =
                    Self::floor_char_boundary(transcript, match_index.saturating_sub(100));
                let end_index = Self::ceil_char_boundary(
                    transcript,
                    (match_index + query_lower.len() + 100).min(transcript.len()),
                );

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

/// Read `source` and `app_name` out of a meeting folder's `metadata.json`.
///
/// Returns `(None, None)` for anything unreadable — a folder from before these
/// fields existed, a partially written file, a recording whose folder was moved.
/// A missing origin is a fact about an old recording, not a reason to fail a
/// save that is otherwise complete.
fn read_recording_origin(folder: &std::path::Path) -> Option<(Option<String>, Option<String>)> {
    let raw = std::fs::read_to_string(folder.join("metadata.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let field = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    Some((field("source"), field("app_name")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    /// In-memory database with the real migrations applied, mirroring the
    /// harness the meetings repository tests established.
    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations apply");
        pool
    }

    fn segment(text: &str, end: Option<f64>) -> TranscriptSegment {
        TranscriptSegment {
            id: String::new(),
            text: text.to_string(),
            timestamp: "2026-08-15T10:00:00Z".to_string(),
            audio_start_time: end.map(|e| e - 1.0),
            audio_end_time: end,
            duration: end.map(|_| 1.0),
            speaker: Some("mic".to_string()),
        }
    }

    #[tokio::test]
    async fn save_transcript_persists_meeting_and_segments_atomically() {
        let pool = pool().await;
        let segments = vec![
            segment("pierwszy fragment", Some(5.0)),
            segment("drugi fragment", Some(12.5)),
        ];

        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Rozmowa z klientem",
            &segments,
            Some("/tmp/somewhere".to_string()),
        )
        .await
        .expect("save succeeds");

        assert!(meeting_id.starts_with("meeting-"), "ids carry the meeting- prefix");

        let row = sqlx::query("SELECT title, folder_path, duration_seconds FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_one(&pool)
            .await
            .expect("meeting row exists");
        assert_eq!(row.get::<String, _>("title"), "Rozmowa z klientem");
        assert_eq!(row.get::<Option<String>, _>("folder_path").as_deref(), Some("/tmp/somewhere"));
        assert_eq!(
            row.get::<Option<f64>, _>("duration_seconds"),
            Some(12.5),
            "duration is the latest segment end, not the sum"
        );

        let count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM transcripts WHERE meeting_id = ?")
            .bind(&meeting_id)
            .fetch_one(&pool)
            .await
            .expect("count runs")
            .get("c");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn a_meeting_without_segments_has_no_duration() {
        let pool = pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(&pool, "Pusty", &[], None)
            .await
            .expect("an empty meeting still saves");

        let row = sqlx::query("SELECT duration_seconds FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_one(&pool)
            .await
            .expect("meeting row exists");
        assert_eq!(row.get::<Option<f64>, _>("duration_seconds"), None);
    }

    #[tokio::test]
    async fn segments_without_timing_leave_duration_empty() {
        let pool = pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Import bez czasów",
            &[segment("stary import", None)],
            None,
        )
        .await
        .expect("save succeeds");

        let row = sqlx::query("SELECT duration_seconds FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_one(&pool)
            .await
            .expect("meeting row exists");
        assert_eq!(row.get::<Option<f64>, _>("duration_seconds"), None);
    }

    #[tokio::test]
    async fn search_is_case_insensitive_for_ascii_and_scoped_to_matches() {
        let pool = pool().await;
        TranscriptsRepository::save_transcript(
            &pool,
            "Budgetary",
            &[segment("we discussed the BUDGET in detail", Some(3.0))],
            None,
        )
        .await
        .unwrap();
        TranscriptsRepository::save_transcript(
            &pool,
            "Inne",
            &[segment("zupełnie inny temat", Some(3.0))],
            None,
        )
        .await
        .unwrap();

        let hits = TranscriptsRepository::search_transcripts(&pool, "budget")
            .await
            .expect("search runs");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Budgetary");
        assert!(hits[0].match_context.contains("BUDGET"));

        let none = TranscriptsRepository::search_transcripts(&pool, "nieistniejące")
            .await
            .expect("search runs");
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn case_insensitivity_covers_uppercase_non_ascii_letters() {
        // Regression test: SQLite's LOWER() folds ASCII only, so matching in
        // SQL made a transcript containing an uppercase `Ż` unreachable by any
        // query. The comparison now happens in Rust with full Unicode
        // case folding, so every casing of every query below finds it.
        let pool = pool().await;
        TranscriptsRepository::save_transcript(
            &pool,
            "Budżetowe",
            &[segment("Omówiliśmy BUDŻET projektu", Some(3.0))],
            None,
        )
        .await
        .unwrap();

        for query in ["budżet", "BUDŻET", "Budżet"] {
            let hits = TranscriptsRepository::search_transcripts(&pool, query)
                .await
                .expect("search runs");
            assert_eq!(hits.len(), 1, "query casing {query:?} should still find the meeting");
            assert_eq!(hits[0].title, "Budżetowe");
        }
    }

    #[tokio::test]
    async fn blank_queries_return_nothing_without_touching_sql() {
        let pool = pool().await;
        assert!(TranscriptsRepository::search_transcripts(&pool, "").await.unwrap().is_empty());
        assert!(TranscriptsRepository::search_transcripts(&pool, "   ").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn long_transcripts_are_trimmed_to_a_context_window() {
        let pool = pool().await;
        let long_text = format!("{} centrum {}", "a".repeat(300), "b".repeat(300));
        TranscriptsRepository::save_transcript(&pool, "Długie", &[segment(&long_text, Some(3.0))], None)
            .await
            .unwrap();

        let hits = TranscriptsRepository::search_transcripts(&pool, "centrum")
            .await
            .expect("search runs");
        assert_eq!(hits.len(), 1);
        let context = &hits[0].match_context;
        assert!(context.starts_with("..."), "a match deep in the text trims the front");
        assert!(context.ends_with("..."), "and the back");
        assert!(context.contains("centrum"));
        assert!(context.len() < long_text.len());
    }

    #[tokio::test]
    async fn a_multi_byte_character_at_the_context_window_edge_does_not_panic() {
        // Regression test: the ±100-byte context window used to slice the
        // original transcript at raw byte offsets, which panicked whenever a
        // multi-byte character (any Polish diacritic) straddled the cut. The
        // exact byte where that happens depends on the surrounding text, so
        // this sweeps the diacritic's position across the whole window
        // instead of hand-computing one offset.
        for offset in 90..=110 {
            let pool = pool().await;
            let padding_before: String = std::iter::repeat('x').take(offset).collect();
            let padding_after: String = "y".repeat(250);
            let text = format!("{padding_before}ą marker {padding_after}");

            TranscriptsRepository::save_transcript(
                &pool,
                &format!("Meeting {offset}"),
                &[segment(&text, Some(3.0))],
                None,
            )
            .await
            .unwrap();

            let hits = TranscriptsRepository::search_transcripts(&pool, "marker")
                .await
                .expect("search must not panic regardless of where the diacritic falls");
            assert_eq!(hits.len(), 1);
            assert!(hits[0].match_context.contains("marker"));
        }
    }
}
