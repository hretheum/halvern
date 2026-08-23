// Tauri commands for built-in AI model management
// Exposes model download, status, and management functionality to frontend

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::Mutex;

use super::model_manager::{DownloadProgress, ModelInfo, ModelManager};

use crate::language::LanguageGroup;

const QWEN35_4B_RECOMMENDED_RAM_GB: u64 = 14;

pub(crate) fn summary_model_priority(model_name: &str) -> u8 {
    match model_name {
        "qwen3.5:4b" => 4,
        "qwen3.5:2b" => 3,
        "gemma3:4b" => 2,
        "gemma3:1b" => 1,
        _ => 0,
    }
}

/// System RAM floor for each built-in summary model, in whole gigabytes.
///
/// This is a ceiling on the choice rather than a hint: a model under its floor
/// is removed from the running before anything is ranked. The 14 GB figure for
/// the 4B-class models is the threshold this code already shipped with. The
/// small models carry a floor of zero on purpose — no measurement supports a
/// higher number, and inventing one would strand low-RAM machines with a worse
/// recommendation than they get today.
///
/// `None` means the model is unknown to this table, which excludes it. That is
/// the strict direction, and `every_shipped_model_declares_a_ram_floor` keeps
/// it from quietly hiding a newly added model.
fn summary_model_min_ram_gb(model_name: &str) -> Option<u64> {
    match model_name {
        "qwen3.5:4b" => Some(QWEN35_4B_RECOMMENDED_RAM_GB),
        "gemma3:4b" => Some(QWEN35_4B_RECOMMENDED_RAM_GB),
        "qwen3.5:2b" => Some(0),
        "gemma3:1b" => Some(0),
        _ => None,
    }
}

/// How well a model is expected to summarise a given family of languages.
///
/// Measured on 19 August 2026
/// (`docs/experiments/summary-model-bakeoff/results/REPORT.md`): five languages
/// covering every group, four models, matched and shipped sampling. Qwen led
/// recall in every group in both tiers, so the inherited order survived almost
/// everywhere.
///
/// Exactly one cell moved, and it is below.
///
/// Do not read the rest as settled. One transcript per language, one repeat, no
/// long transcripts and no real meetings — a direction rather than a
/// measurement, and the report lists what a decisive re-run would need. In
/// particular the small tier is re-opened by our own fix: `gemma3:1b` lost on
/// output-hygiene defects that `clean_llm_markdown_output` now handles.
///
/// An unmeasured group must not drift on somebody's intuition. Add a cell here
/// only with a run behind it.
fn language_score(model_name: &str, group: LanguageGroup) -> u8 {
    match (model_name, group) {
        // The one cell the bake-off filled. qwen3.5:2b summarises Japanese into
        // Chinese — the pipeline's first pass demands English, and it produces
        // neither — reproducibly, across three byte-identical runs under greedy
        // decoding. Everything downstream is built on that intermediate, so the
        // failure is total rather than a matter of quality.
        //
        // Scoring it below gemma3:1b makes the small tier's CJK default the
        // other model, which got the intermediate right in the same test. That
        // is a choice between two weak options, not an endorsement: the report
        // says plainly that neither small model is fit for CJK.
        ("qwen3.5:2b", LanguageGroup::Cjk) => 0,
        _ => summary_model_priority(model_name),
    }
}

/// Pick the built-in summary model for this machine and these meetings.
///
/// RAM filters and language ranks: the two constraints are different in kind
/// and blending them into one score would let a good language fit talk a model
/// onto a machine that cannot hold it. Ties break towards the smaller download,
/// so that when quality is equal the user waits for less.
///
/// If nothing clears its floor the smallest model is returned anyway. A machine
/// too small for every option still needs an answer, and summaries that are
/// slow beat a first run that recommends nothing at all.
pub(crate) fn recommend_summary_model(system_ram_gb: u64, group: LanguageGroup) -> String {
    let models = super::models::get_available_models();

    let best_fitting = models
        .iter()
        .filter(|model| {
            summary_model_min_ram_gb(&model.name).is_some_and(|floor| system_ram_gb >= floor)
        })
        .max_by(|a, b| {
            language_score(&a.name, group)
                .cmp(&language_score(&b.name, group))
                .then(b.size_mb.cmp(&a.size_mb))
        });

    let chosen = best_fitting.or_else(|| models.iter().min_by_key(|model| model.size_mb));

    chosen
        .map(|model| model.name.clone())
        .unwrap_or_else(|| "qwen3.5:2b".to_string())
}

pub(crate) fn get_recommended_summary_model_for_current_system() -> Result<String, String> {
    let system_ram_gb = get_system_ram_gb()?;

    log::info!("System RAM detected: {} GB", system_ram_gb);

    // No meeting language is known at this call site: it serves fresh-install
    // defaults and the pre-answer state of onboarding. `Other` is the group
    // that assumes least, and the caller re-asks once the user has answered.
    Ok(recommend_summary_model(system_ram_gb, LanguageGroup::Other))
}

// ============================================================================
// Global State
// ============================================================================

/// Global model manager instance
pub struct ModelManagerState(pub Arc<Mutex<Option<Arc<ModelManager>>>>);

/// Initialize the model manager
pub async fn init_model_manager<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<()> {
    let models_dir = app.path().app_data_dir()?.join("models").join("summary");

    let manager = ModelManager::new_with_models_dir(Some(models_dir))?;
    manager.init().await?;

    let state: State<ModelManagerState> = app.state();
    let mut manager_lock = state.0.lock().await;
    *manager_lock = Some(Arc::new(manager));

    log::info!("Built-in AI model manager initialized");
    Ok(())
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// List all available built-in AI models with their status
#[tauri::command]
pub async fn builtin_ai_list_models<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
) -> Result<Vec<ModelInfo>, String> {
    let manager = {
        // Ensure manager is initialized
        {
            let manager_lock = state.0.lock().await;
            if manager_lock.is_none() {
                drop(manager_lock);
                init_model_manager(&app)
                    .await
                    .map_err(|e| format!("Failed to initialize model manager: {}", e))?;
            }
        }

        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone()
    };

    let models = manager.list_models().await;
    Ok(models)
}

/// Get information about a specific model
#[tauri::command]
pub async fn builtin_ai_get_model_info<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<Option<ModelInfo>, String> {
    let manager = {
        // Ensure manager is initialized
        {
            let manager_lock = state.0.lock().await;
            if manager_lock.is_none() {
                drop(manager_lock);
                init_model_manager(&app)
                    .await
                    .map_err(|e| format!("Failed to initialize model manager: {}", e))?;
            }
        }

        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone()
    };

    let info = manager.get_model_info(&model_name).await;
    Ok(info)
}

/// Download a built-in AI model with progress updates
#[tauri::command]
pub async fn builtin_ai_download_model<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    let manager = {
        // Ensure manager is initialized
        {
            let manager_lock = state.0.lock().await;
            if manager_lock.is_none() {
                drop(manager_lock);
                init_model_manager(&app)
                    .await
                    .map_err(|e| format!("Failed to initialize model manager: {}", e))?;
            }
        }

        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone() // Clone the Arc, not the ModelManager
    };
    // IMPORTANT: Only emit "downloading" status here, never "completed"
    // Completion event is emitted AFTER download task fully finishes (validation, etc.)
    let app_clone = app.clone();
    let model_name_clone = model_name.clone();
    let progress_callback = Box::new(move |progress: DownloadProgress| {
        let _ = app_clone.emit(
            "builtin-ai-download-progress",
            serde_json::json!({
                "model": model_name_clone,
                "progress": progress.percent,
                "downloaded_mb": progress.downloaded_mb,
                "total_mb": progress.total_mb,
                "speed_mbps": progress.speed_mbps,
                "status": "downloading"  // Always "downloading", never "completed" from progress callback
            }),
        );
    });

    match manager
        .download_model_detailed(&model_name, Some(progress_callback))
        .await
    {
        Ok(_) => {
            // Download task completed successfully (validation passed, status set to Available)
            let _ = app.emit(
                "builtin-ai-download-progress",
                serde_json::json!({
                    "model": model_name,
                    "progress": 100,
                    "downloaded_mb": 0,  // Not used by completion handler
                    "total_mb": 0,       // Not used by completion handler
                    "speed_mbps": 0,     // Not used by completion handler
                    "status": "completed"
                }),
            );
            Ok(())
        },
        Err(e) => {
            let error_msg = e.to_string();

            // Check if this is a cancellation error (marked with "CANCELLED:" prefix)
            // Don't emit error event for cancellations - cancel command already emits cancelled event
            if !error_msg.starts_with("CANCELLED:") {
                // Emit error via progress event for frontend to display (only for real errors)
                let _ = app.emit(
                    "builtin-ai-download-progress",
                    serde_json::json!({
                        "model": model_name,
                        "progress": 0,
                        "downloaded_mb": 0,
                        "total_mb": 0,
                        "speed_mbps": 0,
                        "status": "error",
                        "error": error_msg
                    }),
                );
            }
            Err(error_msg)
        }
    }
}

/// Cancel an ongoing model download
#[tauri::command]
pub async fn builtin_ai_cancel_download<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    let manager = {
        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone()
    };

    manager
        .cancel_download(&model_name)
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "builtin-ai-download-progress",
        serde_json::json!({
            "model": model_name,
            "progress": 0,
            "status": "cancelled"
        }),
    );

    Ok(())
}

/// Delete a corrupted or available model file
#[tauri::command]
pub async fn builtin_ai_delete_model(
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    let manager = {
        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone()
    };

    manager
        .delete_model(&model_name)
        .await
        .map_err(|e| e.to_string())
}

/// Check if a model is ready to use
#[tauri::command]
pub async fn builtin_ai_is_model_ready<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
    refresh: Option<bool>,  // NEW: Optional refresh parameter
) -> Result<bool, String> {
    let manager = {
        // Ensure manager is initialized
        {
            let manager_lock = state.0.lock().await;
            if manager_lock.is_none() {
                drop(manager_lock);
                init_model_manager(&app)
                    .await
                    .map_err(|e| format!("Failed to initialize model manager: {}", e))?;
            }
        }

        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone()
    };

    let refresh_scan = refresh.unwrap_or(false);
    let ready = manager.is_model_ready(&model_name, refresh_scan).await;

    log::info!(
        "Model '{}' ready check (refresh={}): {}",
        model_name,
        refresh_scan,
        ready
    );

    Ok(ready)
}

/// Check if any summary model is available (for onboarding)
/// Returns the first available model name by priority, or None if no models exist
#[tauri::command]
pub async fn builtin_ai_get_available_summary_model<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
) -> Result<Option<String>, String> {
    let manager = {
        // Ensure manager is initialized
        {
            let manager_lock = state.0.lock().await;
            if manager_lock.is_none() {
                drop(manager_lock);
                init_model_manager(&app)
                    .await
                    .map_err(|e| format!("Failed to initialize model manager: {}", e))?;
            }
        }

        let manager_lock = state.0.lock().await;
        manager_lock
            .as_ref()
            .ok_or_else(|| "Model manager not initialized".to_string())?
            .clone()
    };

    // Force fresh scan to ensure accurate state
    manager
        .scan_models()
        .await
        .map_err(|e| format!("Failed to scan models: {}", e))?;

    // Get all available models
    let all_models = manager.list_models().await;

    // Find first available summary model
    let available = all_models
        .iter()
        .filter(|m| matches!(m.status, crate::summary::summary_engine::model_manager::ModelStatus::Available))
        .max_by_key(|m| summary_model_priority(&m.name))
        .map(|m| m.name.clone());

    log::info!("Available summary model check: {:?}", available);
    Ok(available)
}

// ============================================================================
// Startup Initialization & Utility Commands
// ============================================================================

pub async fn init_model_manager_at_startup<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models")
        .join("summary");

    let manager = ModelManager::new_with_models_dir(Some(models_dir))
        .map_err(|e| format!("Failed to create ModelManager: {}", e))?;

    manager
        .init()
        .await
        .map_err(|e| format!("Failed to initialize ModelManager: {}", e))?;

    let state: State<ModelManagerState> = app.state();
    let mut manager_lock = state.0.lock().await;
    *manager_lock = Some(Arc::new(manager));

    log::info!("ModelManager initialized at startup");
    Ok(())
}


/// Get recommended summary model based on platform and system RAM.
/// macOS → qwen3.5:4b
/// non-macOS + <8GB RAM → qwen3.5:2b
/// non-macOS + >=8GB RAM → qwen3.5:4b
#[tauri::command]
pub async fn builtin_ai_get_recommended_model() -> Result<String, String> {
    let recommended = get_recommended_summary_model_for_current_system()?;

    log::info!("Recommended summary model: {}", recommended);
    Ok(recommended.to_string())
}

/// One built-in model, as onboarding needs to show it.
///
/// The interface asks the user to choose, so it has to give them something to
/// choose on. The shipped display names — "High Quality", "Balanced", "Fast" —
/// are inherited marketing and decide nothing; `note` carries what is actually
/// known, and `blocker` says plainly when a model cannot be used at all.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct SummaryModelOption {
    pub name: String,
    pub display_name: String,
    pub size_mb: u64,
    /// Whole gigabytes of system RAM this model needs. Zero means no floor.
    pub min_ram_gb: u64,
    /// False when the machine is under `min_ram_gb`.
    pub fits_ram: bool,
    /// True for the one option the app suggests by default.
    pub is_default: bool,
    /// Whether to show this under "recommended" or below the line.
    pub recommended: bool,
    /// One honest sentence. Empty when there is nothing specific to say.
    pub note: String,
    /// Why this model is not recommended, if it is not. Empty otherwise.
    pub blocker: String,
}

/// Everything the model-choice step renders, in one call.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct SummaryModelChoices {
    pub system_ram_gb: u64,
    pub options: Vec<SummaryModelOption>,
}

/// What is known about a model for a given family of languages.
///
/// Sourced from the bake-off of 19 August 2026
/// (`docs/experiments/summary-model-bakeoff/results/REPORT.md`), and phrased for
/// somebody deciding rather than for somebody reading a benchmark. Where nothing
/// specific was measured, this says nothing rather than inventing reassurance.
fn model_note(model_name: &str, group: LanguageGroup) -> &'static str {
    match (model_name, group) {
        // Measured: writes its intermediate summary in Chinese for Japanese
        // input, every time, which corrupts everything downstream. Reproduced
        // across three identical runs under greedy decoding.
        ("qwen3.5:2b", LanguageGroup::Cjk) => {
            "Not reliable for Chinese, Japanese or Korean — it summarises into the wrong language."
        }
        ("qwen3.5:4b", _) => "Most accurate in testing, and the slowest.",
        ("gemma3:4b", _) => "Close to the 4B Qwen, a little weaker at picking up details.",
        ("qwen3.5:2b", _) => "Good balance of speed and accuracy on a modest machine.",
        ("gemma3:1b", _) => "Fastest and smallest. Misses more, so expect a thinner summary.",
        _ => "",
    }
}

/// The model choices for this machine and these meetings.
///
/// Pure so the rules are testable without a running app: which models fit, which
/// are worth suggesting, and which are listed only so the user can see they
/// exist and why they are not offered.
pub(crate) fn summary_model_choices_for(
    system_ram_gb: u64,
    group: LanguageGroup,
    models: &[super::models::ModelDef],
) -> SummaryModelChoices {
    let default = recommend_summary_model(system_ram_gb, group);

    let options = models
        .iter()
        .map(|m| {
            let min_ram_gb = summary_model_min_ram_gb(&m.name).unwrap_or(0);
            let fits_ram = system_ram_gb >= min_ram_gb;
            let note = model_note(&m.name, group);

            // Two reasons to put a model below the line, and the user deserves
            // to know which: it will not run on this machine, or it will run
            // and do the job badly in their language.
            let blocker = if !fits_ram {
                format!("Needs {min_ram_gb} GB of memory; this Mac has {system_ram_gb} GB.")
            } else if note.starts_with("Not reliable") {
                note.to_string()
            } else {
                String::new()
            };

            SummaryModelOption {
                name: m.name.clone(),
                display_name: m.display_name.clone(),
                size_mb: m.size_mb,
                min_ram_gb,
                fits_ram,
                is_default: m.name == default,
                recommended: blocker.is_empty(),
                note: note.to_string(),
                blocker,
            }
        })
        .collect();

    SummaryModelChoices {
        system_ram_gb,
        options,
    }
}

/// Model choices for the onboarding step, for the languages the user picked.
#[tauri::command]
pub fn summary_model_choices(languages: Vec<String>) -> Result<SummaryModelChoices, String> {
    let group = crate::language::summary_language_group(&languages);
    let ram = get_system_ram_gb()?;
    Ok(summary_model_choices_for(
        ram,
        group,
        &super::models::get_available_models(),
    ))
}

/// Get total system RAM in gigabytes
pub(crate) fn get_system_ram_gb() -> Result<u64, String> {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_memory();

    let total_memory_bytes = sys.total_memory();
    let total_memory_gb = total_memory_bytes / (1024 * 1024 * 1024);

    Ok(total_memory_gb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_summary_model_uses_qwen2b_below_effective_16gb_floor() {
        // Cjk excepted: the bake-off measured qwen3.5:2b summarising Japanese
        // into Chinese, so below the floor that group gets the other small
        // model instead. See `language_score`.
        for group in ALL_GROUPS {
            let expected = if group == LanguageGroup::Cjk { "gemma3:1b" } else { "qwen3.5:2b" };
            assert_eq!(recommend_summary_model(13, group), expected, "group={group:?}");
        }
    }

    #[test]
    fn recommended_summary_model_uses_qwen4b_at_effective_16gb_floor() {
        for group in ALL_GROUPS {
            assert_eq!(recommend_summary_model(14, group), "qwen3.5:4b");
        }
    }

    #[test]
    fn available_summary_model_priority_prefers_qwen_over_gemma() {
        assert!(summary_model_priority("qwen3.5:4b") > summary_model_priority("qwen3.5:2b"));
        assert!(summary_model_priority("qwen3.5:2b") > summary_model_priority("gemma3:4b"));
        assert!(summary_model_priority("gemma3:4b") > summary_model_priority("gemma3:1b"));
    }

    const ALL_GROUPS: [LanguageGroup; 5] = [
        LanguageGroup::English,
        LanguageGroup::WesternEuropean,
        LanguageGroup::Slavic,
        LanguageGroup::Cjk,
        LanguageGroup::Other,
    ];

    #[test]
    fn every_shipped_model_declares_a_ram_floor() {
        // Without this, adding a model to models.rs would silently make it
        // unrecommendable rather than loudly wrong.
        for model in super::super::models::get_available_models() {
            assert!(
                summary_model_min_ram_gb(&model.name).is_some(),
                "{} has no RAM floor, so it can never be recommended",
                model.name
            );
        }
    }

    #[test]
    fn the_ram_ceiling_excludes_models_that_do_not_fit() {
        // Both 4B-class models are out below the floor, so the winner has to
        // come from the two that remain.
        let chosen = recommend_summary_model(8, LanguageGroup::English);
        assert!(
            chosen == "qwen3.5:2b" || chosen == "gemma3:1b",
            "expected a small model on 8 GB, got {chosen}"
        );
    }

    #[test]
    fn a_machine_below_every_floor_still_gets_an_answer() {
        // Every model currently sits at or above a zero floor, so this asserts
        // the contract rather than today's arithmetic: no amount of RAM, not
        // even none, may produce an empty recommendation.
        let chosen = recommend_summary_model(0, LanguageGroup::Other);
        assert!(
            super::super::models::get_model_by_name(&chosen).is_some(),
            "{chosen} is not a real model"
        );
    }

    #[test]
    fn the_recommendation_is_always_a_model_that_exists() {
        for ram in [0, 1, 4, 8, 13, 14, 16, 32, 128] {
            for group in ALL_GROUPS {
                let chosen = recommend_summary_model(ram, group);
                assert!(
                    super::super::models::get_model_by_name(&chosen).is_some(),
                    "{chosen} at {ram} GB is not a real model"
                );
            }
        }
    }

    #[test]
    fn language_moves_the_choice_only_where_it_was_measured() {
        // Replaces `language_does_not_move_the_choice_until_the_bake_off_lands`,
        // which guarded the interim state and was meant to fail once results
        // arrived. They arrived on 19 August 2026
        // (docs/experiments/summary-model-bakeoff/results/REPORT.md).
        //
        // Exactly one cell moved, and only below the RAM floor: small-tier CJK.
        // Everywhere else the measured order matched the inherited one, and an
        // unmeasured group must not drift on someone's intuition.
        for ram in [8, 14, 32] {
            let english = recommend_summary_model(ram, LanguageGroup::English);
            for group in ALL_GROUPS {
                let got = recommend_summary_model(ram, group);
                if group == LanguageGroup::Cjk && ram < QWEN35_4B_RECOMMENDED_RAM_GB {
                    assert_eq!(got, "gemma3:1b", "ram={ram}");
                } else {
                    assert_eq!(got, english, "ram={ram} group={group:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod model_choice_tests {
    use super::*;
    use crate::summary::summary_engine::models::get_available_models;

    fn choices(ram: u64, group: LanguageGroup) -> SummaryModelChoices {
        summary_model_choices_for(ram, group, &get_available_models())
    }

    #[test]
    fn every_shipped_model_is_offered_somewhere() {
        // The point of the step is that nothing is hidden. A model the app can
        // download must appear, even when it is a bad idea here — with the
        // reason attached.
        let c = choices(8, LanguageGroup::English);
        assert_eq!(c.options.len(), get_available_models().len());
        for o in &c.options {
            assert!(
                o.recommended || !o.blocker.is_empty(),
                "{} is not recommended and gives no reason",
                o.name
            );
        }
    }

    #[test]
    fn a_machine_under_the_floor_is_told_the_number() {
        let c = choices(8, LanguageGroup::English);
        let big: Vec<_> = c.options.iter().filter(|o| o.min_ram_gb > 8).collect();
        assert!(!big.is_empty(), "expected some model to need more than 8 GB");
        for o in big {
            assert!(!o.fits_ram);
            assert!(!o.recommended);
            assert!(
                o.blocker.contains("14 GB") && o.blocker.contains("8 GB"),
                "blocker should name both numbers, got {:?}",
                o.blocker
            );
        }
    }

    #[test]
    fn exactly_one_default_and_it_is_recommended() {
        for ram in [4, 8, 16, 64] {
            for group in [LanguageGroup::English, LanguageGroup::Cjk, LanguageGroup::Slavic] {
                let c = choices(ram, group);
                let defaults: Vec<_> = c.options.iter().filter(|o| o.is_default).collect();
                assert_eq!(defaults.len(), 1, "ram={ram} group={group:?}");
                assert!(
                    defaults[0].recommended,
                    "the default must never sit below the line: ram={ram} group={group:?}"
                );
                assert!(defaults[0].fits_ram);
            }
        }
    }

    #[test]
    fn the_cjk_finding_reaches_the_user() {
        // The bake-off measured qwen3.5:2b summarising Japanese into Chinese,
        // reproducibly. Somebody with Japanese meetings must not be handed it
        // without being told.
        let c = choices(8, LanguageGroup::Cjk);
        let small = c.options.iter().find(|o| o.name == "qwen3.5:2b").unwrap();
        assert!(!small.recommended);
        assert!(small.blocker.contains("Japanese"), "got {:?}", small.blocker);

        // ...and it is a language-specific finding, not a blanket verdict.
        let c = choices(8, LanguageGroup::Slavic);
        let small = c.options.iter().find(|o| o.name == "qwen3.5:2b").unwrap();
        assert!(small.recommended);
        assert!(small.blocker.is_empty());
    }

    #[test]
    fn a_low_ram_cjk_machine_still_gets_a_usable_default() {
        // The nastiest combination: under the 14 GB floor, and the small Qwen
        // is disqualified for the language. Something still has to be offered.
        let c = choices(8, LanguageGroup::Cjk);
        let default = c.options.iter().find(|o| o.is_default).unwrap();
        assert!(default.recommended, "default fell below the line: {:?}", default);
        assert!(c.options.iter().any(|o| o.recommended));
    }
}
