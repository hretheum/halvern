use crate::database::models::SummaryProcess;
use chrono::Utc;
use serde_json::Value;
use sqlx::SqlitePool;
use tracing::{error, info as log_info};

pub struct SummaryProcessesRepository;

impl SummaryProcessesRepository {
    /// Retrieves the current summary process state for a given meeting ID.
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>("SELECT * FROM summary_processes WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = pool.begin().await?;

        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();

        if !meeting_exists {
            log_info!(
                "Attempted to save summary for a non-existent meeting_id: {}",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        let result_json = serde_json::to_string(summary);
        if result_json.is_err() {
            error!("Can't convert the json to string for saving to Database");
            transaction.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now();

        let updated = sqlx::query(
            "UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ?",
        )
        .bind(result_json.unwrap())
        .bind(now)
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

        // The meeting existing is not enough: this UPDATE has nothing to
        // write into without a summary_processes row already there. Without
        // this check the transaction would still commit — touching only
        // meetings.updated_at — and the caller would be told the edited
        // summary was saved when it was silently dropped.
        if updated.rows_affected() == 0 {
            log_info!(
                "Attempted to save summary for meeting_id {} with no summary process row",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        log_info!(
            "Successfully updated summary and timestamp for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    /// Reads the summary process state for a meeting, the same as
    /// [`Self::get_summary_data`].
    ///
    /// This used to inner-join `transcript_chunks`, which had no columns in
    /// the result and served no purpose beyond accidentally hiding the
    /// summary: the transcript-chunk write and the summary-completion write
    /// are not transactional, so a completed summary was invisible through
    /// this function until the chunk row also landed, even though
    /// `get_summary_data` returned it the whole time.
    pub async fn get_summary_data_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        Self::get_summary_data(pool, meeting_id).await
    }

    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Creating or resetting summary process for meeting_id: {}",
            meeting_id
        );
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time, result, error)
            VALUES (?, 'PENDING', ?, ?, ?, NULL, NULL)
            ON CONFLICT(meeting_id) DO UPDATE SET
                status = 'PENDING',
                updated_at = excluded.updated_at,
                start_time = excluded.start_time,
                result_backup = result,
                result_backup_timestamp = excluded.updated_at,
                result = result,
                error = NULL
            "#
        )
        .bind(meeting_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {}",
            meeting_id
        );
        Ok(())
    }

    /// Returns `Ok(true)` when a process row actually existed to update, so
    /// callers can tell a real completion from one that landed on a meeting
    /// id no summary process was ever opened for.
    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        result: Value, // Keep this as Value to handle both old and new formats if needed
        chunk_count: i64,
        processing_time: f64,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        let updated = sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ?
            "#
        )
        .bind(result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .execute(pool)
        .await?;

        if updated.rows_affected() == 0 {
            log_info!(
                "update_process_completed found no summary process row for meeting_id: {}",
                meeting_id
            );
            return Ok(false);
        }
        log_info!(
            "Summary completed and backup cleared for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    /// See [`Self::update_process_completed`] for the meaning of the `bool`.
    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        error: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        // Restore from backup if it exists, otherwise keep current result
        let updated = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ?
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .execute(pool)
        .await?;

        if updated.rows_affected() == 0 {
            log_info!(
                "update_process_failed found no summary process row for meeting_id: {}",
                meeting_id
            );
            return Ok(false);
        }
        log_info!(
            "Summary generation failed and backup restored for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    /// See [`Self::update_process_completed`] for the meaning of the `bool`.
    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        // Restore from backup if it exists, otherwise keep current result
        let updated = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'cancelled',
                updated_at = ?,
                end_time = ?,
                error = 'Generation was cancelled by user',
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .execute(pool)
        .await?;

        if updated.rows_affected() == 0 {
            log_info!(
                "update_process_cancelled found no summary process row for meeting_id: {}",
                meeting_id
            );
            return Ok(false);
        }
        log_info!(
            "Marked summary process as cancelled and restored backup for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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

    /// Every row in `summary_processes` is anchored to a meeting by a foreign
    /// key, so the meeting has to exist before any of these functions can work.
    async fn seed_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?,?,?,?)")
            .bind(id)
            .bind("Rozmowa z klientem")
            .bind(SEEDED_TIMESTAMP)
            .bind(SEEDED_TIMESTAMP)
            .execute(pool)
            .await
            .expect("insert meeting");
    }

    const SEEDED_TIMESTAMP: &str = "2026-08-14T10:00:00Z";

    /// Reads a single column straight from the table, bypassing `SummaryProcess`
    /// so that the backup columns — which the model exposes but the happy path
    /// rarely touches — can be asserted on directly.
    async fn column(pool: &SqlitePool, meeting_id: &str, name: &str) -> Option<String> {
        let sql = format!("SELECT {} AS v FROM summary_processes WHERE meeting_id = ?", name);
        sqlx::query(&sql)
            .bind(meeting_id)
            .fetch_one(pool)
            .await
            .expect("process row exists")
            .get("v")
    }

    #[tokio::test]
    async fn a_meeting_without_a_summary_process_reads_back_as_none() {
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        assert!(SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .expect("query runs")
            .is_none());
        // An id that never existed behaves the same way: absent, not an error.
        assert!(SummaryProcessesRepository::get_summary_data(&pool, "nie-istnieje")
            .await
            .expect("query runs")
            .is_none());
    }

    #[tokio::test]
    async fn create_or_reset_process_opens_a_pending_row_with_no_result_yet() {
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .expect("process created");

        let process = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .expect("query runs")
            .expect("process row exists");
        assert_eq!(process.status, "PENDING");
        assert_eq!(process.result, None);
        assert_eq!(process.error, None);
        assert_eq!(process.result_backup, None);
        // The schema defaults, not the insert, supply these two.
        assert_eq!(process.chunk_count, 0);
        assert_eq!(process.processing_time, 0.0);
        assert!(process.start_time.is_some(), "the clock starts on creation");
        assert!(process.end_time.is_none());
    }

    #[tokio::test]
    async fn resetting_a_finished_process_keeps_the_old_summary_and_backs_it_up() {
        // This is what protects the user's previous summary while a
        // regeneration runs: the visible result stays put and a copy is parked
        // in result_backup for update_process_failed / _cancelled to restore.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();
        SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            json!({"summary": "pierwsza wersja"}),
            3,
            12.5,
        )
        .await
        .unwrap();

        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .expect("regeneration starts");

        let process = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .unwrap()
            .expect("process row exists");
        assert_eq!(process.status, "PENDING");
        assert_eq!(
            process.result.as_deref(),
            Some(r#"{"summary":"pierwsza wersja"}"#),
            "the old summary stays visible while the new one is generated"
        );
        assert_eq!(process.result_backup, process.result);
        assert!(process.result_backup_timestamp.is_some());
        assert_eq!(process.error, None);
        // Counters from the previous run are deliberately left alone; only
        // update_process_completed rewrites them.
        assert_eq!(process.chunk_count, 3);
    }

    #[tokio::test]
    async fn update_process_completed_stores_the_result_and_drops_the_backup() {
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();

        SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            json!({"key_points": ["budżet", "termin"]}),
            7,
            42.25,
        )
        .await
        .expect("completion recorded");

        let process = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .unwrap()
            .expect("process row exists");
        assert_eq!(process.status, "completed");
        assert_eq!(
            process.result.as_deref(),
            Some(r#"{"key_points":["budżet","termin"]}"#)
        );
        assert_eq!(process.chunk_count, 7);
        assert_eq!(process.processing_time, 42.25);
        assert_eq!(process.error, None);
        assert!(process.end_time.is_some());
        // A successful run makes the backup pointless, and keeping it would
        // let a later failure resurrect a summary two generations old.
        assert_eq!(process.result_backup, None);
        assert_eq!(process.result_backup_timestamp, None);
    }

    #[tokio::test]
    async fn a_failed_regeneration_puts_the_previous_summary_back() {
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();
        SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            json!({"summary": "dobra wersja"}),
            1,
            1.0,
        )
        .await
        .unwrap();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();

        SummaryProcessesRepository::update_process_failed(&pool, "m1", "model timed out")
            .await
            .expect("failure recorded");

        let process = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .unwrap()
            .expect("process row exists");
        assert_eq!(process.status, "failed");
        assert_eq!(process.error.as_deref(), Some("model timed out"));
        assert_eq!(
            process.result.as_deref(),
            Some(r#"{"summary":"dobra wersja"}"#),
            "the user keeps the summary they already had"
        );
        assert_eq!(process.result_backup, None, "the backup was consumed");
    }

    #[tokio::test]
    async fn a_failure_without_a_backup_leaves_the_current_result_untouched() {
        // COALESCE(result_backup, result) has to cope with the first-ever run,
        // where there is nothing to restore and result is still NULL.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();

        SummaryProcessesRepository::update_process_failed(&pool, "m1", "no model configured")
            .await
            .expect("failure recorded");

        let process = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .unwrap()
            .expect("process row exists");
        assert_eq!(process.status, "failed");
        assert_eq!(process.result, None);
        assert_eq!(process.error.as_deref(), Some("no model configured"));
    }

    #[tokio::test]
    async fn a_cancelled_regeneration_restores_the_backup_under_a_fixed_message() {
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();
        SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            json!({"summary": "dobra wersja"}),
            1,
            1.0,
        )
        .await
        .unwrap();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();

        SummaryProcessesRepository::update_process_cancelled(&pool, "m1")
            .await
            .expect("cancellation recorded");

        let process = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .unwrap()
            .expect("process row exists");
        assert_eq!(process.status, "cancelled");
        // The message is written by the query itself, not passed in, so the UI
        // can rely on it being exactly this string.
        assert_eq!(
            process.error.as_deref(),
            Some("Generation was cancelled by user")
        );
        assert_eq!(
            process.result.as_deref(),
            Some(r#"{"summary":"dobra wersja"}"#)
        );
        assert_eq!(process.result_backup, None);
    }

    #[tokio::test]
    async fn finishing_a_process_that_was_never_started_reports_failure() {
        // Regression test: all three terminal updates are bare UPDATEs that
        // match zero rows when no process exists. They now inspect the
        // affected-row count and report Ok(false) instead of claiming a
        // completion, failure or cancellation was recorded when nothing was.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        let completed = SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            json!({"summary": "znika"}),
            1,
            1.0,
        )
        .await
        .expect("query runs");
        assert!(!completed, "no process row existed to complete");

        let failed = SummaryProcessesRepository::update_process_failed(&pool, "m1", "znika")
            .await
            .expect("query runs");
        assert!(!failed, "no process row existed to fail");

        let cancelled = SummaryProcessesRepository::update_process_cancelled(&pool, "m1")
            .await
            .expect("query runs");
        assert!(!cancelled, "no process row existed to cancel");

        // A meeting id that does not exist at all behaves the same way.
        let cancelled_unknown =
            SummaryProcessesRepository::update_process_cancelled(&pool, "nie-istnieje")
                .await
                .expect("query runs");
        assert!(!cancelled_unknown);

        assert!(
            SummaryProcessesRepository::get_summary_data(&pool, "m1")
                .await
                .unwrap()
                .is_none(),
            "nothing was written by any of the three updates"
        );
    }

    #[tokio::test]
    async fn a_process_cannot_be_opened_for_an_unknown_meeting() {
        // The foreign key is what stops a summary process from being created
        // for a meeting that was deleted while the request was in flight.
        let pool = pool().await;

        let err = SummaryProcessesRepository::create_or_reset_process(&pool, "nie-istnieje")
            .await
            .expect_err("an unknown meeting is rejected");
        assert!(
            err.to_string().to_lowercase().contains("foreign key"),
            "expected a foreign key violation, got: {err}"
        );
    }

    #[tokio::test]
    async fn update_meeting_summary_writes_the_edited_summary_and_touches_the_meeting() {
        // This is the manual-edit path: the user rewrites a summary in the
        // editor, and the meeting's updated_at has to move so the list re-sorts.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();

        let saved = SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "m1",
            &json!({"summary": "poprawione ręcznie"}),
        )
        .await
        .expect("update runs");
        assert!(saved);

        assert_eq!(
            column(&pool, "m1", "result").await.as_deref(),
            Some(r#"{"summary":"poprawione ręcznie"}"#)
        );

        let updated_at: String = sqlx::query("SELECT updated_at FROM meetings WHERE id = ?")
            .bind("m1")
            .fetch_one(&pool)
            .await
            .expect("meeting row exists")
            .get("updated_at");
        assert_ne!(updated_at, SEEDED_TIMESTAMP, "the meeting was touched");
    }

    #[tokio::test]
    async fn update_meeting_summary_refuses_an_unknown_meeting() {
        // The explicit existence check exists because the UPDATE alone would
        // match nothing and report success; here the caller is told the truth.
        let pool = pool().await;

        let saved = SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "nie-istnieje",
            &json!({"summary": "gdziekolwiek"}),
        )
        .await
        .expect("update runs");
        assert!(!saved);
    }

    #[tokio::test]
    async fn update_meeting_summary_refuses_a_meeting_with_no_process_row() {
        // Regression test: the guard used to check only that the *meeting*
        // exists, not that a summary process does. For a meeting that was
        // never summarised the UPDATE touched zero rows, the transaction
        // still committed, and the caller was told `true` while the edited
        // summary was dropped. The row count is now checked the same way
        // the three terminal updates check it.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;

        let saved = SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "m1",
            &json!({"summary": "znika bez śladu"}),
        )
        .await
        .expect("query runs");
        assert!(!saved, "there is nowhere to write the summary yet");

        assert!(
            SummaryProcessesRepository::get_summary_data(&pool, "m1")
                .await
                .unwrap()
                .is_none(),
            "no row was created to hold the summary"
        );
    }

    #[tokio::test]
    async fn get_summary_data_for_meeting_matches_get_summary_data() {
        // Regression test: this variant used to inner-join summary_processes
        // against transcript_chunks — a table its SELECT never read a column
        // from — so a completed summary was invisible through this function
        // until the chunked transcript that produced it was also saved,
        // even though get_summary_data returned it the whole time. The two
        // writes are not transactional, so the gap was reachable in
        // practice. It now delegates to get_summary_data directly.
        let pool = pool().await;
        seed_meeting(&pool, "m1").await;
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1")
            .await
            .unwrap();
        SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            json!({"summary": "gotowe"}),
            1,
            1.0,
        )
        .await
        .unwrap();

        // No transcript_chunks row exists at all, and the summary is visible
        // through both functions regardless.
        let process = SummaryProcessesRepository::get_summary_data_for_meeting(&pool, "m1")
            .await
            .expect("query runs")
            .expect("the summary is visible without a transcript chunk");
        assert_eq!(process.status, "completed");
        assert_eq!(process.result.as_deref(), Some(r#"{"summary":"gotowe"}"#));

        let via_plain = SummaryProcessesRepository::get_summary_data(&pool, "m1")
            .await
            .unwrap()
            .expect("the plain lookup agrees");
        assert_eq!(via_plain.status, process.status);
        assert_eq!(via_plain.result, process.result);
    }
}
