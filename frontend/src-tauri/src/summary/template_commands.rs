use crate::summary::templates;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;
use tracing::{info, warn};

/// Template metadata for UI display
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateInfo {
    /// Template identifier (e.g., "daily_standup", "standard_meeting")
    pub id: String,

    /// Display name for the template
    pub name: String,

    /// Brief description of the template's purpose
    pub description: String,

    /// Whether this template has a file in the user's custom templates
    /// directory. Only custom templates can be edited or deleted from the UI;
    /// built-in and bundled templates are read-only.
    pub is_custom: bool,
}

/// Detailed template structure for preview/debugging
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateDetails {
    /// Template identifier
    pub id: String,

    /// Display name
    pub name: String,

    /// Description
    pub description: String,

    /// List of section titles in order
    pub sections: Vec<String>,
}

/// Lists all available templates
///
/// Returns templates from both built-in (embedded) and custom (user data directory) sources.
/// Templates are automatically discovered - no code changes needed to add new templates.
///
/// # Returns
/// Vector of TemplateInfo with id, name, and description for each template
#[tauri::command]
pub async fn api_list_templates<R: Runtime>(
    _app: tauri::AppHandle<R>,
) -> Result<Vec<TemplateInfo>, String> {
    info!("api_list_templates called");

    let templates = templates::list_templates();
    let custom_ids = templates::list_custom_template_ids();

    let template_infos: Vec<TemplateInfo> = templates
        .into_iter()
        .map(|(id, name, description)| {
            let is_custom = custom_ids.contains(&id);
            TemplateInfo {
                id,
                name,
                description,
                is_custom,
            }
        })
        .collect();

    info!("Found {} available templates", template_infos.len());

    Ok(template_infos)
}

/// Gets detailed information about a specific template
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup")
///
/// # Returns
/// TemplateDetails with full template structure
#[tauri::command]
pub async fn api_get_template_details<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<TemplateDetails, String> {
    info!("api_get_template_details called for template_id: {}", template_id);

    let template = templates::get_template(&template_id)?;

    let section_titles: Vec<String> = template
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect();

    let details = TemplateDetails {
        id: template_id,
        name: template.name,
        description: template.description,
        sections: section_titles,
    };

    info!("Retrieved template details for '{}'", details.name);

    Ok(details)
}

/// Validates a custom template JSON string
///
/// Useful for template editor UI or validation before saving custom templates
///
/// # Arguments
/// * `template_json` - Raw JSON string of the template
///
/// # Returns
/// Ok(template_name) if valid, Err(error_message) if invalid
#[tauri::command]
pub async fn api_validate_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_json: String,
) -> Result<String, String> {
    info!("api_validate_template called");

    match templates::validate_and_parse_template(&template_json) {
        Ok(template) => {
            info!("Template '{}' validated successfully", template.name);
            Ok(template.name)
        }
        Err(e) => {
            warn!("Template validation failed: {}", e);
            Err(e)
        }
    }
}

/// Saves a template to the user's custom templates directory.
///
/// Validates the id and JSON content again on this side, independent of
/// any client-side check: a raw `invoke()` call must not be able to bypass
/// validation and write something the JSON parser or `Template::validate`
/// would reject.
///
/// # Arguments
/// * `template_id` - Filesystem-safe identifier this template will be saved under
/// * `template_json` - Raw template JSON
#[tauri::command]
pub async fn api_save_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
    template_json: String,
) -> Result<(), String> {
    info!("api_save_template called for template_id: {}", template_id);

    match templates::save_template(&template_id, &template_json) {
        Ok(()) => {
            info!("Template '{}' saved successfully", template_id);
            Ok(())
        }
        Err(e) => {
            warn!("Failed to save template '{}': {}", template_id, e);
            Err(e)
        }
    }
}

/// Deletes a custom template.
///
/// Only removes a file from the user's custom templates directory — there is
/// nothing to delete for a built-in or bundled template, and this errors in
/// that case rather than silently succeeding.
#[tauri::command]
pub async fn api_delete_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<(), String> {
    info!("api_delete_template called for template_id: {}", template_id);

    match templates::delete_template(&template_id) {
        Ok(()) => {
            info!("Template '{}' deleted successfully", template_id);
            Ok(())
        }
        Err(e) => {
            warn!("Failed to delete template '{}': {}", template_id, e);
            Err(e)
        }
    }
}

/// Gets the raw JSON of a custom template, for pre-filling the edit form.
///
/// Only custom templates are editable, so this deliberately does not fall
/// back to bundled/built-in content the way `get_template` does — editing a
/// built-in template isn't a supported flow.
#[tauri::command]
pub async fn api_get_custom_template_raw<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<String, String> {
    info!("api_get_custom_template_raw called for template_id: {}", template_id);

    templates::load_custom_template(&template_id).ok_or_else(|| {
        format!("No custom template '{}' found", template_id)
    })
}

/// Reads template JSON from a file, applying the same size cap as
/// `save_template` before the content is ever loaded into memory. Shared by
/// both file-import paths below.
fn read_template_file(path: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    if metadata.len() as usize > templates::MAX_TEMPLATE_JSON_BYTES {
        return Err(format!(
            "File is too large ({} KB, max {} KB)",
            metadata.len() / 1024,
            templates::MAX_TEMPLATE_JSON_BYTES / 1024
        ));
    }

    std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))
}

/// Opens a native file dialog filtered to JSON files and reads the selected
/// file's content. Returns `None` if the user cancels.
#[tauri::command]
pub async fn pick_template_file_command<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<String>, String> {
    info!("Opening file dialog for template import");

    let app_clone = app.clone();
    let file_path = tokio::task::spawn_blocking(move || {
        app_clone
            .dialog()
            .file()
            .add_filter("Template JSON", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("File dialog task failed: {}", e))?;

    match file_path {
        Some(path) => {
            let path_str = path.to_string();
            info!("User selected template file: {}", path_str);
            read_template_file(Path::new(&path_str)).map(Some)
        }
        None => {
            info!("User cancelled template file selection");
            Ok(None)
        }
    }
}

/// Reads template JSON from a given path (for drag-and-drop, which hands the
/// frontend a path rather than file content).
#[tauri::command]
pub async fn read_template_file_command(path: String) -> Result<String, String> {
    info!("Reading dropped template file: {}", path);
    read_template_file(Path::new(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_templates() {
        // This test requires the templates to be embedded/available
        // In a real test environment, you might want to mock the templates module

        // For now, just verify the function compiles and runs
        // You can expand this with more specific assertions
    }

    #[tokio::test]
    async fn test_validate_template_valid() {
        let valid_json = r#"
        {
            "name": "Test Template",
            "description": "A test template",
            "sections": [
                {
                    "title": "Summary",
                    "instruction": "Provide a summary",
                    "format": "paragraph"
                }
            ]
        }"#;

        // Mock app handle would be needed for actual testing
        // For now, test the validation logic directly
        let result = templates::validate_and_parse_template(valid_json);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_template_invalid() {
        let invalid_json = "invalid json";

        let result = templates::validate_and_parse_template(invalid_json);
        assert!(result.is_err());
    }
}
