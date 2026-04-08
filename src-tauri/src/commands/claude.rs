// Claude Code Hooks Installation Commands
//
// Installs / uninstalls / checks the user-level Claude Code hook setup at
// `~/.claude/settings.json` plus a set of small bash forwarder scripts in
// `~/.claude/hooks/`. Each forwarder reads the JSON Claude Code writes to
// stdin and POSTs it to the matching `/claude_hook/*` endpoint on the local
// Tauri server.
//
// We install one script per matcher group (separate scripts are easier to
// debug than a single dispatcher) and fail open: if the local server is
// unreachable, the script exits 0 with no output, allowing the action.
//
// The shell-output compression hook (managed in `commands/shell_compression.rs`)
// is a separate concern and coexists with the entries we add here.

use crate::SERVER_PORT;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ============================================================================
// Paths
// ============================================================================

fn get_claude_hooks_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".claude").join("hooks"))
}

fn get_claude_settings_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

fn read_claude_settings() -> Result<serde_json::Value, String> {
    let path = get_claude_settings_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read settings.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))
    } else {
        Ok(serde_json::json!({}))
    }
}

fn write_claude_settings(settings: &serde_json::Value) -> Result<(), String> {
    let path = get_claude_settings_path()?;
    let hooks_dir = get_claude_hooks_dir()?;
    if !hooks_dir.exists() {
        fs::create_dir_all(&hooks_dir)
            .map_err(|e| format!("Failed to create ~/.claude/hooks directory: {}", e))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings.json: {}", e))?;
    Ok(())
}

// ============================================================================
// Forwarder script generation
// ============================================================================

/// Per-event forwarder script. Reads stdin once, POSTs to the local server, and
/// pipes the response body back to stdout. Fail-open: if curl can't reach the
/// server, exit 0 with no output so the agent isn't bricked when the Tauri app
/// is closed.
fn generate_forwarder_script(port: u16, endpoint: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
# LLMwatcher Claude Code hook forwarder ({endpoint})
# Reads stdin JSON, POSTs to the local server, prints the response.
set -u

INPUT=$(cat)

RESPONSE=$(printf '%s' "$INPUT" | curl -sS --max-time 10 \
    -H 'Content-Type: application/json' \
    -d @- \
    "http://localhost:{port}/claude_hook/{endpoint}" 2>/dev/null)

CURL_EXIT=$?

# Fail-open: if curl failed (server down, port stale, network) just allow.
if [ $CURL_EXIT -ne 0 ] || [ -z "$RESPONSE" ]; then
    exit 0
fi

printf '%s' "$RESPONSE"
"#,
        port = port,
        endpoint = endpoint,
    )
}

#[derive(Debug, Clone, Copy)]
struct ScriptSpec {
    /// Filename in `~/.claude/hooks/`.
    file_name: &'static str,
    /// Endpoint after `/claude_hook/`.
    endpoint: &'static str,
}

const SCRIPT_SPECS: &[ScriptSpec] = &[
    ScriptSpec { file_name: "llmwatcher-claude-user-prompt-submit.sh", endpoint: "user_prompt_submit" },
    ScriptSpec { file_name: "llmwatcher-claude-pre-bash.sh",          endpoint: "pre_bash" },
    ScriptSpec { file_name: "llmwatcher-claude-pre-read.sh",          endpoint: "pre_read" },
    ScriptSpec { file_name: "llmwatcher-claude-pre-write.sh",         endpoint: "pre_write" },
    ScriptSpec { file_name: "llmwatcher-claude-pre-mcp.sh",           endpoint: "pre_mcp" },
    ScriptSpec { file_name: "llmwatcher-claude-post-tool.sh",         endpoint: "post_tool" },
    ScriptSpec { file_name: "llmwatcher-claude-stop.sh",              endpoint: "stop" },
    ScriptSpec { file_name: "llmwatcher-claude-session-start.sh",     endpoint: "session_start" },
    ScriptSpec { file_name: "llmwatcher-claude-session-end.sh",       endpoint: "session_end" },
];

fn write_script(spec: &ScriptSpec, port: u16) -> Result<String, String> {
    let dir = get_claude_hooks_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create hooks directory: {}", e))?;
    }
    let path = dir.join(spec.file_name);
    let content = generate_forwarder_script(port, spec.endpoint);
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write {}: {}", spec.file_name, e))?;

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&path)
            .map_err(|e| format!("Failed to stat {}: {}", spec.file_name, e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)
            .map_err(|e| format!("Failed to chmod {}: {}", spec.file_name, e))?;
    }

    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Invalid path for {}", spec.file_name))
}

// ============================================================================
// settings.json wiring
// ============================================================================

/// Marker substring used to recognize entries we own.
const OWN_MARKER: &str = "llmwatcher-claude-";

/// Description of one hook entry to install in settings.json.
struct HookInstall {
    /// Top-level key under `hooks` (e.g. "PreToolUse", "Stop", ...).
    event: &'static str,
    /// Optional matcher pattern for tool-call hooks. None for events that
    /// don't accept matchers (Stop, SessionStart, ...).
    matcher: Option<&'static str>,
    /// Filename of the forwarder script to install for this event.
    script_name: &'static str,
}

const HOOK_INSTALLS: &[HookInstall] = &[
    HookInstall {
        event: "UserPromptSubmit",
        matcher: None,
        script_name: "llmwatcher-claude-user-prompt-submit.sh",
    },
    HookInstall {
        event: "PreToolUse",
        matcher: Some("Bash"),
        script_name: "llmwatcher-claude-pre-bash.sh",
    },
    HookInstall {
        event: "PreToolUse",
        matcher: Some("Read|NotebookRead"),
        script_name: "llmwatcher-claude-pre-read.sh",
    },
    HookInstall {
        event: "PreToolUse",
        matcher: Some("Write|Edit|MultiEdit|NotebookEdit"),
        script_name: "llmwatcher-claude-pre-write.sh",
    },
    HookInstall {
        event: "PreToolUse",
        matcher: Some("mcp__.*"),
        script_name: "llmwatcher-claude-pre-mcp.sh",
    },
    HookInstall {
        event: "PostToolUse",
        matcher: Some("Bash|Read|Write|Edit|MultiEdit|NotebookRead|NotebookEdit|mcp__.*"),
        script_name: "llmwatcher-claude-post-tool.sh",
    },
    HookInstall {
        event: "Stop",
        matcher: None,
        script_name: "llmwatcher-claude-stop.sh",
    },
    HookInstall {
        event: "SessionStart",
        matcher: None,
        script_name: "llmwatcher-claude-session-start.sh",
    },
    HookInstall {
        event: "SessionEnd",
        matcher: None,
        script_name: "llmwatcher-claude-session-end.sh",
    },
];

/// Add (or update) one hook entry to a settings.json `hooks` object. Idempotent
/// — if our entry already exists for the same matcher we leave the array
/// unchanged.
fn add_hook_entry(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    install: &HookInstall,
    script_path: &str,
) {
    let event_arr = hooks
        .entry(install.event.to_string())
        .or_insert_with(|| serde_json::json!([]));
    let arr = match event_arr.as_array_mut() {
        Some(a) => a,
        None => {
            *event_arr = serde_json::json!([]);
            event_arr.as_array_mut().unwrap()
        }
    };

    // Drop any existing entry for the same matcher that points at one of our
    // scripts; we'll re-add a fresh one. This handles port-changes / reinstalls.
    arr.retain(|entry| {
        let same_matcher = entry
            .get("matcher")
            .and_then(|v| v.as_str())
            .map(|s| Some(s) == install.matcher)
            .unwrap_or(install.matcher.is_none());
        if !same_matcher {
            return true;
        }
        let owns_one = entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hs| {
                hs.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|s| s.contains(install.script_name))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        !owns_one
    });

    let mut new_entry = serde_json::Map::new();
    if let Some(matcher) = install.matcher {
        new_entry.insert("matcher".to_string(), serde_json::Value::String(matcher.to_string()));
    }
    new_entry.insert(
        "hooks".to_string(),
        serde_json::json!([{
            "type": "command",
            "command": script_path,
        }]),
    );
    arr.push(serde_json::Value::Object(new_entry));
}

/// Strip every entry we own from the settings.json `hooks` object.
fn remove_own_entries(hooks: &mut serde_json::Map<String, serde_json::Value>) {
    let event_keys: Vec<String> = hooks.keys().cloned().collect();
    for key in event_keys {
        let Some(arr) = hooks.get_mut(&key).and_then(|v| v.as_array_mut()) else {
            continue;
        };
        arr.retain(|entry| {
            let owns_any = entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|hs| {
                    hs.iter().any(|h| {
                        h.get("command")
                            .and_then(|c| c.as_str())
                            .map(|s| s.contains(OWN_MARKER))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            !owns_any
        });
        if arr.is_empty() {
            hooks.remove(&key);
        }
    }
}

// ============================================================================
// Tauri commands
// ============================================================================

#[tauri::command]
pub fn install_claude_hooks() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

    // 1. Write all forwarder scripts.
    let mut script_paths: std::collections::HashMap<&'static str, String> =
        std::collections::HashMap::new();
    for spec in SCRIPT_SPECS {
        let path = write_script(spec, port)?;
        script_paths.insert(spec.file_name, path);
    }

    // 2. Update settings.json.
    let mut settings = read_claude_settings()?;
    let obj = settings
        .as_object_mut()
        .ok_or("settings.json is not a valid JSON object")?;
    if !obj.contains_key("hooks") {
        obj.insert("hooks".to_string(), serde_json::json!({}));
    }
    let hooks = obj
        .get_mut("hooks")
        .and_then(|v| v.as_object_mut())
        .ok_or("Failed to access hooks object")?;

    for install in HOOK_INSTALLS {
        let path = script_paths
            .get(install.script_name)
            .ok_or_else(|| format!("Missing script path for {}", install.script_name))?;
        add_hook_entry(hooks, install, path);
    }

    write_claude_settings(&settings)?;

    Ok("Claude Code hooks installed".to_string())
}

#[tauri::command]
pub fn uninstall_claude_hooks() -> Result<String, String> {
    // 1. Remove forwarder scripts.
    let dir = get_claude_hooks_dir()?;
    for spec in SCRIPT_SPECS {
        let path = dir.join(spec.file_name);
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }

    // 2. Strip our entries from settings.json.
    let mut settings = read_claude_settings()?;
    if let Some(obj) = settings.as_object_mut() {
        if let Some(hooks) = obj.get_mut("hooks").and_then(|v| v.as_object_mut()) {
            remove_own_entries(hooks);
            if hooks.is_empty() {
                obj.remove("hooks");
            }
        }
    }
    write_claude_settings(&settings)?;

    Ok("Claude Code hooks removed".to_string())
}

#[tauri::command]
pub fn check_claude_hooks_installed() -> Result<bool, String> {
    let dir = get_claude_hooks_dir()?;
    // We consider hooks installed iff at least one script exists AND the
    // settings.json has at least one entry pointing at one of our scripts.
    let any_script = SCRIPT_SPECS.iter().any(|s| dir.join(s.file_name).exists());
    if !any_script {
        return Ok(false);
    }
    let settings = read_claude_settings()?;
    let installed = settings
        .get("hooks")
        .and_then(|h| h.as_object())
        .map(|hooks| {
            hooks.values().any(|v| {
                v.as_array()
                    .map(|arr| {
                        arr.iter().any(|entry| {
                            entry
                                .get("hooks")
                                .and_then(|h| h.as_array())
                                .map(|hs| {
                                    hs.iter().any(|h| {
                                        h.get("command")
                                            .and_then(|c| c.as_str())
                                            .map(|s| s.contains(OWN_MARKER))
                                            .unwrap_or(false)
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    Ok(installed)
}
