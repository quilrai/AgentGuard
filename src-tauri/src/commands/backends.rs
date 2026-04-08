// Predefined Backend Management Commands

use crate::database::Database;
use crate::dlp_pattern_config::get_db_path;
use serde::{Deserialize, Serialize};

// ============================================================================
// Predefined Backend Commands
// ============================================================================

/// Anthropic Claude API base URL — shown as the "Target" field in the
/// predefined backend modal. Inlined here because the dedicated backend
/// modules were removed when the passthrough proxy was deleted.
const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// OpenAI Codex API base URL — shown as the "Target" field in the
/// predefined backend modal.
const CODEX_BASE_URL: &str = "https://api.openai.com";

/// Predefined backend information with settings
#[derive(Debug, Serialize, Deserialize)]
pub struct PredefinedBackendResponse {
    pub name: String,
    pub base_url: String,
    pub settings: String,
}

/// List of predefined backends
const PREDEFINED_BACKENDS: &[(&str, &str)] = &[
    ("claude", ANTHROPIC_BASE_URL),
    ("codex", CODEX_BASE_URL),
    ("cursor-hooks", "N/A"),
];

/// Get all predefined backends with their settings
#[tauri::command]
pub fn get_predefined_backends() -> Result<Vec<PredefinedBackendResponse>, String> {
    let db = Database::new(get_db_path()).map_err(|e| e.to_string())?;

    let mut backends = Vec::new();
    for (name, base_url) in PREDEFINED_BACKENDS {
        let settings = db
            .get_predefined_backend_settings(name)
            .map_err(|e| e.to_string())?;

        backends.push(PredefinedBackendResponse {
            name: name.to_string(),
            base_url: base_url.to_string(),
            settings,
        });
    }

    Ok(backends)
}

/// Update settings for a predefined backend
#[tauri::command]
pub fn update_predefined_backend(name: String, settings: String) -> Result<(), String> {
    // Validate name is a known predefined backend
    let valid_names: Vec<&str> = PREDEFINED_BACKENDS.iter().map(|(n, _)| *n).collect();
    if !valid_names.contains(&name.as_str()) {
        return Err(format!("Unknown predefined backend: {}", name));
    }

    // Validate settings is valid JSON
    let settings = settings.trim();
    if !settings.is_empty() && settings != "{}" {
        serde_json::from_str::<serde_json::Value>(settings)
            .map_err(|_| "Settings must be valid JSON".to_string())?;
    }
    let settings = if settings.is_empty() { "{}" } else { settings };

    let db = Database::new(get_db_path()).map_err(|e| e.to_string())?;

    db.update_predefined_backend_settings(&name, settings)
        .map_err(|e| e.to_string())
}

/// Reset predefined backend settings to defaults
#[tauri::command]
pub fn reset_predefined_backend(name: String) -> Result<(), String> {
    // Validate name is a known predefined backend
    let valid_names: Vec<&str> = PREDEFINED_BACKENDS.iter().map(|(n, _)| *n).collect();
    if !valid_names.contains(&name.as_str()) {
        return Err(format!("Unknown predefined backend: {}", name));
    }

    let db = Database::new(get_db_path()).map_err(|e| e.to_string())?;

    db.reset_predefined_backend_settings(&name)
        .map_err(|e| e.to_string())
}
