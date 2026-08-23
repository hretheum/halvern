use crate::audio::transcription::RemoteTranscriptionConfig;
use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use sqlx::SqlitePool;

#[derive(serde::Deserialize, Debug)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

pub struct SettingsRepository;

// Transcript providers: localWhisper, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, added openrouter
// NOTE: Handle data exclusion in the higher layer as this is database abstraction layer(using SELECT *)

impl SettingsRepository {
    pub async fn get_model_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<Setting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(setting)
    }

    pub async fn save_model_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
        whisper_model: &str,
        ollama_endpoint: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        // Using id '1' for backward compatibility
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint)
            VALUES ('1', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                whisperModel = excluded.whisperModel,
                ollamaEndpoint = excluded.ollamaEndpoint
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(whisper_model)
        .bind(ollama_endpoint)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config (customOpenAIConfig) instead of a separate API key column
        if provider == "custom-openai" {
            return Err(sqlx::Error::Protocol(
                "custom-openai provider should use save_custom_openai_config() instead of save_api_key()".into(),
            ));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "claude" => "anthropicApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider),
                ))
            }
        };

        // `provider` also seeds the row's own provider column on a fresh
        // insert. The literal 'openai' used to sit here regardless of which
        // provider's key was actually being saved, so the very first key
        // saved on an empty database left the row misdeclared as OpenAI —
        // e.g. a Claude key on a fresh install configured the app for
        // OpenAI, with no OpenAI key to show for it. On every insert after
        // the first this made no difference, because save_model_config
        // (which sets the real provider) always runs before save_api_key in
        // the one place that calls it.
        let query = format!(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, "{}")
            VALUES ('1', $2, 'gpt-4o-2024-11-20', 'large-v3', $1)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = $1
            "#,
            api_key_column, api_key_column
        );
        sqlx::query(&query)
            .bind(api_key)
            .bind(provider)
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        // Custom OpenAI uses JSON config - extract API key from there
        if provider == "custom-openai" {
            let config = Self::get_custom_openai_config(pool).await?;
            return Ok(config.and_then(|c| c.api_key));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(None), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider),
                ))
            }
        };

        let query = format!(
            "SELECT {} FROM settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let api_key: Option<String> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        // A column that was never set for this provider decodes as an empty
        // string rather than SQL NULL once any other setting has created the
        // row. An empty key is never a usable one, so callers that only ever
        // check `is_some()` to decide whether a provider is configured must
        // see it the same way as "never set": None.
        Ok(api_key.filter(|k| !k.is_empty()))
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting =
            sqlx::query_as::<_, TranscriptSetting>("SELECT * FROM transcript_settings LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting)

    }

    pub async fn save_transcript_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', $1, $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model
            "#,
        )
        .bind(provider)
        .bind(model)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(()), // Parakeet doesn't need an API key, return early
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider),
                ))
            }
        };

        // See save_api_key for why `provider` is bound here rather than the
        // literal 'parakeet' the insert used to hardcode: it left the very
        // first transcript key saved on a fresh database attributed to
        // parakeet (local, no key needed) regardless of whose key it was.
        let query = format!(
            r#"
            INSERT INTO transcript_settings (id, provider, model, "{}")
            VALUES ('1', $2, '{}', $1)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = $1
            "#,
            api_key_column, crate::config::DEFAULT_PARAKEET_MODEL, api_key_column
        );
        sqlx::query(&query)
            .bind(api_key)
            .bind(provider)
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(None), // Parakeet doesn't need an API key
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider),
                ))
            }
        };

        let query = format!(
            "SELECT {} FROM transcript_settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let api_key: Option<String> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        // See get_api_key: an unset column decodes as "" once the row
        // exists, and that must read the same as "never set".
        Ok(api_key.filter(|k| !k.is_empty()))
    }

    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config - clear the entire config
        if provider == "custom-openai" {
            sqlx::query("UPDATE settings SET customOpenAIConfig = NULL WHERE id = '1'")
                .execute(pool)
                .await?;
            return Ok(());
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider),
                ))
            }
        };

        let query = format!(
            "UPDATE settings SET {} = NULL WHERE id = '1'",
            api_key_column
        );
        sqlx::query(&query).execute(pool).await?;

        Ok(())
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Gets the custom OpenAI configuration from JSON
    ///
    /// # Returns
    /// * `Ok(Some(CustomOpenAIConfig))` - Config exists and is valid JSON
    /// * `Ok(None)` - No config stored
    /// * `Err(sqlx::Error)` - Database error
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("customOpenAIConfig");

                if let Some(json) = config_json {
                    // Parse JSON into CustomOpenAIConfig
                    let config: CustomOpenAIConfig = serde_json::from_str(&json)
                        .map_err(|e| sqlx::Error::Protocol(
                            format!("Invalid JSON in customOpenAIConfig: {}", e)
                        ))?;

                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Saves the custom OpenAI configuration as JSON
    ///
    /// # Arguments
    /// * `pool` - Database connection pool
    /// * `config` - CustomOpenAIConfig to save (includes endpoint, apiKey, model, maxTokens, temperature, topP)
    ///
    /// # Returns
    /// * `Ok(())` - Config saved successfully
    /// * `Err(sqlx::Error)` - Database or JSON serialization error
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        // Serialize config to JSON
        let config_json = serde_json::to_string(config)
            .map_err(|e| sqlx::Error::Protocol(
                format!("Failed to serialize config to JSON: {}", e)
            ))?;

        // Upsert into settings table. `provider` and `model` are rewritten on
        // conflict, not just inserted: the ON CONFLICT branch used to touch
        // only customOpenAIConfig, so switching the custom model (or
        // reactivating this provider over a row another provider had
        // claimed) left the model column, and provider along with it,
        // showing the previous configuration to anything reading the column
        // directly rather than the JSON blob.
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = 'custom-openai',
                model = excluded.model,
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Reads the remote transcription endpoint configuration (batch jobs only).
    /// `None` means the feature was never configured — a normal state, not an error.
    pub async fn get_remote_transcription_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<RemoteTranscriptionConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT remoteTranscriptionConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("remoteTranscriptionConfig");
                match config_json {
                    Some(json) => {
                        let config: RemoteTranscriptionConfig = serde_json::from_str(&json)
                            .map_err(|e| {
                                sqlx::Error::Protocol(
                                    format!("Invalid JSON in remoteTranscriptionConfig: {}", e),
                                )
                            })?;
                        Ok(Some(config))
                    }
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    /// Saves the remote transcription endpoint configuration as JSON.
    /// The upsert's INSERT arm mirrors `save_custom_openai_config`: it only
    /// fires when the settings row does not exist yet, and then seeds the
    /// unrelated columns with the same defaults that function uses.
    pub async fn save_remote_transcription_config(
        pool: &SqlitePool,
        config: &RemoteTranscriptionConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        let config_json = serde_json::to_string(config).map_err(|e| {
            sqlx::Error::Protocol(format!("Failed to serialize config to JSON: {}", e))
        })?;

        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, remoteTranscriptionConfig)
            VALUES ('1', 'parakeet', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                remoteTranscriptionConfig = excluded.remoteTranscriptionConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Removes the remote transcription endpoint configuration.
    pub async fn clear_remote_transcription_config(
        pool: &SqlitePool,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE settings SET remoteTranscriptionConfig = NULL WHERE id = '1'
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}

// ===== OBSIDIAN EXPORT SETTINGS =====

/// Obsidian export configuration as stored in the `settings` row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObsidianConfig {
    /// Base destination. Everything falls back to this.
    pub vault_path: Option<String>,
    /// Optional override for transcript notes.
    pub transcript_path: Option<String>,
    /// Optional override for summary notes.
    pub summary_path: Option<String>,
    pub auto_export: bool,
}

impl SettingsRepository {
    /// Reads the Obsidian export configuration.
    ///
    /// A missing settings row is not an error, it simply means the feature was
    /// never configured. Blank strings are normalised to `None` so that callers
    /// never have to distinguish "unset" from "set to empty".
    pub async fn get_obsidian_settings(
        pool: &SqlitePool,
    ) -> std::result::Result<ObsidianConfig, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            "SELECT obsidianVaultPath, obsidianTranscriptPath, obsidianSummaryPath, \
             obsidianAutoExport FROM settings WHERE id = '1' LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;

        let Some(record) = row else {
            return Ok(ObsidianConfig::default());
        };

        fn non_empty(value: Option<String>) -> Option<String> {
            value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        }

        Ok(ObsidianConfig {
            vault_path: non_empty(record.try_get("obsidianVaultPath").unwrap_or(None)),
            transcript_path: non_empty(record.try_get("obsidianTranscriptPath").unwrap_or(None)),
            summary_path: non_empty(record.try_get("obsidianSummaryPath").unwrap_or(None)),
            auto_export: record.try_get::<i64, _>("obsidianAutoExport").unwrap_or(0) != 0,
        })
    }

    /// Persists the Obsidian export configuration.
    ///
    /// The upsert only touches the export columns, so an existing row keeps its
    /// provider and model. The literals on the INSERT branch are only used when
    /// no settings row exists yet, matching `save_api_key`.
    pub async fn save_obsidian_settings(
        pool: &SqlitePool,
        config: &ObsidianConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel,
                                  obsidianVaultPath, obsidianTranscriptPath,
                                  obsidianSummaryPath, obsidianAutoExport)
            VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                obsidianVaultPath = excluded.obsidianVaultPath,
                obsidianTranscriptPath = excluded.obsidianTranscriptPath,
                obsidianSummaryPath = excluded.obsidianSummaryPath,
                obsidianAutoExport = excluded.obsidianAutoExport
            "#,
        )
        .bind(config.vault_path.as_deref())
        .bind(config.transcript_path.as_deref())
        .bind(config.summary_path.as_deref())
        .bind(if config.auto_export { 1_i64 } else { 0_i64 })
        .execute(pool)
        .await?;

        Ok(())
    }

    // ===== MEETING DETECTION SETTINGS =====

    /// Reads the automatic meeting detection configuration.
    ///
    /// Falls back to [`DetectionConfig::default`] when nothing is stored yet or
    /// when the stored JSON no longer parses, so a shape change in a future
    /// version degrades to sane defaults instead of breaking startup.
    pub async fn get_detection_settings(
        pool: &SqlitePool,
    ) -> std::result::Result<crate::detection::DetectionConfig, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query("SELECT meetingDetectionConfig FROM settings WHERE id = '1' LIMIT 1")
            .fetch_optional(pool)
            .await?;

        let stored: Option<String> = row.and_then(|r| r.try_get("meetingDetectionConfig").ok()).flatten();

        let Some(json) = stored.filter(|s| !s.trim().is_empty()) else {
            return Ok(crate::detection::DetectionConfig::default());
        };

        match serde_json::from_str(&json) {
            Ok(config) => Ok(config),
            Err(e) => {
                log::warn!(
                    "Stored meetingDetectionConfig is unreadable ({}), falling back to defaults",
                    e
                );
                Ok(crate::detection::DetectionConfig::default())
            }
        }
    }

    /// Persists the automatic meeting detection configuration.
    pub async fn save_detection_settings(
        pool: &SqlitePool,
        config: &crate::detection::DetectionConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        let json = serde_json::to_string(config).map_err(|e| {
            sqlx::Error::Protocol(format!("Could not serialise detection config: {}", e))
        })?;

        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, meetingDetectionConfig)
            VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1)
            ON CONFLICT(id) DO UPDATE SET
                meetingDetectionConfig = excluded.meetingDetectionConfig
            "#,
        )
        .bind(json)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::DetectionConfig;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    /// In-memory database with the real migrations applied, mirroring the
    /// harness the transcripts repository tests established. Nothing here ever
    /// reaches a keychain or a user profile — every setting these functions
    /// touch, API keys included, lives in the `settings` table.
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

    fn custom_config() -> CustomOpenAIConfig {
        CustomOpenAIConfig {
            endpoint: "http://localhost:8000/v1".to_string(),
            api_key: Some("sk-lokalny".to_string()),
            model: "llama-3-70b".to_string(),
            max_tokens: Some(4096),
            temperature: Some(0.3),
            top_p: Some(0.9),
        }
    }

    // ===== MODEL CONFIG =====

    #[tokio::test]
    async fn there_is_no_model_config_until_one_is_saved() {
        let pool = pool().await;
        assert!(SettingsRepository::get_model_config(&pool)
            .await
            .expect("query runs")
            .is_none());
    }

    #[tokio::test]
    async fn saving_the_model_config_twice_updates_the_single_settings_row() {
        // Everything in this repository writes to the row with id '1'; a second
        // save must not leave two competing configurations behind.
        let pool = pool().await;

        SettingsRepository::save_model_config(
            &pool,
            "ollama",
            "llama3.1:8b",
            "large-v3",
            Some("http://localhost:11434"),
        )
        .await
        .expect("first save succeeds");
        SettingsRepository::save_model_config(&pool, "claude", "claude-sonnet-4", "medium", None)
            .await
            .expect("second save succeeds");

        let rows: i64 = sqlx::query("SELECT COUNT(*) AS c FROM settings")
            .fetch_one(&pool)
            .await
            .expect("count runs")
            .get("c");
        assert_eq!(rows, 1);

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .expect("query runs")
            .expect("row exists");
        assert_eq!(setting.id, "1");
        assert_eq!(setting.provider, "claude");
        assert_eq!(setting.model, "claude-sonnet-4");
        assert_eq!(setting.whisper_model, "medium");
        assert_eq!(
            setting.ollama_endpoint, None,
            "the endpoint is overwritten, not merged"
        );
    }

    #[tokio::test]
    async fn changing_the_model_config_keeps_the_stored_api_keys() {
        // Provider and key live in the same row, so an upsert that named the
        // key columns would wipe them; this pins that it does not.
        let pool = pool().await;
        SettingsRepository::save_api_key(&pool, "claude", "sk-ant-tajne")
            .await
            .unwrap();

        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "large-v3", None)
            .await
            .unwrap();

        assert_eq!(
            SettingsRepository::get_api_key(&pool, "claude")
                .await
                .expect("query runs")
                .as_deref(),
            Some("sk-ant-tajne")
        );
    }

    // ===== SUMMARY PROVIDER API KEYS =====

    #[tokio::test]
    async fn every_summary_provider_key_round_trips_to_its_own_column() {
        let pool = pool().await;
        let keys = [
            ("openai", "sk-openai"),
            ("claude", "sk-ant"),
            ("ollama", "sk-ollama"),
            ("groq", "gsk-groq"),
            ("openrouter", "sk-or"),
        ];

        for (provider, key) in keys {
            SettingsRepository::save_api_key(&pool, provider, key)
                .await
                .unwrap_or_else(|e| panic!("saving {provider} failed: {e}"));
        }
        // Written one after another into the same row, so the last write must
        // not have clobbered the earlier providers.
        for (provider, key) in keys {
            assert_eq!(
                SettingsRepository::get_api_key(&pool, provider)
                    .await
                    .unwrap_or_else(|e| panic!("reading {provider} failed: {e}"))
                    .as_deref(),
                Some(key)
            );
        }
    }

    #[tokio::test]
    async fn the_first_api_key_saved_creates_a_row_for_its_own_provider() {
        // Regression test: when no settings row exists yet, save_api_key
        // used to create one with hardcoded provider 'openai' whatever
        // provider the key actually belonged to — a user who pasted a Claude
        // key before choosing a provider ended up configured for OpenAI with
        // no OpenAI key. The row's provider now matches the key being saved.
        let pool = pool().await;

        SettingsRepository::save_api_key(&pool, "claude", "sk-ant-tajne")
            .await
            .unwrap();

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .expect("a row was created");
        assert_eq!(setting.provider, "claude");
        assert_eq!(setting.whisper_model, "large-v3");
        assert_eq!(setting.anthropic_api_key.as_deref(), Some("sk-ant-tajne"));
    }

    #[tokio::test]
    async fn the_builtin_provider_needs_no_key_and_writes_nothing() {
        let pool = pool().await;

        SettingsRepository::save_api_key(&pool, "builtin-ai", "ignorowane")
            .await
            .expect("accepted");
        SettingsRepository::delete_api_key(&pool, "builtin-ai")
            .await
            .expect("accepted");
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "builtin-ai")
                .await
                .expect("query runs"),
            None
        );
        assert!(
            SettingsRepository::get_model_config(&pool)
                .await
                .unwrap()
                .is_none(),
            "the early return means no settings row was created"
        );
    }

    #[tokio::test]
    async fn an_unknown_summary_provider_is_rejected_on_every_key_path() {
        // The provider name arrives from the frontend, so a typo has to fail
        // loudly rather than silently target a column that does not exist.
        let pool = pool().await;

        assert!(SettingsRepository::save_api_key(&pool, "gemini", "k").await.is_err());
        assert!(SettingsRepository::get_api_key(&pool, "gemini").await.is_err());
        assert!(SettingsRepository::delete_api_key(&pool, "gemini").await.is_err());
    }

    #[tokio::test]
    async fn custom_openai_is_pushed_away_from_the_plain_api_key_path() {
        // Its key lives inside the JSON blob, so accepting it here would write
        // a key nothing ever reads.
        let pool = pool().await;

        let err = SettingsRepository::save_api_key(&pool, "custom-openai", "sk-lokalny")
            .await
            .expect_err("rejected");
        assert!(
            err.to_string().contains("save_custom_openai_config"),
            "the error points at the right function, got: {err}"
        );
    }

    #[tokio::test]
    async fn an_api_key_that_was_never_set_reads_back_as_none() {
        // Regression test: the underlying column decodes a NULL as "" once
        // any unrelated setting has created the row (sqlite/sqlx quirk on
        // this driver), so get_api_key used to report Some("") for a
        // provider whose key was never entered — indistinguishable from a
        // real key to a caller testing `is_some()`. Empty now reads as
        // unset, matching the honest None returned before any row exists.
        let pool = pool().await;
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "openai")
                .await
                .expect("query runs"),
            None,
            "with no settings row at all the answer is honest"
        );

        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "large-v3", None)
            .await
            .unwrap();

        assert_eq!(
            SettingsRepository::get_api_key(&pool, "openai")
                .await
                .expect("query runs"),
            None,
            "an unset key on an existing row still reads as unset"
        );
    }

    #[tokio::test]
    async fn deleting_one_api_key_leaves_the_others_alone() {
        let pool = pool().await;
        SettingsRepository::save_api_key(&pool, "openai", "sk-openai")
            .await
            .unwrap();
        SettingsRepository::save_api_key(&pool, "groq", "gsk-groq")
            .await
            .unwrap();

        SettingsRepository::delete_api_key(&pool, "openai")
            .await
            .expect("delete runs");

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(setting.openai_api_key, None);
        assert_eq!(setting.groq_api_key.as_deref(), Some("gsk-groq"));
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "openai")
                .await
                .expect("query runs"),
            None,
            "a deleted key reads back as unset, not as an empty string"
        );
    }

    #[tokio::test]
    async fn deleting_a_key_with_no_settings_row_is_accepted() {
        // Clearing a key the user never entered is a no-op, not a failure.
        let pool = pool().await;
        SettingsRepository::delete_api_key(&pool, "openai")
            .await
            .expect("delete runs");
    }

    // ===== CUSTOM OPENAI CONFIG =====

    #[tokio::test]
    async fn there_is_no_custom_openai_config_until_one_is_saved() {
        let pool = pool().await;
        assert!(SettingsRepository::get_custom_openai_config(&pool)
            .await
            .expect("query runs")
            .is_none());

        // A settings row without the blob is also "not configured" rather than
        // an error — unlike the plain API key columns above.
        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "large-v3", None)
            .await
            .unwrap();
        assert!(SettingsRepository::get_custom_openai_config(&pool)
            .await
            .expect("query runs")
            .is_none());
    }

    #[tokio::test]
    async fn the_custom_openai_config_round_trips_through_json() {
        let pool = pool().await;
        let config = custom_config();

        SettingsRepository::save_custom_openai_config(&pool, &config)
            .await
            .expect("save succeeds");

        let stored = SettingsRepository::get_custom_openai_config(&pool)
            .await
            .expect("query runs")
            .expect("config exists");
        assert_eq!(stored.endpoint, config.endpoint);
        assert_eq!(stored.api_key, config.api_key);
        assert_eq!(stored.model, config.model);
        assert_eq!(stored.max_tokens, config.max_tokens);
        assert_eq!(stored.temperature, config.temperature);
        assert_eq!(stored.top_p, config.top_p);

        // The key inside the blob is what get_api_key returns for this provider.
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "custom-openai")
                .await
                .expect("query runs")
                .as_deref(),
            Some("sk-lokalny")
        );
    }

    #[tokio::test]
    async fn saving_a_custom_openai_config_over_an_existing_row_switches_it_over() {
        // Regression test: the ON CONFLICT branch used to update the JSON
        // blob alone, leaving settings.provider and settings.model showing
        // whatever the row was configured for before — so switching to (or
        // updating) custom OpenAI over an already-configured app left
        // anything reading those columns directly seeing a stale provider or
        // model name, even though the blob itself was current.
        let pool = pool().await;
        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "large-v3", None)
            .await
            .unwrap();

        SettingsRepository::save_custom_openai_config(&pool, &custom_config())
            .await
            .expect("save succeeds");

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(setting.provider, "custom-openai", "the column now agrees with the blob");
        assert_eq!(setting.model, "llama-3-70b");
        assert_eq!(
            SettingsRepository::get_custom_openai_config(&pool)
                .await
                .unwrap()
                .expect("config exists")
                .model,
            "llama-3-70b"
        );
    }

    #[tokio::test]
    async fn a_fresh_database_gets_the_custom_openai_provider_from_the_first_save() {
        let pool = pool().await;

        SettingsRepository::save_custom_openai_config(&pool, &custom_config())
            .await
            .expect("save succeeds");

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(setting.provider, "custom-openai");
        assert_eq!(setting.model, "llama-3-70b");
    }

    #[tokio::test]
    async fn deleting_the_custom_openai_key_clears_the_whole_configuration() {
        // There is no key column to null out, so the delete takes the endpoint
        // and the sampling parameters with it.
        let pool = pool().await;
        SettingsRepository::save_custom_openai_config(&pool, &custom_config())
            .await
            .unwrap();

        SettingsRepository::delete_api_key(&pool, "custom-openai")
            .await
            .expect("delete runs");

        assert!(SettingsRepository::get_custom_openai_config(&pool)
            .await
            .expect("query runs")
            .is_none());
    }

    #[tokio::test]
    async fn a_custom_openai_blob_that_no_longer_parses_surfaces_as_an_error() {
        // Unlike the detection config, which falls back to defaults, a broken
        // blob here fails the read outright — there is no sane default endpoint.
        let pool = pool().await;
        SettingsRepository::save_custom_openai_config(&pool, &custom_config())
            .await
            .unwrap();
        sqlx::query("UPDATE settings SET customOpenAIConfig = ? WHERE id = '1'")
            .bind("{ to nie jest json")
            .execute(&pool)
            .await
            .unwrap();

        let err = SettingsRepository::get_custom_openai_config(&pool)
            .await
            .expect_err("unreadable JSON is an error");
        assert!(
            err.to_string().contains("Invalid JSON in customOpenAIConfig"),
            "got: {err}"
        );
    }

    // ===== TRANSCRIPT CONFIG =====

    #[tokio::test]
    async fn there_is_no_transcript_config_until_one_is_saved() {
        let pool = pool().await;
        assert!(SettingsRepository::get_transcript_config(&pool)
            .await
            .expect("query runs")
            .is_none());
    }

    #[tokio::test]
    async fn saving_the_transcript_config_twice_updates_the_single_row() {
        let pool = pool().await;

        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .expect("first save succeeds");
        SettingsRepository::save_transcript_config(&pool, "deepgram", "nova-3")
            .await
            .expect("second save succeeds");

        let rows: i64 = sqlx::query("SELECT COUNT(*) AS c FROM transcript_settings")
            .fetch_one(&pool)
            .await
            .expect("count runs")
            .get("c");
        assert_eq!(rows, 1);

        let setting = SettingsRepository::get_transcript_config(&pool)
            .await
            .expect("query runs")
            .expect("row exists");
        assert_eq!(setting.id, "1");
        assert_eq!(setting.provider, "deepgram");
        assert_eq!(setting.model, "nova-3");
    }

    #[tokio::test]
    async fn every_transcript_provider_key_round_trips_to_its_own_column() {
        let pool = pool().await;
        let keys = [
            ("localWhisper", "whisper-key"),
            ("deepgram", "dg-key"),
            ("elevenLabs", "el-key"),
            ("groq", "gsk-key"),
            ("openai", "sk-key"),
        ];

        for (provider, key) in keys {
            SettingsRepository::save_transcript_api_key(&pool, provider, key)
                .await
                .unwrap_or_else(|e| panic!("saving {provider} failed: {e}"));
        }
        for (provider, key) in keys {
            assert_eq!(
                SettingsRepository::get_transcript_api_key(&pool, provider)
                    .await
                    .unwrap_or_else(|e| panic!("reading {provider} failed: {e}"))
                    .as_deref(),
                Some(key)
            );
        }
    }

    #[tokio::test]
    async fn parakeet_runs_locally_and_ignores_api_keys_in_both_directions() {
        let pool = pool().await;

        SettingsRepository::save_transcript_api_key(&pool, "parakeet", "cokolwiek")
            .await
            .expect("accepted");
        assert!(
            SettingsRepository::get_transcript_config(&pool)
                .await
                .unwrap()
                .is_none(),
            "the early return means no row was created"
        );
        assert_eq!(
            SettingsRepository::get_transcript_api_key(&pool, "parakeet")
                .await
                .expect("query runs"),
            None
        );
    }

    #[tokio::test]
    async fn the_first_transcript_key_saved_creates_a_row_for_its_own_provider() {
        // Regression test, mirror image of the summary-side fix: a key saved
        // before a provider was chosen used to leave the app configured for
        // local Parakeet transcription, ignoring the paid service whose key
        // was just entered. The row's provider now matches the key.
        let pool = pool().await;

        SettingsRepository::save_transcript_api_key(&pool, "deepgram", "dg-key")
            .await
            .unwrap();

        let setting = SettingsRepository::get_transcript_config(&pool)
            .await
            .unwrap()
            .expect("a row was created");
        assert_eq!(setting.provider, "deepgram");
        assert_eq!(setting.deepgram_api_key.as_deref(), Some("dg-key"));
    }

    #[tokio::test]
    async fn an_unknown_transcript_provider_is_rejected_in_both_directions() {
        let pool = pool().await;

        assert!(SettingsRepository::save_transcript_api_key(&pool, "whisperX", "k")
            .await
            .is_err());
        assert!(SettingsRepository::get_transcript_api_key(&pool, "whisperX")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn an_unset_transcript_key_also_reads_back_as_none() {
        // Same fixed decoding as the summary side; kept as its own test
        // because the two functions read different tables. It bit harder
        // here: choosing a local provider was enough to create the row,
        // after which every paid transcription provider reported a key it
        // did not have.
        let pool = pool().await;
        assert_eq!(
            SettingsRepository::get_transcript_api_key(&pool, "deepgram")
                .await
                .expect("query runs"),
            None
        );

        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .unwrap();

        assert_eq!(
            SettingsRepository::get_transcript_api_key(&pool, "deepgram")
                .await
                .expect("query runs"),
            None,
            "an unset key on an existing row still reads as unset"
        );
    }

    // ===== OBSIDIAN EXPORT SETTINGS =====

    #[tokio::test]
    async fn obsidian_export_is_unconfigured_when_there_is_no_settings_row() {
        let pool = pool().await;
        assert_eq!(
            SettingsRepository::get_obsidian_settings(&pool)
                .await
                .expect("query runs"),
            ObsidianConfig::default()
        );
    }

    #[tokio::test]
    async fn obsidian_paths_and_the_auto_export_flag_round_trip() {
        let pool = pool().await;
        let config = ObsidianConfig {
            vault_path: Some("/Users/ktos/Vault".to_string()),
            transcript_path: Some("/Volumes/Local/Transkrypty".to_string()),
            summary_path: Some("/Users/ktos/Vault/Podsumowania".to_string()),
            auto_export: true,
        };

        SettingsRepository::save_obsidian_settings(&pool, &config)
            .await
            .expect("save succeeds");

        assert_eq!(
            SettingsRepository::get_obsidian_settings(&pool)
                .await
                .expect("query runs"),
            config
        );

        // Turning the feature off again has to clear, not merge.
        SettingsRepository::save_obsidian_settings(&pool, &ObsidianConfig::default())
            .await
            .expect("save succeeds");
        assert_eq!(
            SettingsRepository::get_obsidian_settings(&pool)
                .await
                .expect("query runs"),
            ObsidianConfig::default()
        );
    }

    #[tokio::test]
    async fn blank_obsidian_paths_read_back_as_unset() {
        // A cleared text field arrives as "" or as whitespace, and callers must
        // not have to tell that apart from "never configured" — an empty path
        // would otherwise be treated as the filesystem root.
        let pool = pool().await;
        SettingsRepository::save_obsidian_settings(
            &pool,
            &ObsidianConfig {
                vault_path: Some("   ".to_string()),
                transcript_path: Some(String::new()),
                summary_path: Some("\t\n".to_string()),
                auto_export: false,
            },
        )
        .await
        .unwrap();

        let stored = SettingsRepository::get_obsidian_settings(&pool)
            .await
            .expect("query runs");
        assert_eq!(stored.vault_path, None);
        assert_eq!(stored.transcript_path, None);
        assert_eq!(stored.summary_path, None);
    }

    #[tokio::test]
    async fn saving_export_settings_leaves_the_model_configuration_alone() {
        let pool = pool().await;
        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "medium", None)
            .await
            .unwrap();

        SettingsRepository::save_obsidian_settings(
            &pool,
            &ObsidianConfig {
                vault_path: Some("/Users/ktos/Vault".to_string()),
                auto_export: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(setting.provider, "ollama");
        assert_eq!(setting.model, "llama3.1:8b");
        assert_eq!(setting.whisper_model, "medium");
    }

    // ===== MEETING DETECTION SETTINGS =====

    #[tokio::test]
    async fn detection_settings_fall_back_to_defaults_when_nothing_is_stored() {
        let pool = pool().await;
        assert_eq!(
            SettingsRepository::get_detection_settings(&pool)
                .await
                .expect("query runs"),
            DetectionConfig::default(),
            "no settings row at all"
        );

        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "large-v3", None)
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_detection_settings(&pool)
                .await
                .expect("query runs"),
            DetectionConfig::default(),
            "a row exists but the column is NULL"
        );
    }

    #[tokio::test]
    async fn detection_settings_round_trip_through_the_json_blob() {
        let pool = pool().await;
        let config = DetectionConfig {
            enabled: true,
            ignored_bundle_ids: vec!["com.obsproject.obs-studio".to_string()],
            always_meeting_bundle_ids: vec!["us.zoom.xos".to_string()],
            min_duration_seconds: 45,
            show_notifications: false,
            auto_stop_enabled: false,
            silence_duration_seconds: 300,
            confirmation_timeout_seconds: 60,
            max_recording_minutes: 90,
        };

        SettingsRepository::save_detection_settings(&pool, &config)
            .await
            .expect("save succeeds");

        assert_eq!(
            SettingsRepository::get_detection_settings(&pool)
                .await
                .expect("query runs"),
            config
        );
    }

    #[tokio::test]
    async fn unreadable_detection_json_degrades_to_defaults_instead_of_failing() {
        // The blob format is expected to change as the allow lists are tuned,
        // and startup reads this: a shape the current build cannot parse must
        // not be able to take the application down with it.
        let pool = pool().await;
        SettingsRepository::save_detection_settings(&pool, &DetectionConfig::default())
            .await
            .unwrap();

        for stored in ["{ to nie jest json", "", "   "] {
            sqlx::query("UPDATE settings SET meetingDetectionConfig = ? WHERE id = '1'")
                .bind(stored)
                .execute(&pool)
                .await
                .unwrap();
            assert_eq!(
                SettingsRepository::get_detection_settings(&pool)
                    .await
                    .expect("query runs"),
                DetectionConfig::default(),
                "stored value was: {stored:?}"
            );
        }
    }

    #[tokio::test]
    async fn detection_settings_written_before_auto_stop_existed_still_load() {
        // The four auto-stop fields carry serde defaults precisely so that a
        // blob saved by an older build keeps parsing; this is what that
        // promise looks like from the repository's side.
        let pool = pool().await;
        SettingsRepository::save_detection_settings(&pool, &DetectionConfig::default())
            .await
            .unwrap();
        sqlx::query("UPDATE settings SET meetingDetectionConfig = ? WHERE id = '1'")
            .bind(
                r#"{"enabled":true,"ignoredBundleIds":[],"alwaysMeetingBundleIds":["us.zoom.xos"],
                    "minDurationSeconds":30,"showNotifications":false}"#,
            )
            .execute(&pool)
            .await
            .unwrap();

        let config = SettingsRepository::get_detection_settings(&pool)
            .await
            .expect("query runs");
        assert!(config.enabled);
        assert_eq!(config.min_duration_seconds, 30);
        assert!(!config.show_notifications);
        // Everything the old blob never knew about comes from the defaults.
        let defaults = DetectionConfig::default();
        assert_eq!(config.auto_stop_enabled, defaults.auto_stop_enabled);
        assert_eq!(
            config.silence_duration_seconds,
            defaults.silence_duration_seconds
        );
        assert_eq!(
            config.confirmation_timeout_seconds,
            defaults.confirmation_timeout_seconds
        );
        assert_eq!(config.max_recording_minutes, defaults.max_recording_minutes);
    }

    #[tokio::test]
    async fn saving_detection_settings_leaves_the_model_configuration_alone() {
        let pool = pool().await;
        SettingsRepository::save_model_config(&pool, "ollama", "llama3.1:8b", "medium", None)
            .await
            .unwrap();

        SettingsRepository::save_detection_settings(
            &pool,
            &DetectionConfig {
                enabled: true,
                ..DetectionConfig::default()
            },
        )
        .await
        .unwrap();

        let setting = SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(setting.provider, "ollama");
        assert_eq!(setting.model, "llama3.1:8b");
        assert!(
            SettingsRepository::get_detection_settings(&pool)
                .await
                .unwrap()
                .enabled
        );
    }
}
