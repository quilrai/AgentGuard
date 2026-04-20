// Cursor Hooks Installation Commands

use crate::SERVER_PORT;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// Get the cursor hooks directory path
fn get_cursor_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".cursor"))
}

/// Get the shell script path
fn get_script_path() -> Result<PathBuf, String> {
    Ok(get_cursor_dir()?.join("quilr-cursor-hooks.sh"))
}

/// Get the hooks.json path
fn get_hooks_json_path() -> Result<PathBuf, String> {
    Ok(get_cursor_dir()?.join("hooks.json"))
}

/// Generate the shell script content
fn generate_shell_script(port: u16) -> String {
    format!(
        r#"#!/bin/bash
# Quilr DLP Hook Script for Cursor
# This script is called by Cursor hooks to check for sensitive data

# Read JSON input from stdin
INPUT=$(cat)

# Extract hook_event_name from JSON. Using grep/sed here is brittle if the
# payload formatting changes, so parse the JSON properly.
if ! command -v perl >/dev/null 2>&1; then
    echo '{{"status": "ok"}}'
    exit 0
fi

HOOK_NAME=$(printf '%s' "$INPUT" | perl -MJSON::PP -e '
    use strict;
    use warnings;
    local $/;
    my $raw = <STDIN>;
    my $data = eval {{ JSON::PP::decode_json($raw) }};
    exit 0 unless $data && ref($data) eq "HASH";
    my $name = $data->{{hook_event_name}};
    print $name if defined $name && !ref($name);
')

# Map hook names to API endpoints
case "$HOOK_NAME" in
    "beforeSubmitPrompt")
        ENDPOINT="before_submit_prompt"
        ;;
    "beforeReadFile")
        ENDPOINT="before_read_file"
        ;;
    "beforeTabFileRead")
        ENDPOINT="before_tab_file_read"
        ;;
    "beforeShellExecution")
        ENDPOINT="before_shell_execution"
        ;;
    "beforeMCPExecution")
        ENDPOINT="before_mcp_execution"
        ;;
    "afterAgentResponse")
        ENDPOINT="after_agent_response"
        ;;
    "afterAgentThought")
        ENDPOINT="after_agent_thought"
        ;;
    "afterTabFileEdit")
        ENDPOINT="after_tab_file_edit"
        ;;
    *)
        # Unknown hook, allow by default
        echo '{{"status": "ok"}}'
        exit 0
        ;;
esac

# Call the Quilr API
RESPONSE=$(echo "$INPUT" | curl -s -X POST \
    -H "Content-Type: application/json" \
    -d @- \
    "http://localhost:{port}/cursor_hook/$ENDPOINT" 2>/dev/null)

# If curl failed or empty response, allow by default
if [ -z "$RESPONSE" ]; then
    case "$HOOK_NAME" in
        "beforeSubmitPrompt")
            echo '{{"continue": true}}'
            ;;
        "beforeReadFile"|"beforeTabFileRead"|"beforeShellExecution"|"beforeMCPExecution")
            echo '{{"permission": "allow"}}'
            ;;
        *)
            echo '{{"status": "ok"}}'
            ;;
    esac
    exit 0
fi

# Return the API response
echo "$RESPONSE"
"#,
        port = port
    )
}

/// Hooks configuration structure
#[derive(Debug, Serialize, Deserialize, Default)]
struct HooksConfig {
    version: i32,
    hooks: HashMap<String, Vec<HookEntry>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HookEntry {
    command: String,
}

/// Our hook names that we manage
const QUILR_HOOKS: &[&str] = &[
    "beforeSubmitPrompt",
    "beforeReadFile",
    "beforeTabFileRead",
    "beforeShellExecution",
    "beforeMCPExecution",
    "afterAgentResponse",
    "afterAgentThought",
    "afterTabFileEdit",
];

#[tauri::command]
pub fn install_cursor_hooks() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

    // Ensure ~/.cursor directory exists
    let cursor_dir = get_cursor_dir()?;
    if !cursor_dir.exists() {
        fs::create_dir_all(&cursor_dir)
            .map_err(|e| format!("Failed to create ~/.cursor directory: {}", e))?;
    }

    // Write the shell script
    let script_path = get_script_path()?;
    let script_content = generate_shell_script(port);
    fs::write(&script_path, &script_content)
        .map_err(|e| format!("Failed to write hook script: {}", e))?;

    // Set executable permissions (755)
    let mut perms = fs::metadata(&script_path)
        .map_err(|e| format!("Failed to get script metadata: {}", e))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms)
        .map_err(|e| format!("Failed to set script permissions: {}", e))?;

    // Get absolute path for hooks.json
    let script_path_str = script_path
        .to_str()
        .ok_or("Invalid script path")?
        .to_string();

    // Read or create hooks.json
    let hooks_json_path = get_hooks_json_path()?;
    let mut config: HooksConfig = if hooks_json_path.exists() {
        let content = fs::read_to_string(&hooks_json_path)
            .map_err(|e| format!("Failed to read hooks.json: {}", e))?;
        serde_json::from_str(&content).unwrap_or(HooksConfig {
            version: 1,
            hooks: HashMap::new(),
        })
    } else {
        HooksConfig {
            version: 1,
            hooks: HashMap::new(),
        }
    };

    // Ensure version is set
    if config.version == 0 {
        config.version = 1;
    }

    // Add our hooks to the config
    let quilr_entry = HookEntry {
        command: script_path_str.clone(),
    };

    for hook_name in QUILR_HOOKS {
        let hook_list = config.hooks.entry(hook_name.to_string()).or_default();

        // Check if our hook is already in the list
        let already_exists = hook_list
            .iter()
            .any(|entry| entry.command.contains("quilr-cursor-hooks.sh"));

        if !already_exists {
            hook_list.push(quilr_entry.clone());
        }
    }

    // Write updated hooks.json
    let json_content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize hooks.json: {}", e))?;
    fs::write(&hooks_json_path, json_content)
        .map_err(|e| format!("Failed to write hooks.json: {}", e))?;

    Ok(format!(
        "Cursor hooks installed successfully. Script: {}",
        script_path_str
    ))
}

#[tauri::command]
pub fn uninstall_cursor_hooks() -> Result<String, String> {
    // Remove our hooks from hooks.json
    let hooks_json_path = get_hooks_json_path()?;

    if hooks_json_path.exists() {
        let content = fs::read_to_string(&hooks_json_path)
            .map_err(|e| format!("Failed to read hooks.json: {}", e))?;

        let mut config: HooksConfig = serde_json::from_str(&content).unwrap_or(HooksConfig {
            version: 1,
            hooks: HashMap::new(),
        });

        // Remove our hooks from each hook type
        for hook_name in QUILR_HOOKS {
            if let Some(hook_list) = config.hooks.get_mut(*hook_name) {
                hook_list.retain(|entry| !entry.command.contains("quilr-cursor-hooks.sh"));
            }
        }

        // Remove empty hook arrays
        config.hooks.retain(|_, v| !v.is_empty());

        // Write updated hooks.json
        let json_content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize hooks.json: {}", e))?;
        fs::write(&hooks_json_path, json_content)
            .map_err(|e| format!("Failed to write hooks.json: {}", e))?;
    }

    // Remove the shell script
    let script_path = get_script_path()?;
    if script_path.exists() {
        fs::remove_file(&script_path)
            .map_err(|e| format!("Failed to remove hook script: {}", e))?;
    }

    Ok("Cursor hooks uninstalled successfully".to_string())
}

#[tauri::command]
pub fn check_cursor_hooks_installed() -> Result<bool, String> {
    let script_path = get_script_path()?;
    let hooks_json_path = get_hooks_json_path()?;

    // Check if script exists
    if !script_path.exists() {
        return Ok(false);
    }

    // Check if hooks.json has our hooks
    if !hooks_json_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&hooks_json_path)
        .map_err(|e| format!("Failed to read hooks.json: {}", e))?;

    let config: HooksConfig = serde_json::from_str(&content).unwrap_or(HooksConfig {
        version: 1,
        hooks: HashMap::new(),
    });

    // Check if at least beforeSubmitPrompt has our hook
    if let Some(hook_list) = config.hooks.get("beforeSubmitPrompt") {
        let has_quilr = hook_list
            .iter()
            .any(|entry| entry.command.contains("quilr-cursor-hooks.sh"));
        return Ok(has_quilr);
    }

    Ok(false)
}
