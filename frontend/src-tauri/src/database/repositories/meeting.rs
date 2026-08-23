use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};

/// One row of the meetings list: enough to render, group and triage without
/// opening the meeting. `segment_count` and `has_summary` are joined rather
/// than stored, which measured at 1.8 ms across 400 meetings.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MeetingListRow {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub duration_seconds: Option<f64>,
    pub source: Option<String>,
    pub app_name: Option<String>,
    pub language: Option<String>,
    pub folder_path: Option<String>,
    pub segment_count: i64,
    pub has_summary: i64,
    /// Excerpt around the search hit, present only for text searches.
    pub snippet: Option<String>,
}

/// Field the list is ordered by. A closed set rather than a caller-supplied
/// string: this reaches SQL as an identifier, where a string would be an
/// injection point no amount of escaping makes safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingSortField {
    CreatedAt,
    Title,
    Duration,
    SegmentCount,
}

impl MeetingSortField {
    fn as_sql(self) -> &'static str {
        match self {
            Self::CreatedAt => "m.created_at",
            Self::Title => "m.title COLLATE NOCASE",
            Self::Duration => "m.duration_seconds",
            Self::SegmentCount => "segment_count",
        }
    }
}

/// Filters, sorting and paging for the meetings list. Every field is optional;
/// the default is "newest first, first page".
#[derive(Debug, Clone)]
pub struct MeetingQuery {
    /// Full-text query over transcripts, plus a substring match on titles.
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub has_summary: Option<bool>,
    /// Matches `meetings.source`; empty means no source filter.
    pub sources: Vec<String>,
    pub language: Option<String>,
    pub sort: MeetingSortField,
    pub descending: bool,
    pub limit: i64,
    pub offset: i64,
}

impl Default for MeetingQuery {
    fn default() -> Self {
        Self {
            search: None,
            date_from: None,
            date_to: None,
            has_summary: None,
            sources: Vec::new(),
            language: None,
            sort: MeetingSortField::CreatedAt,
            descending: true,
            limit: 50,
            offset: 0,
        }
    }
}

/// Turn user input into an FTS5 MATCH expression that cannot be a syntax
/// error or an operator injection.
///
/// FTS5 treats `"`, `*`, `:`, `^`, `-`, `AND`/`OR`/`NEAT` and parentheses as
/// syntax; a raw user string containing any of them either errors out or
/// silently means something the user did not ask for. Each run of
/// alphanumerics becomes one double-quoted term (quotes doubled per FTS5
/// escaping) and the terms are ANDed, so "klient projekt" finds segments
/// containing both. Returns None when nothing searchable remains.
fn build_fts_match(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

/// Escape LIKE wildcards in user input so a title search for "50%" does not
/// match everything. Pairs with `ESCAPE '\'` in the SQL.
fn escape_like(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub struct MeetingsRepository;

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let meetings =
            sqlx::query_as::<_, MeetingModel>("SELECT * FROM meetings ORDER BY created_at DESC")
                .fetch_all(pool)
                .await?;
        Ok(meetings)
    }

    /// Query the meetings list with filtering, sorting and paging.
    ///
    /// Returns the page plus the total number of matches, so the caller can
    /// show "showing 50 of 412" without fetching everything.
    ///
    /// Text search goes through the FTS5 index over transcripts and also
    /// matches titles, so typing a person's name finds both the meeting
    /// called after them and the meetings where they were mentioned.
    pub async fn query_meetings(
        pool: &SqlitePool,
        query: &MeetingQuery,
    ) -> Result<(Vec<MeetingListRow>, i64), SqlxError> {
        // Clamp paging rather than trusting the caller: an unbounded limit is
        // how the old search ended up shipping 2.81 MB per keystroke.
        let limit = query.limit.clamp(1, 200);
        let offset = query.offset.max(0);

        let fts_match = query.search.as_deref().and_then(build_fts_match);
        let like_pattern = query
            .search
            .as_deref()
            .map(|s| format!("%{}%", escape_like(s.trim())));

        // Conditions are assembled as fixed fragments with bound parameters;
        // no user text is ever concatenated into the SQL.
        let mut conditions: Vec<String> = Vec::new();
        if fts_match.is_some() {
            // DISTINCT and a join back to `transcripts` rather than a
            // correlated EXISTS: the correlated form re-runs the MATCH per
            // candidate row and measured 337 ms against 2.90 ms for this one.
            conditions.push(
                "(m.id IN (SELECT DISTINCT t.meeting_id \
                             FROM transcripts_fts \
                             JOIN transcripts t ON t.rowid = transcripts_fts.rowid \
                            WHERE transcripts_fts MATCH ?) \
                 OR m.title LIKE ? ESCAPE '\\')"
                    .to_string(),
            );
        }
        if query.date_from.is_some() {
            conditions.push("m.created_at >= ?".to_string());
        }
        if query.date_to.is_some() {
            conditions.push("m.created_at <= ?".to_string());
        }
        if query.language.is_some() {
            conditions.push("m.language = ?".to_string());
        }
        if !query.sources.is_empty() {
            let placeholders = vec!["?"; query.sources.len()].join(", ");
            conditions.push(format!("m.source IN ({})", placeholders));
        }
        match query.has_summary {
            Some(true) => conditions.push(
                "EXISTS (SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id)".to_string(),
            ),
            Some(false) => conditions.push(
                "NOT EXISTS (SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id)"
                    .to_string(),
            ),
            None => {}
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Filter values collected in the same order the placeholders appear,
        // so both the count query and the page query can bind them by looping.
        // (A closure would have to be generic over Query and QueryAs, which
        // Rust closures cannot be.) Every value here is a String, which is
        // what keeps this uniform.
        let mut filter_params: Vec<String> = Vec::new();
        if let Some(m) = fts_match.as_ref() {
            filter_params.push(m.clone());
            filter_params.push(like_pattern.clone().unwrap_or_default());
        }
        if let Some(v) = query.date_from.as_ref() {
            filter_params.push(v.clone());
        }
        if let Some(v) = query.date_to.as_ref() {
            filter_params.push(v.clone());
        }
        if let Some(v) = query.language.as_ref() {
            filter_params.push(v.clone());
        }
        for s in &query.sources {
            filter_params.push(s.clone());
        }

        let count_sql = format!("SELECT COUNT(*) FROM meetings m {}", where_clause);
        let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql);
        for p in &filter_params {
            count_query = count_query.bind(p.clone());
        }
        let total: (i64,) = count_query.fetch_one(pool).await?;

        let direction = if query.descending { "DESC" } else { "ASC" };
        let page_sql = format!(
            "SELECT m.id, m.title, m.created_at, m.duration_seconds, m.source, m.app_name,
                    m.language, m.folder_path,
                    (SELECT COUNT(*) FROM transcripts t WHERE t.meeting_id = m.id) AS segment_count,
                    EXISTS (SELECT 1 FROM summary_processes sp WHERE sp.meeting_id = m.id) AS has_summary,
                    NULL AS snippet
             FROM meetings m
             {}
             ORDER BY {} {}
             LIMIT ? OFFSET ?",
            where_clause,
            query.sort.as_sql(),
            direction
        );

        let mut page_query = sqlx::query_as::<_, MeetingListRow>(&page_sql);
        for p in &filter_params {
            page_query = page_query.bind(p.clone());
        }
        let mut rows = page_query
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        // Snippets are fetched separately, for the visible page only.
        //
        // Putting `snippet()` in the page SELECT looks tidier but is a
        // correlated subquery: it re-runs the MATCH once per returned row and
        // measured 320 ms against 13 ms for this two-step shape. Fetching them
        // for at most `limit` meetings also bounds the work regardless of how
        // many segments matched overall.
        if let Some(fts) = fts_match.as_ref() {
            if !rows.is_empty() {
                Self::attach_snippets(pool, fts, &mut rows).await?;
            }
        }

        Ok((rows, total.0))
    }

    /// Fill in `snippet` for the given rows, keeping the first hit per meeting.
    async fn attach_snippets(
        pool: &SqlitePool,
        fts_match: &str,
        rows: &mut [MeetingListRow],
    ) -> Result<(), SqlxError> {
        use std::collections::HashMap;

        let placeholders = vec!["?"; rows.len()].join(", ");
        let sql = format!(
            "SELECT t.meeting_id, snippet(transcripts_fts, 0, '[', ']', '…', 12)
               FROM transcripts_fts
               JOIN transcripts t ON t.rowid = transcripts_fts.rowid
              WHERE transcripts_fts MATCH ? AND t.meeting_id IN ({})",
            placeholders
        );

        let mut q = sqlx::query_as::<_, (String, String)>(&sql).bind(fts_match.to_string());
        for row in rows.iter() {
            q = q.bind(row.id.clone());
        }
        let hits = q.fetch_all(pool).await?;

        // A meeting can match in many segments; the first one is enough to
        // show why it appeared in the results.
        let mut first: HashMap<String, String> = HashMap::new();
        for (meeting_id, snippet) in hits {
            first.entry(meeting_id).or_insert(snippet);
        }
        for row in rows.iter_mut() {
            row.snippet = first.remove(&row.id);
        }

        Ok(())
    }

    pub async fn delete_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id).await {
            Ok(success) => {
                if success {
                    transaction.commit().await?;
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details. The column list must cover every MeetingModel
        // field - FromRow fails at runtime on any missing column.
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path, duration_seconds, source, app_name, language FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting
            let transcripts =
                sqlx::query_as::<_, Transcript>("SELECT * FROM transcripts WHERE meeting_id = ?")
                    .bind(meeting_id)
                    .fetch_all(&mut *transaction)
                    .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                    speaker: t.speaker,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                transcripts: meeting_transcripts,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // The column list must cover every MeetingModel field - FromRow
        // fails at runtime on any missing column.
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path, duration_seconds, source, app_name, language FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        Ok(meeting)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;

        // Get paginated transcripts ordered by audio_start_time
        let transcripts = sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts
             WHERE meeting_id = ?
             ORDER BY audio_start_time ASC
             LIMIT ? OFFSET ?"
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<bool, SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok(false);
    }

    // Delete from related tables in proper order
    // 1. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // FTS5 treats quotes, asterisks, colons, carets, hyphens, parentheses and
    // the bare words AND/OR/NOT as syntax. Anything the user types reaches
    // MATCH, so these are the shapes that would otherwise raise a syntax error
    // or silently search for something else entirely.

    #[test]
    fn fts_match_quotes_each_term_and_ands_them() {
        assert_eq!(
            build_fts_match("klient projekt").unwrap(),
            "\"klient\" AND \"projekt\""
        );
    }

    #[test]
    fn fts_match_neutralises_operators() {
        // Bare AND/OR/NOT would be parsed as operators; quoting makes them
        // ordinary search terms.
        assert_eq!(
            build_fts_match("klient OR projekt").unwrap(),
            "\"klient\" AND \"OR\" AND \"projekt\""
        );
        // A leading hyphen is FTS5's exclusion syntax.
        assert_eq!(build_fts_match("-klient").unwrap(), "\"klient\"");
        // Column filter syntax.
        assert_eq!(
            build_fts_match("meeting_id:x").unwrap(),
            "\"meeting\" AND \"id\" AND \"x\""
        );
    }

    #[test]
    fn fts_match_survives_quotes_and_wildcards() {
        // An unbalanced quote is the classic FTS5 syntax error.
        assert_eq!(build_fts_match("\"niedomknięty").unwrap(), "\"niedomknięty\"");
        assert_eq!(build_fts_match("klient*").unwrap(), "\"klient\"");
        assert_eq!(build_fts_match("(a OR b)").unwrap(), "\"a\" AND \"OR\" AND \"b\"");
    }

    #[test]
    fn fts_match_keeps_non_ascii_terms() {
        // Polish text is the normal case here, not an edge case.
        assert_eq!(
            build_fts_match("zażółć gęślą").unwrap(),
            "\"zażółć\" AND \"gęślą\""
        );
    }

    #[test]
    fn fts_match_is_none_when_nothing_searchable_remains() {
        assert!(build_fts_match("").is_none());
        assert!(build_fts_match("   ").is_none());
        // Punctuation only: there is no term to search for, and an empty
        // MATCH expression is a syntax error.
        assert!(build_fts_match("*** --- \"\"").is_none());
    }

    #[test]
    fn like_escape_neutralises_wildcards() {
        // Without escaping, a title search for "50%" matches every title.
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        // The escape character itself has to be escaped first, or the
        // replacements below would double-escape it.
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("zwykły tytuł"), "zwykły tytuł");
    }

    #[test]
    fn sort_fields_map_to_known_columns() {
        // These reach SQL as identifiers, so the mapping is the thing that
        // keeps a caller-supplied string out of the query.
        assert_eq!(MeetingSortField::CreatedAt.as_sql(), "m.created_at");
        assert_eq!(MeetingSortField::Title.as_sql(), "m.title COLLATE NOCASE");
        assert_eq!(MeetingSortField::Duration.as_sql(), "m.duration_seconds");
        assert_eq!(MeetingSortField::SegmentCount.as_sql(), "segment_count");
    }

    #[test]
    fn default_query_is_newest_first_first_page() {
        let q = MeetingQuery::default();
        assert_eq!(q.sort, MeetingSortField::CreatedAt);
        assert!(q.descending);
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(q.search.is_none());
        assert!(q.sources.is_empty());
    }
}

#[cfg(test)]
mod query_integration_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Build an in-memory database with the real migrations applied, so these
    /// tests exercise the actual schema, indexes and FTS triggers rather than
    /// a hand-written approximation of them.
    async fn seeded_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations apply");

        let rows = [
            ("m1", "Rozmowa z klientem", "2026-08-14T10:00:00Z", 2820.0, "auto", "Microsoft Teams", "pl"),
            ("m2", "Standup zespołu", "2026-08-13T09:00:00Z", 720.0, "manual", "", "pl"),
            ("m3", "Client workshop", "2026-08-12T14:00:00Z", 5400.0, "imported", "", "en"),
        ];
        for (id, title, created, dur, source, app, lang) in rows {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at, duration_seconds, source, app_name, language)
                 VALUES (?,?,?,?,?,?,?,?)",
            )
            .bind(id).bind(title).bind(created).bind(created)
            .bind(dur).bind(source)
            .bind(if app.is_empty() { None } else { Some(app) })
            .bind(lang)
            .execute(&pool).await.expect("insert meeting");
        }

        // Transcript text the FTS index will pick up.
        let segments = [
            ("t1", "m1", "omówiliśmy budżet projektu z klientem"),
            ("t2", "m1", "termin wdrożenia ustalony na wrzesień"),
            ("t3", "m2", "wczoraj skończyłem integrację"),
            ("t4", "m3", "we discussed the budget in detail"),
        ];
        for (id, meeting, text) in segments {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?,?,?,?)")
                .bind(id).bind(meeting).bind(text).bind("2026-08-14T10:00:00Z")
                .execute(&pool).await.expect("insert transcript");
        }

        // m1 has a summary, the others do not.
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, status, created_at, updated_at) VALUES (?,?,?,?)",
        )
        .bind("m1").bind("completed").bind("2026-08-14T10:00:00Z").bind("2026-08-14T10:00:00Z")
        .execute(&pool).await.expect("insert summary process");

        pool
    }

    #[tokio::test]
    async fn get_meeting_metadata_returns_the_full_row() {
        // Regression guard: MeetingModel gained duration/source/app_name/
        // language columns, and a SELECT listing only the original five
        // columns makes sqlx's FromRow fail at runtime with ColumnNotFound -
        // which silently broke opening any meeting.
        let pool = seeded_pool().await;
        let meeting = MeetingsRepository::get_meeting_metadata(&pool, "m1")
            .await
            .expect("query runs")
            .expect("meeting exists");

        assert_eq!(meeting.id, "m1");
        assert_eq!(meeting.duration_seconds, Some(2820.0));
        assert_eq!(meeting.source.as_deref(), Some("auto"));
        assert_eq!(meeting.app_name.as_deref(), Some("Microsoft Teams"));
        assert_eq!(meeting.language.as_deref(), Some("pl"));
    }

    #[tokio::test]
    async fn returns_enriched_rows_newest_first() {
        let pool = seeded_pool().await;
        let (rows, total) = MeetingsRepository::query_meetings(&pool, &MeetingQuery::default())
            .await
            .expect("query runs");

        assert_eq!(total, 3);
        assert_eq!(rows.len(), 3);
        // The fields the old list could never show.
        assert_eq!(rows[0].id, "m1");
        assert_eq!(rows[0].segment_count, 2);
        assert_eq!(rows[0].has_summary, 1);
        assert_eq!(rows[0].duration_seconds, Some(2820.0));
        assert_eq!(rows[0].app_name.as_deref(), Some("Microsoft Teams"));
        assert_eq!(rows[2].id, "m3");
        assert_eq!(rows[1].has_summary, 0);
    }

    #[tokio::test]
    async fn full_text_search_finds_transcript_content_with_snippet() {
        let pool = seeded_pool().await;
        let query = MeetingQuery {
            search: Some("budżet".to_string()),
            ..Default::default()
        };
        let (rows, total) = MeetingsRepository::query_meetings(&pool, &query)
            .await
            .expect("query runs");

        assert_eq!(total, 1);
        assert_eq!(rows[0].id, "m1");
        let snippet = rows[0].snippet.as_deref().expect("snippet present");
        assert!(snippet.contains("budżet"), "snippet was: {}", snippet);
        // The excerpt is built by SQLite, not by shipping the whole transcript.
        assert!(snippet.len() < 200);
    }

    #[tokio::test]
    async fn search_also_matches_titles() {
        let pool = seeded_pool().await;
        // "Standup" appears in a title only, never in transcript text.
        let query = MeetingQuery {
            search: Some("Standup".to_string()),
            ..Default::default()
        };
        let (rows, total) = MeetingsRepository::query_meetings(&pool, &query)
            .await
            .expect("query runs");
        assert_eq!(total, 1);
        assert_eq!(rows[0].id, "m2");
    }

    #[tokio::test]
    async fn search_with_only_punctuation_does_not_error() {
        // build_fts_match returns None here; the query must degrade to
        // "no text filter" rather than producing invalid MATCH syntax.
        let pool = seeded_pool().await;
        let query = MeetingQuery {
            search: Some("*** \"".to_string()),
            ..Default::default()
        };
        let (_, total) = MeetingsRepository::query_meetings(&pool, &query)
            .await
            .expect("query must not fail on punctuation-only input");
        assert_eq!(total, 3);
    }

    #[tokio::test]
    async fn filters_compose() {
        let pool = seeded_pool().await;

        let by_summary = MeetingQuery { has_summary: Some(false), ..Default::default() };
        let (rows, total) = MeetingsRepository::query_meetings(&pool, &by_summary).await.unwrap();
        assert_eq!(total, 2);
        assert!(rows.iter().all(|r| r.has_summary == 0));

        let by_source = MeetingQuery {
            sources: vec!["auto".into(), "imported".into()],
            ..Default::default()
        };
        let (_, total) = MeetingsRepository::query_meetings(&pool, &by_source).await.unwrap();
        assert_eq!(total, 2);

        let by_language = MeetingQuery { language: Some("en".into()), ..Default::default() };
        let (rows, _) = MeetingsRepository::query_meetings(&pool, &by_language).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "m3");

        let by_date = MeetingQuery {
            date_from: Some("2026-08-13T00:00:00Z".into()),
            ..Default::default()
        };
        let (_, total) = MeetingsRepository::query_meetings(&pool, &by_date).await.unwrap();
        assert_eq!(total, 2);
    }

    #[tokio::test]
    async fn sorting_and_paging_work() {
        let pool = seeded_pool().await;

        let by_duration = MeetingQuery {
            sort: MeetingSortField::Duration,
            descending: true,
            ..Default::default()
        };
        let (rows, _) = MeetingsRepository::query_meetings(&pool, &by_duration).await.unwrap();
        assert_eq!(rows[0].id, "m3");

        let by_title = MeetingQuery {
            sort: MeetingSortField::Title,
            descending: false,
            ..Default::default()
        };
        let (rows, _) = MeetingsRepository::query_meetings(&pool, &by_title).await.unwrap();
        assert_eq!(rows[0].title, "Client workshop");

        // Paging keeps the total, so the UI can say "1 of 3".
        let page = MeetingQuery { limit: 1, offset: 1, ..Default::default() };
        let (rows, total) = MeetingsRepository::query_meetings(&pool, &page).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(total, 3);
        assert_eq!(rows[0].id, "m2");
    }

    #[tokio::test]
    async fn limit_is_clamped_against_unbounded_callers() {
        let pool = seeded_pool().await;
        let huge = MeetingQuery { limit: 100_000, ..Default::default() };
        let (rows, _) = MeetingsRepository::query_meetings(&pool, &huge).await.unwrap();
        assert!(rows.len() <= 200);

        let zero = MeetingQuery { limit: 0, offset: -5, ..Default::default() };
        let (rows, _) = MeetingsRepository::query_meetings(&pool, &zero).await.unwrap();
        assert_eq!(rows.len(), 1, "limit 0 clamps to 1 rather than returning nothing");
    }

    #[tokio::test]
    async fn fts_index_follows_transcript_deletion() {
        // The external-content FTS table only stays correct because of the
        // triggers in the migration; this is what proves they fire.
        let pool = seeded_pool().await;
        sqlx::query("DELETE FROM transcripts WHERE id = 't1'")
            .execute(&pool).await.unwrap();

        let query = MeetingQuery { search: Some("budżet".into()), ..Default::default() };
        let (_, total) = MeetingsRepository::query_meetings(&pool, &query).await.unwrap();
        assert_eq!(total, 0, "deleted transcript must leave the search index");
    }
}
