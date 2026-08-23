//! Tauri commands backing the Obsidian export.
//!
//! The folder picker runs on the Rust side on purpose: `@tauri-apps/plugin-dialog`
//! is not among the frontend dependencies, and adding an npm package would need
//! network access this build environment does not have. The Rust crate is already
//! a dependency, so the picker costs nothing extra.

use log::{error as log_error, info as log_info};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;

use crate::{database::repositories::setting::SettingsRepository, state::AppState};

/// Obsidian export configuration as exchanged with the frontend.
///
/// `transcriptPath` and `summaryPath` are optional overrides of `vaultPath`.
/// Leaving them empty writes both notes into the vault, which is the behaviour
/// from before the split.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianSettings {
    #[serde(rename = "vaultPath")]
    pub vault_path: Option<String>,
    #[serde(rename = "transcriptPath")]
    pub transcript_path: Option<String>,
    #[serde(rename = "summaryPath")]
    pub summary_path: Option<String>,
    #[serde(rename = "autoExport")]
    pub auto_export: bool,
}

/// Opens a native folder picker and returns the chosen path.
///
/// Returns `Ok(None)` when the user dismisses the dialog. Uses the callback API
/// rather than `blocking_pick_folder`, because the blocking variant must not run
/// on the main thread and would risk deadlocking the event loop.
#[tauri::command]
pub async fn api_pick_obsidian_vault<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    log_info!("api_pick_obsidian_vault called");

    let (tx, rx) = tokio::sync::oneshot::channel();

    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });

    let picked = rx
        .await
        .map_err(|e| format!("The folder picker was interrupted: {}", e))?;

    let path = match picked {
        Some(file_path) => Some(
            file_path
                .into_path()
                .map_err(|e| format!("The chosen location is not a file path: {}", e))?
                .to_string_lossy()
                .to_string(),
        ),
        None => None,
    };

    log_info!("Vault picked: {:?}", path);
    Ok(path)
}

/// Reads the stored Obsidian export configuration.
#[tauri::command]
pub async fn api_get_obsidian_settings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<ObsidianSettings, String> {
    let pool = state.db_manager.pool();

    let config = SettingsRepository::get_obsidian_settings(pool)
        .await
        .map_err(|e| format!("Reading the Obsidian settings failed: {}", e))?;

    Ok(ObsidianSettings {
        vault_path: config.vault_path,
        transcript_path: config.transcript_path,
        summary_path: config.summary_path,
        auto_export: config.auto_export,
    })
}

/// Trims a path, turns blanks into `None`, and rejects anything that cannot
/// serve as a destination directory.
fn validate_directory(value: Option<String>, label: &str) -> Result<Option<String>, String> {
    let normalised = value
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    if let Some(path) = &normalised {
        let candidate = PathBuf::from(path);
        if !candidate.is_absolute() {
            return Err(format!("{} must be an absolute path", label));
        }
        if candidate.exists() && !candidate.is_dir() {
            return Err(format!("{} exists but is not a directory", label));
        }
    }

    Ok(normalised)
}

/// Persists the Obsidian export configuration.
#[tauri::command]
pub async fn api_save_obsidian_settings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    vault_path: Option<String>,
    transcript_path: Option<String>,
    summary_path: Option<String>,
    auto_export: bool,
) -> Result<(), String> {
    log_info!(
        "api_save_obsidian_settings called: vault={:?}, transcript={:?}, summary={:?}, auto={}",
        vault_path,
        transcript_path,
        summary_path,
        auto_export
    );

    let vault = validate_directory(vault_path, "Katalog vaulta")?;
    let transcript = validate_directory(transcript_path, "The transcript directory")?;
    let summary = validate_directory(summary_path, "The summary directory")?;

    // Overrides fall back to the vault, so at least the vault must exist as a
    // setting unless both overrides cover the two destinations themselves.
    let covered_by_overrides = transcript.is_some() && summary.is_some();
    if vault.is_none() && !covered_by_overrides {
        if transcript.is_some() || summary.is_some() {
            return Err(
                "Without a vault directory both paths, transcripts and summaries, must be given"
                    .to_string(),
            );
        }
        if auto_export {
            return Err("Automatyczny eksport wymaga wskazania katalogu".to_string());
        }
    }

    let pool = state.db_manager.pool();
    SettingsRepository::save_obsidian_settings(
        pool,
        &crate::database::repositories::setting::ObsidianConfig {
            vault_path: vault,
            transcript_path: transcript,
            summary_path: summary,
            auto_export,
        },
    )
    .await
    .map_err(|e| format!("Writing the Obsidian settings failed: {}", e))
}

/// Exports one meeting on demand, regardless of the auto-export switch.
#[tauri::command]
pub async fn api_export_meeting<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<String>, String> {
    log_info!("api_export_meeting called: {}", meeting_id);

    let pool = state.db_manager.pool();

    let config = SettingsRepository::get_obsidian_settings(pool)
        .await
        .map_err(|e| format!("Reading the Obsidian settings failed: {}", e))?;

    let targets = super::targets_from_config(&config)
        .ok_or_else(|| "No destination folder is set in the settings".to_string())?;

    match super::export_meeting(pool, &targets, &meeting_id).await {
        Ok(paths) => {
            let as_strings: Vec<String> = paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            log_info!("Exported {} files: {:?}", as_strings.len(), as_strings);
            Ok(as_strings)
        }
        Err(e) => {
            log_error!("Exporting meeting {} failed: {}", meeting_id, e);
            Err(e)
        }
    }
}
