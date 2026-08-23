use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;
use log::{info, warn, error};
use anyhow::Result;

use crate::state::AppState;
use crate::database::repositories::setting::SettingsRepository;
use crate::language::{choose_engine, summary_language_group, SpeechEngine};


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OnboardingStatus {
    pub version: String,
    pub completed: bool,
    pub current_step: u8,
    pub model_status: ModelStatus,
    /// The languages the user said their meetings are in, as ISO 639-1 codes.
    ///
    /// Empty means they declined to narrow it down or skipped the question,
    /// which is a real answer rather than missing data — see `choose_engine`.
    /// Defaulted for installs that completed onboarding before the question
    /// existed; those users are asked again from Settings rather than guessed at.
    #[serde(default)]
    pub meeting_languages: Vec<String>,
    pub last_updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModelStatus {
    /// Whether the transcription model is on disk. The stored key is still
    /// "parakeet" because installed copies carry it, but the engine behind it
    /// is no longer always Parakeet.
    #[serde(rename = "parakeet")]
    pub transcription: String,  // "downloaded" | "not_downloaded" | "downloading"
    pub summary: String,   // Generic field for summary model (Qwen 3.5 or legacy Gemma variants)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_summary_model: Option<String>,
}

impl Default for OnboardingStatus {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            completed: false,
            current_step: 1,
            model_status: ModelStatus {
                transcription: "not_downloaded".to_string(),
                summary: "not_downloaded".to_string(),  // Changed from gemma
                selected_summary_model: None,
            },
            meeting_languages: Vec::new(),
            last_updated: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// What a given answer costs and installs, so that the setup step can say so
/// before any download starts.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct OnboardingPlan {
    pub engine: SpeechEngine,
    pub transcription_model: String,
    pub transcription_size_mb: u32,
    pub summary_model: String,
    pub summary_size_mb: u64,
}

/// Work out what to install for these meeting languages on this machine.
///
/// Pure on purpose: the whole point of the language question is this decision,
/// and it should be testable without a running app, a download, or an
/// `AppHandle`.
pub fn plan_for_languages(languages: &[String], system_ram_gb: u64) -> OnboardingPlan {
    let engine = choose_engine(languages);
    let summary_model = crate::summary::summary_engine::commands::recommend_summary_model(
        system_ram_gb,
        summary_language_group(languages),
    );
    let summary_size_mb = crate::summary::summary_engine::models::get_model_by_name(&summary_model)
        .map(|model| model.size_mb)
        .unwrap_or(0);

    OnboardingPlan {
        engine,
        transcription_model: engine.default_model().to_string(),
        transcription_size_mb: engine.download_size_mb(),
        summary_model,
        summary_size_mb,
    }
}


/// Load onboarding status from store
pub async fn load_onboarding_status<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<OnboardingStatus> {
    // Try to load from Tauri store
    let store = match app.store("onboarding-status.json") {
        Ok(store) => store,
        Err(e) => {
            warn!("Failed to access onboarding store: {}, using defaults", e);
            return Ok(OnboardingStatus::default());
        }
    };

    // Try to get the status from store
    let status = if let Some(value) = store.get("status") {
        match serde_json::from_value::<OnboardingStatus>(value.clone()) {
            Ok(s) => {
                info!("Loaded onboarding status from store - Step: {}, Completed: {}",
                      s.current_step, s.completed);
                s
            }
            Err(e) => {
                warn!("Failed to deserialize onboarding status: {}, using defaults", e);
                OnboardingStatus::default()
            }
        }
    } else {
        info!("No stored onboarding status found, using defaults");
        OnboardingStatus::default()
    };

    Ok(status)
}

/// Save onboarding status to store
pub async fn save_onboarding_status<R: Runtime>(
    app: &AppHandle<R>,
    status: &OnboardingStatus,
) -> Result<()> {
    info!("Saving onboarding status: step={}, completed={}",
          status.current_step, status.completed);

    // Get or create store
    let store = app.store("onboarding-status.json")
        .map_err(|e| anyhow::anyhow!("Failed to access onboarding store: {}", e))?;

    // Update last_updated timestamp
    let mut status = status.clone();
    status.last_updated = chrono::Utc::now().to_rfc3339();

    // Serialize status to JSON value
    let status_value = serde_json::to_value(&status)
        .map_err(|e| anyhow::anyhow!("Failed to serialize onboarding status: {}", e))?;

    // Save to store
    store.set("status", status_value);

    // Persist to disk
    store.save()
        .map_err(|e| anyhow::anyhow!("Failed to save onboarding store to disk: {}", e))?;

    info!("Successfully persisted onboarding status to disk");
    Ok(())
}

/// Tauri commands for onboarding status
#[tauri::command]
pub async fn get_onboarding_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<OnboardingStatus>, String> {
    let status = load_onboarding_status(&app)
        .await
        .map_err(|e| format!("Failed to load onboarding status: {}", e))?;

    // Return None if it's the default (never saved before)
    // Check if we have any saved data by seeing if the store has the key
    let store = app.store("onboarding-status.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;

    if store.get("status").is_none() {
        Ok(None)
    } else {
        Ok(Some(status))
    }
}

#[tauri::command]
pub async fn save_onboarding_status_cmd<R: Runtime>(
    app: AppHandle<R>,
    status: OnboardingStatus,
) -> Result<(), String> {
    save_onboarding_status(&app, &status)
        .await
        .map_err(|e| format!("Failed to save onboarding status: {}", e))
}

/// The last step of the flow, which is where the language answer turns into
/// stored configuration.
#[tauri::command]
pub async fn onboarding_plan_for_languages(
    languages: Vec<String>,
) -> Result<OnboardingPlan, String> {
    let system_ram_gb = crate::summary::summary_engine::commands::get_system_ram_gb()?;
    Ok(plan_for_languages(&languages, system_ram_gb))
}

#[tauri::command]
pub async fn complete_onboarding<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    model: String,
    languages: Vec<String>,
) -> Result<(), String> {
    let engine = choose_engine(&languages);
    info!(
        "Completing onboarding with builtin-ai model: {}, meeting languages: {:?}, transcription engine: {:?}",
        model, languages, engine
    );

    // Step 1: Save model configuration to SQLite database FIRST
    let pool = state.db_manager.pool();

    // Onboarding always uses builtin-ai (local LLM)
    if let Err(e) = SettingsRepository::save_model_config(
        pool,
        "builtin-ai",
        &model,
        "large-v3",
        None,
    ).await {
        error!("Failed to save builtin-ai model config: {}", e);
        return Err(format!("Failed to save builtin-ai model config: {}", e));
    }
    info!("Saved builtin-ai model config: model={}", model);

    // Save the transcription engine the answer calls for. This used to be
    // Parakeet unconditionally, which silently handed everyone outside its 25
    // European languages an engine that could not transcribe them.
    let transcription_model = engine.default_model();
    if let Err(e) = SettingsRepository::save_transcript_config(
        pool,
        engine.provider(),
        transcription_model,
    ).await {
        error!("Failed to save transcription model config: {}", e);
        return Err(format!("Failed to save transcription model config: {}", e));
    }
    info!(
        "Saved transcription model config: provider={}, model={}",
        engine.provider(),
        transcription_model
    );

    // Step 2: Only NOW mark onboarding as complete (after DB operations succeed)
    let mut status = load_onboarding_status(&app)
        .await
        .map_err(|e| format!("Failed to load onboarding status: {}", e))?;

    status.completed = true;
    status.current_step = 6; // Max step (6 on macOS with permissions, 5 on other platforms)
    status.model_status.transcription = "downloaded".to_string();
    status.model_status.summary = "downloaded".to_string();
    status.model_status.selected_summary_model = Some(model.clone());
    status.meeting_languages = languages;

    save_onboarding_status(&app, &status)
        .await
        .map_err(|e| format!("Failed to save completed onboarding status: {}", e))?;

    info!("Onboarding completed successfully with model: {}", model);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_status_deserializes_without_selected_summary_model() {
        let status: OnboardingStatus = serde_json::from_str(
            r#"{
                "version": "1.0",
                "completed": true,
                "current_step": 4,
                "model_status": {
                    "parakeet": "downloaded",
                    "summary": "downloaded"
                },
                "last_updated": "2026-05-30T00:00:00Z"
            }"#,
        )
        .expect("old onboarding status should remain compatible");

        assert_eq!(status.model_status.selected_summary_model, None);
    }

    #[test]
    fn a_status_saved_before_the_language_question_still_loads() {
        // Anyone who finished onboarding before this step existed has no answer
        // stored. That must read as "did not say" rather than failing to parse
        // and resetting them to the start of the flow.
        let status: OnboardingStatus = serde_json::from_str(
            r#"{
                "version": "1.0",
                "completed": true,
                "current_step": 4,
                "model_status": {
                    "parakeet": "downloaded",
                    "summary": "downloaded"
                },
                "last_updated": "2026-05-30T00:00:00Z"
            }"#,
        )
        .expect("a status without meeting_languages should remain compatible");

        assert!(status.meeting_languages.is_empty());
    }

    #[test]
    fn the_transcription_status_still_reads_and_writes_the_parakeet_key() {
        // The field was renamed for honesty, not the storage format; installed
        // copies carry the old key and must keep working in both directions.
        let status: OnboardingStatus = serde_json::from_str(
            r#"{
                "version": "1.0",
                "completed": false,
                "current_step": 3,
                "model_status": {
                    "parakeet": "downloading",
                    "summary": "not_downloaded"
                },
                "meeting_languages": ["ja"],
                "last_updated": "2026-05-30T00:00:00Z"
            }"#,
        )
        .expect("status should parse");

        assert_eq!(status.model_status.transcription, "downloading");
        assert_eq!(status.meeting_languages, vec!["ja".to_string()]);

        let round_tripped = serde_json::to_value(&status).expect("status should serialize");
        assert_eq!(round_tripped["model_status"]["parakeet"], "downloading");
    }

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_european_answer_plans_a_parakeet_install() {
        let plan = plan_for_languages(&codes(&["pl"]), 16);

        assert_eq!(plan.engine, SpeechEngine::Parakeet);
        assert_eq!(plan.transcription_model, crate::config::DEFAULT_PARAKEET_MODEL);
        assert!(plan.transcription_size_mb > 0);
    }

    #[test]
    fn a_japanese_answer_plans_a_whisper_install() {
        // The case the whole step exists for.
        let plan = plan_for_languages(&codes(&["ja"]), 16);

        assert_eq!(plan.engine, SpeechEngine::Whisper);
        assert_eq!(plan.transcription_model, "large-v3-turbo-q5_0");
    }

    #[test]
    fn the_plan_always_names_a_real_summary_model_with_a_real_size() {
        for languages in [vec![], codes(&["en"]), codes(&["ja"]), codes(&["en", "ar"])] {
            for ram in [0, 8, 14, 64] {
                let plan = plan_for_languages(&languages, ram);
                assert!(
                    crate::summary::summary_engine::models::get_model_by_name(&plan.summary_model)
                        .is_some(),
                    "{} is not a real model",
                    plan.summary_model
                );
                assert!(
                    plan.summary_size_mb > 0,
                    "a plan that cannot state its download size cannot be shown to the user"
                );
            }
        }
    }

    #[test]
    fn ram_moves_the_summary_model_without_moving_the_engine() {
        let small = plan_for_languages(&codes(&["de"]), 8);
        let large = plan_for_languages(&codes(&["de"]), 32);

        assert_eq!(small.engine, large.engine);
        assert_ne!(small.summary_model, large.summary_model);
    }
}
