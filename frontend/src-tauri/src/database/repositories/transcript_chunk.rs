// src/database/repo/transcript_chunks.rs

use chrono::Utc;
use log::info as log_info;
use sqlx::SqlitePool;
pub struct TranscriptChunksRepository;

impl TranscriptChunksRepository {
    /// Saves the full transcript text and processing parameters.
    pub async fn save_transcript_data(
        pool: &SqlitePool,
        meeting_id: &str,
        text: &str,
        model: &str,
        model_name: &str,
        chunk_size: i32,
        overlap: i32,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Saving transcript data to transcript_chunks for meeting_id: {}",
            meeting_id
        );
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO transcript_chunks (meeting_id, transcript_text, model, model_name, chunk_size, overlap, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(meeting_id) DO UPDATE SET
                transcript_text = excluded.transcript_text,
                model = excluded.model,
                model_name = excluded.model_name,
                chunk_size = excluded.chunk_size,
                overlap = excluded.overlap,
                created_at = excluded.created_at
            "#
        )
        .bind(meeting_id)
        .bind(text)
        .bind(model)
        .bind(model_name)
        .bind(chunk_size)
        .bind(overlap)
        .bind(now)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    /// In-memory database with the real migrations applied, mirroring the
    /// harness the transcripts repository tests established.
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

    /// `transcript_chunks.meeting_id` is a foreign key, so every test that
    /// expects a successful write needs its meeting to exist first.
    async fn seed_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?,?,?,?)")
            .bind(id)
            .bind("Rozmowa z klientem")
            .bind("2026-08-14T10:00:00Z")
            .bind("2026-08-14T10:00:00Z")
            .execute(pool)
            .await
            .expect("insert meeting");
    }

    #[tokio::test]
    async fn save_transcript_data_stores_the_text_and_its_processing_parameters() {
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        TranscriptChunksRepository::save_transcript_data(
            &pool,
            "m1",
            "omówiliśmy budżet projektu",
            "ollama",
            "llama3.1:8b",
            5000,
            1000,
        )
        .await
        .expect("save succeeds");

        let row = sqlx::query("SELECT * FROM transcript_chunks WHERE meeting_id = ?")
            .bind("m1")
            .fetch_one(&pool)
            .await
            .expect("chunk row exists");
        assert_eq!(
            row.get::<String, _>("transcript_text"),
            "omówiliśmy budżet projektu"
        );
        assert_eq!(row.get::<String, _>("model"), "ollama");
        assert_eq!(row.get::<String, _>("model_name"), "llama3.1:8b");
        assert_eq!(row.get::<i64, _>("chunk_size"), 5000);
        assert_eq!(row.get::<i64, _>("overlap"), 1000);
        assert!(
            !row.get::<String, _>("created_at").is_empty(),
            "the write stamps its own time"
        );
        // The table carries a meeting_name column that this function never
        // writes; the meeting title lives in `meetings` and is read from there.
        assert_eq!(row.get::<Option<String>, _>("meeting_name"), None);
    }

    #[tokio::test]
    async fn saving_again_replaces_the_previous_chunk_row() {
        // meeting_id is the primary key, so a meeting holds exactly one chunk
        // row: re-summarising with a different model overwrites, it does not
        // accumulate history.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        TranscriptChunksRepository::save_transcript_data(
            &pool, "m1", "pierwsza wersja", "ollama", "llama3.1:8b", 5000, 1000,
        )
        .await
        .expect("first save succeeds");
        TranscriptChunksRepository::save_transcript_data(
            &pool, "m1", "druga wersja", "claude", "claude-sonnet-4", 8000, 200,
        )
        .await
        .expect("second save succeeds");

        let rows: i64 = sqlx::query("SELECT COUNT(*) AS c FROM transcript_chunks")
            .fetch_one(&pool)
            .await
            .expect("count runs")
            .get("c");
        assert_eq!(rows, 1, "the second save replaced the first");

        let row = sqlx::query("SELECT * FROM transcript_chunks WHERE meeting_id = ?")
            .bind("m1")
            .fetch_one(&pool)
            .await
            .expect("chunk row exists");
        assert_eq!(row.get::<String, _>("transcript_text"), "druga wersja");
        assert_eq!(row.get::<String, _>("model"), "claude");
        assert_eq!(row.get::<String, _>("model_name"), "claude-sonnet-4");
        assert_eq!(row.get::<i64, _>("chunk_size"), 8000);
        assert_eq!(row.get::<i64, _>("overlap"), 200);
    }

    #[tokio::test]
    async fn nothing_here_validates_its_input() {
        // Frozen deliberately: an empty transcript, an empty model name and an
        // overlap larger than the chunk are all written without complaint.
        // Validation belongs to the summary pipeline that calls this; the
        // repository is a plain persistence layer, and a test that pinned it
        // otherwise would be describing code that does not exist.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        TranscriptChunksRepository::save_transcript_data(&pool, "m1", "", "", "", -1, 999_999)
            .await
            .expect("an empty, self-contradictory chunk is still accepted");

        let row = sqlx::query("SELECT * FROM transcript_chunks WHERE meeting_id = ?")
            .bind("m1")
            .fetch_one(&pool)
            .await
            .expect("chunk row exists");
        assert_eq!(row.get::<String, _>("transcript_text"), "");
        assert_eq!(row.get::<i64, _>("chunk_size"), -1);
        assert_eq!(row.get::<i64, _>("overlap"), 999_999);
    }

    #[tokio::test]
    async fn a_chunk_cannot_outlive_its_meeting() {
        // The foreign key is the only guard on the meeting_id argument, so a
        // typo or a deleted meeting surfaces as a database error rather than an
        // orphaned row that no query would ever find again.
        let pool = pool().await;

        let err = TranscriptChunksRepository::save_transcript_data(
            &pool,
            "nie-istnieje",
            "tekst",
            "ollama",
            "llama3.1:8b",
            5000,
            1000,
        )
        .await
        .expect_err("an unknown meeting is rejected");
        assert!(
            err.to_string().to_lowercase().contains("foreign key"),
            "expected a foreign key violation, got: {err}"
        );
    }
}
