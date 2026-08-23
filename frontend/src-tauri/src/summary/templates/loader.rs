use super::defaults;
use super::types::Template;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use once_cell::sync::Lazy;
use std::sync::RwLock;

// Global storage for the bundled templates directory path
static BUNDLED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Upper bound on template JSON accepted from the user (paste, file import,
/// or drag-drop). A real template is a few KB; this is generous headroom
/// while still rejecting an oversized or accidentally-huge file before it's
/// ever loaded into memory or handed to the JSON parser.
pub const MAX_TEMPLATE_JSON_BYTES: usize = 1024 * 1024; // 1 MiB

/// Set the bundled templates directory path (called once at app startup)
pub fn set_bundled_templates_dir(path: PathBuf) {
    info!("Bundled templates directory set to: {:?}", path);
    if let Ok(mut dir) = BUNDLED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/Halvern/templates/
/// - Windows: %APPDATA%\Halvern\templates\
/// - Linux: ~/.config/Halvern/templates/
fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push(crate::APP_DATA_DIR_NAME);
    path.push("templates");
    Some(path)
}

/// Load a template from the bundled resources directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_bundled_template(template_id: &str) -> Option<String> {
    let bundled_dir = BUNDLED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = bundled_dir.join(format!("{}.json", template_id));

    debug!("Checking for bundled template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded bundled template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No bundled template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Reject any template id that could escape the templates directory.
///
/// Ids are turned into filenames by `format!("{}.json", id)` and joined onto
/// a directory, so an id like `../../secrets` would resolve outside it — and
/// these functions read, write, and delete the resulting path. Restricting
/// ids to characters that cannot form a path component is what keeps every
/// one of those operations inside the templates directory.
fn validate_template_id(template_id: &str) -> Result<&str, String> {
    let trimmed = template_id.trim();
    if trimmed.is_empty() {
        return Err("Template id cannot be empty".to_string());
    }
    let is_filesystem_safe = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !is_filesystem_safe {
        return Err("Template id may only contain letters, numbers, '_' and '-'".to_string());
    }
    Ok(trimmed)
}

/// Load a template from the user's custom templates directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
pub fn load_custom_template(template_id: &str) -> Option<String> {
    let template_id = validate_template_id(template_id).ok()?;
    let custom_dir = get_custom_templates_dir()?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    debug!("Checking for custom template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded custom template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No custom template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load and parse a template by identifier
///
/// This function implements a fallback strategy:
/// 1. Check user's custom templates directory
/// 2. Check bundled resources directory (app templates)
/// 3. Fall back to built-in embedded templates
/// 4. Return error if not found in any location
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// Parsed and validated Template struct
pub fn get_template(template_id: &str) -> Result<Template, String> {
    info!("Loading template: {}", template_id);

    // Try custom template first, then bundled, then built-in
    let json_content = if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(bundled_content) = load_bundled_template(template_id) {
        debug!("Using bundled template for '{}'", template_id);
        bundled_content
    } else if let Some(builtin_content) = defaults::get_builtin_template(template_id) {
        debug!("Using built-in template for '{}'", template_id);
        builtin_content.to_string()
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    // Parse and validate
    validate_and_parse_template(&json_content)
}

/// Validate and parse template JSON
///
/// # Arguments
/// * `json_content` - Raw JSON string
///
/// # Returns
/// Parsed and validated Template struct
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// List all available template identifiers
///
/// Returns a combined list of:
/// - Built-in template IDs
/// - Bundled template IDs (from app resources)
/// - Custom template IDs (from user's data directory)
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = defaults::list_builtin_template_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Add bundled templates if directory is set
    if let Ok(bundled_dir_lock) = BUNDLED_TEMPLATES_DIR.read() {
        if let Some(bundled_dir) = bundled_dir_lock.as_ref() {
            if bundled_dir.exists() {
                match std::fs::read_dir(bundled_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(filename) = entry.file_name().to_str() {
                                if filename.ends_with(".json") {
                                    let id = filename.trim_end_matches(".json").to_string();
                                    if !ids.contains(&id) {
                                        ids.push(id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read bundled templates directory: {}", e);
                    }
                }
            }
        }
    }

    // Add custom templates if directory exists
    for id in list_custom_template_ids() {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    ids.sort();
    ids
}

/// List identifiers of templates in the user's custom templates directory only.
///
/// Unlike `list_template_ids`, this never includes built-in or bundled
/// templates — it exists so callers can tell which listed templates are
/// user-editable (have a file here) versus read-only (compiled in or shipped
/// as an app resource).
pub fn list_custom_template_ids() -> Vec<String> {
    let mut ids = Vec::new();

    let Some(custom_dir) = get_custom_templates_dir() else {
        return ids;
    };
    if !custom_dir.exists() {
        return ids;
    }

    match std::fs::read_dir(&custom_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if let Some(id) = filename.strip_suffix(".json") {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to read custom templates directory: {}", e);
        }
    }

    ids
}

/// Save a template to the user's custom templates directory, creating the
/// directory if needed. Overwrites any existing custom template with the
/// same id — that's how editing an existing custom template works.
///
/// Saving under an id that matches a built-in or bundled template is
/// allowed on purpose: `get_template`'s fallback checks the custom
/// directory first, so this simply shadows the built-in one. Callers
/// that want to warn about this should check `list_template_ids()`
/// (or the built-in/bundled listings) before calling.
///
/// # Arguments
/// * `template_id` - Filesystem-safe identifier (validated here)
/// * `json_content` - Raw template JSON (validated here before writing)
pub fn save_template(template_id: &str, json_content: &str) -> Result<(), String> {
    let trimmed_id = validate_template_id(template_id)?;

    if json_content.len() > MAX_TEMPLATE_JSON_BYTES {
        return Err(format!(
            "Template content is too large ({} KB, max {} KB)",
            json_content.len() / 1024,
            MAX_TEMPLATE_JSON_BYTES / 1024
        ));
    }

    // Validate before writing: never a Tauri command can persist garbage,
    // even if the frontend's own pre-save validation step was skipped.
    validate_and_parse_template(json_content)?;

    let custom_dir = get_custom_templates_dir()
        .ok_or_else(|| "Could not determine custom templates directory".to_string())?;
    std::fs::create_dir_all(&custom_dir)
        .map_err(|e| format!("Failed to create custom templates directory: {}", e))?;

    let template_path = custom_dir.join(format!("{}.json", trimmed_id));
    std::fs::write(&template_path, json_content)
        .map_err(|e| format!("Failed to write template file: {}", e))?;

    info!("Saved custom template '{}' to {:?}", trimmed_id, template_path);
    Ok(())
}

/// Delete a custom template. Only removes files in the user's custom
/// directory — there is nothing to delete for a purely built-in or
/// bundled template, and this returns an error in that case rather than
/// silently doing nothing.
///
/// If the deleted template's id shadowed a built-in or bundled template,
/// `get_template` will resolve to that one again automatically on the
/// next call — no special-casing needed here.
pub fn delete_template(template_id: &str) -> Result<(), String> {
    let template_id = validate_template_id(template_id)?;
    let custom_dir = get_custom_templates_dir()
        .ok_or_else(|| "Could not determine custom templates directory".to_string())?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    if !template_path.exists() {
        return Err(format!(
            "No custom template '{}' to delete (built-in and bundled templates can't be removed)",
            template_id
        ));
    }

    std::fs::remove_file(&template_path)
        .map_err(|e| format!("Failed to delete template file: {}", e))?;

    info!("Deleted custom template '{}' from {:?}", template_id, template_path);
    Ok(())
}

/// List all available templates with their metadata
///
/// Returns a list of (id, name, description) tuples
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    for id in list_template_ids() {
        match get_template(&id) {
            Ok(template) => {
                templates.push((id, template.name, template.description));
            }
            Err(e) => {
                warn!("Failed to load template '{}': {}", id, e);
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_builtin_template() {
        let template = get_template("daily_standup");
        assert!(template.is_ok());

        let template = template.unwrap();
        assert_eq!(template.name, "Daily Standup");
        assert!(!template.sections.is_empty());
    }

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_template_ids() {
        let ids = list_template_ids();
        assert!(ids.contains(&"daily_standup".to_string()));
        assert!(ids.contains(&"standard_meeting".to_string()));
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }

    // save_template's id and content checks are pure — no filesystem access
    // happens before they return, so these run without touching
    // get_custom_templates_dir() (the real app-data directory, not
    // injectable without a larger refactor). The disk-touching round trip
    // (save, appears in list_custom_template_ids, delete) is exercised
    // manually in the packaged app instead, per the review standard for
    // this feature.

    const VALID_TEMPLATE_JSON: &str = r#"{
        "name": "Test Template",
        "description": "A test template",
        "sections": [
            {"title": "Summary", "instruction": "Summarize", "format": "paragraph"}
        ]
    }"#;

    #[test]
    fn test_save_template_rejects_empty_id() {
        let result = save_template("", VALID_TEMPLATE_JSON);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_save_template_rejects_whitespace_only_id() {
        let result = save_template("   ", VALID_TEMPLATE_JSON);
        assert!(result.is_err());
    }

    #[test]
    fn test_save_template_rejects_unsafe_id_characters() {
        for bad_id in ["../escape", "has spaces", "slash/in/id", "id.json", "emoji🎉"] {
            let result = save_template(bad_id, VALID_TEMPLATE_JSON);
            assert!(result.is_err(), "expected '{}' to be rejected", bad_id);
        }
    }

    #[test]
    fn test_save_template_accepts_safe_id_characters() {
        // These must pass the id check specifically — invalid content still
        // rejects further down, so assert the error is about JSON/template
        // validation, not the id.
        for ok_id in ["client_kickoff", "client-kickoff", "ClientKickoff123"] {
            let result = save_template(ok_id, "not valid json");
            let err = result.unwrap_err();
            // Matches the exact phrase save_template's id check uses, so this
            // doesn't false-positive on unrelated words containing "id"
            // (the JSON parser's own error mentions "ident", for instance).
            assert!(
                err.contains("Failed to parse template JSON"),
                "id '{}' should have passed the id check, got: {}",
                ok_id,
                err
            );
        }
    }

    #[test]
    fn test_save_template_rejects_invalid_template_content() {
        let result = save_template("valid_id", "not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_save_template_rejects_template_missing_required_fields() {
        let result = save_template("valid_id", r#"{"name": "", "description": "x", "sections": []}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_save_template_rejects_oversized_content() {
        // Well past MAX_TEMPLATE_JSON_BYTES; must be rejected before the
        // JSON parser ever sees it — the check happens after the id check
        // but before validate_and_parse_template.
        let oversized = "x".repeat(MAX_TEMPLATE_JSON_BYTES + 1);
        let result = save_template("valid_id", &oversized);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_lowercase().contains("large"));
    }

    /// Ids that would escape the templates directory once turned into
    /// `{id}.json` and joined onto it. Every entry point that builds a path
    /// from an id must reject all of these — otherwise a raw `invoke()` call
    /// could read, overwrite, or delete files anywhere the app can reach.
    const TRAVERSAL_IDS: &[&str] = &[
        "../escape",
        "../../etc/passwd",
        "..%2F..%2Fetc",
        "foo/../../bar",
        "/absolute/path",
        "sub/dir",
        "sub\\dir",
        ".",
        "..",
    ];

    #[test]
    fn test_save_template_rejects_path_traversal_ids() {
        for id in TRAVERSAL_IDS {
            let result = save_template(id, VALID_TEMPLATE_JSON);
            assert!(result.is_err(), "save should reject traversal id '{}'", id);
        }
    }

    #[test]
    fn test_delete_template_rejects_path_traversal_ids() {
        for id in TRAVERSAL_IDS {
            let result = delete_template(id);
            assert!(result.is_err(), "delete should reject traversal id '{}'", id);
            // Must fail on the id check, never reach the filesystem and report
            // back about whether the traversed-to file happened to exist.
            assert!(
                result.unwrap_err().contains("Template id"),
                "delete should reject traversal id '{}' at the id check",
                id
            );
        }
    }

    #[test]
    fn test_load_custom_template_rejects_path_traversal_ids() {
        for id in TRAVERSAL_IDS {
            assert!(
                load_custom_template(id).is_none(),
                "load should reject traversal id '{}'",
                id
            );
        }
    }

    #[test]
    fn test_delete_template_errors_on_missing_custom_file() {
        // Not present in the real custom directory under any plausible test
        // run: exercises the "nothing to delete" error path without
        // depending on prior state.
        let result = delete_template("__halvern_loader_test_definitely_missing__");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No custom template"));
    }
}
