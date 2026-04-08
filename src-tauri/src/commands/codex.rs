// Codex CLI Hooks Installation Commands
//
// Installs / uninstalls / checks the user-level Codex CLI hook setup at
// `~/.codex/hooks.json` plus a set of small bash forwarder scripts in
// `~/.codex/hooks/`. Each forwarder reads the JSON Codex writes to stdin and
// POSTs it to the matching `/codex_hook/*` endpoint on the local Tauri server.
//
// Codex hooks are still gated behind a feature flag in `~/.codex/config.toml`
// (`[features] codex_hooks = true`), so install_codex_hooks also writes that
// flag in if it's missing. We do not remove it on uninstall — the user may want
// to keep the flag on for hooks they configured themselves.
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

fn get_codex_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".codex"))
}

fn get_codex_hooks_dir() -> Result<PathBuf, String> {
    Ok(get_codex_dir()?.join("hooks"))
}

fn get_codex_hooks_json_path() -> Result<PathBuf, String> {
    Ok(get_codex_dir()?.join("hooks.json"))
}

fn get_codex_config_toml_path() -> Result<PathBuf, String> {
    Ok(get_codex_dir()?.join("config.toml"))
}

fn read_codex_hooks_json() -> Result<serde_json::Value, String> {
    let path = get_codex_hooks_json_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read hooks.json: {}", e))?;
        if content.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse hooks.json: {}", e))
    } else {
        Ok(serde_json::json!({}))
    }
}

fn write_codex_hooks_json(value: &serde_json::Value) -> Result<(), String> {
    let path = get_codex_hooks_json_path()?;
    let dir = get_codex_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create ~/.codex directory: {}", e))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize hooks.json: {}", e))?;
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write hooks.json: {}", e))?;
    Ok(())
}

// ============================================================================
// config.toml feature-flag patching
// ============================================================================
//
// We do this as a small line-based patcher rather than pulling in the `toml`
// crate. The cases we need to handle are:
//   1. config.toml does not exist           -> create with `[features]\ncodex_hooks = true\n`
//   2. config.toml has no [features] section -> append a new section
//   3. [features] section exists but no codex_hooks key -> insert the key
//   4. [features].codex_hooks = false        -> flip to true
//   5. [features].codex_hooks = true         -> leave alone
//
// This is robust enough for an idempotent install and avoids reformatting the
// rest of the user's TOML.

const CODEX_HOOKS_FEATURE_KEY: &str = "codex_hooks";
const CODEX_FEATURES_HEADER: &str = "[features]";

fn ensure_codex_hooks_feature_enabled() -> Result<(), String> {
    let path = get_codex_config_toml_path()?;
    let dir = get_codex_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create ~/.codex directory: {}", e))?;
    }

    if !path.exists() {
        let content = format!("{}\n{} = true\n", CODEX_FEATURES_HEADER, CODEX_HOOKS_FEATURE_KEY);
        fs::write(&path, content)
            .map_err(|e| format!("Failed to write config.toml: {}", e))?;
        return Ok(());
    }

    let original = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read config.toml: {}", e))?;
    let lines: Vec<&str> = original.lines().collect();

    // Locate the [features] section (if any) and find the index range it covers
    // (header line .. next section header / EOF).
    let mut features_start: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == CODEX_FEATURES_HEADER {
            features_start = Some(i);
            break;
        }
    }

    let features_end = if let Some(start) = features_start {
        let mut end = lines.len();
        for (i, line) in lines.iter().enumerate().skip(start + 1) {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                end = i;
                break;
            }
        }
        Some(end)
    } else {
        None
    };

    // Case A: no [features] section -> append.
    let Some(start) = features_start else {
        let mut new_content = original.clone();
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        if !new_content.is_empty() {
            new_content.push('\n');
        }
        new_content.push_str(CODEX_FEATURES_HEADER);
        new_content.push('\n');
        new_content.push_str(CODEX_HOOKS_FEATURE_KEY);
        new_content.push_str(" = true\n");
        fs::write(&path, new_content)
            .map_err(|e| format!("Failed to write config.toml: {}", e))?;
        return Ok(());
    };
    let end = features_end.unwrap_or(lines.len());

    // Case B/C/D: scan inside the section for an existing codex_hooks key.
    let mut updated_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut found_key = false;
    for (i, line) in lines.iter().enumerate().skip(start + 1).take(end - start - 1) {
        let trimmed = line.trim_start();
        // Skip comments / blank lines.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Match `codex_hooks` with any whitespace around the `=`.
        let key_part = trimmed
            .split('=')
            .next()
            .map(|s| s.trim())
            .unwrap_or("");
        if key_part == CODEX_HOOKS_FEATURE_KEY {
            // Force the value to true. Preserve any leading indentation.
            let leading_ws_len = line.len() - trimmed.len();
            let leading_ws = &line[..leading_ws_len];
            updated_lines[i] = format!("{}{} = true", leading_ws, CODEX_HOOKS_FEATURE_KEY);
            found_key = true;
            break;
        }
    }

    if !found_key {
        // Insert the key at the start of the section body.
        updated_lines.insert(start + 1, format!("{} = true", CODEX_HOOKS_FEATURE_KEY));
    }

    let mut new_content = updated_lines.join("\n");
    if original.ends_with('\n') && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    fs::write(&path, new_content)
        .map_err(|e| format!("Failed to write config.toml: {}", e))?;
    Ok(())
}

/// Read-only counterpart to `ensure_codex_hooks_feature_enabled`. Returns
/// `true` only if `~/.codex/config.toml` exists, contains a `[features]`
/// section, and within that section has a `codex_hooks` key set to a truthy
/// TOML literal (`true` / `1` / `"true"`). Used by `check_codex_hooks_installed`
/// so the UI doesn't claim Codex hooks are wired up after the user disables
/// the feature flag — Codex would silently stop loading hooks in that case.
fn is_codex_hooks_feature_enabled() -> bool {
    let Ok(path) = get_codex_config_toml_path() else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    let Ok(content) = fs::read_to_string(&path) else {
        return false;
    };

    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|line| line.trim() == CODEX_FEATURES_HEADER)
    else {
        return false;
    };

    // Walk the [features] section body until the next section header / EOF.
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().map(|s| s.trim()).unwrap_or("");
        if key != CODEX_HOOKS_FEATURE_KEY {
            continue;
        }
        let value = parts
            .next()
            .map(|s| {
                // Strip an inline comment (`# ...`) and surrounding whitespace.
                let v = s.split('#').next().unwrap_or("").trim();
                v.to_string()
            })
            .unwrap_or_default();
        return matches!(value.as_str(), "true" | "1" | "\"true\"");
    }

    false
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
# LLMwatcher Codex CLI hook forwarder ({endpoint})
# Reads stdin JSON, POSTs to the local server, prints the response.
set -u

INPUT=$(cat)

RESPONSE=$(printf '%s' "$INPUT" | curl -sS --max-time 10 \
    -H 'Content-Type: application/json' \
    -d @- \
    "http://localhost:{port}/codex_hook/{endpoint}" 2>/dev/null)

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
    /// Filename in `~/.codex/hooks/`.
    file_name: &'static str,
    /// Endpoint after `/codex_hook/`.
    endpoint: &'static str,
}

const SCRIPT_SPECS: &[ScriptSpec] = &[
    ScriptSpec { file_name: "llmwatcher-codex-user-prompt-submit.sh", endpoint: "user_prompt_submit" },
    ScriptSpec { file_name: "llmwatcher-codex-pre-bash.sh",          endpoint: "pre_bash" },
    ScriptSpec { file_name: "llmwatcher-codex-post-tool.sh",         endpoint: "post_tool" },
    ScriptSpec { file_name: "llmwatcher-codex-stop.sh",              endpoint: "stop" },
    ScriptSpec { file_name: "llmwatcher-codex-session-start.sh",     endpoint: "session_start" },
];

fn write_script(spec: &ScriptSpec, port: u16) -> Result<String, String> {
    let dir = get_codex_hooks_dir()?;
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
// hooks.json wiring
// ============================================================================

/// Marker substring used to recognize entries we own.
const OWN_MARKER: &str = "llmwatcher-codex-";

/// Description of one hook entry to install in hooks.json.
struct HookInstall {
    /// Top-level key under `hooks` (e.g. "PreToolUse", "Stop", ...).
    event: &'static str,
    /// Optional matcher pattern for tool-call hooks. None for events that
    /// don't accept matchers (Stop, SessionStart, UserPromptSubmit).
    matcher: Option<&'static str>,
    /// Filename of the forwarder script to install for this event.
    script_name: &'static str,
}

const HOOK_INSTALLS: &[HookInstall] = &[
    HookInstall {
        event: "UserPromptSubmit",
        matcher: None,
        script_name: "llmwatcher-codex-user-prompt-submit.sh",
    },
    // Codex's PreToolUse / PostToolUse currently only fire for Bash; once Codex
    // exposes Read/Write/MCP we'll add matchers here the same way claude_hooks
    // does.
    HookInstall {
        event: "PreToolUse",
        matcher: Some("Bash"),
        script_name: "llmwatcher-codex-pre-bash.sh",
    },
    HookInstall {
        event: "PostToolUse",
        matcher: Some("Bash"),
        script_name: "llmwatcher-codex-post-tool.sh",
    },
    HookInstall {
        event: "Stop",
        matcher: None,
        script_name: "llmwatcher-codex-stop.sh",
    },
    HookInstall {
        event: "SessionStart",
        matcher: None,
        script_name: "llmwatcher-codex-session-start.sh",
    },
];

/// Add (or update) one hook entry to a hooks.json `hooks` object. Idempotent —
/// if our entry already exists for the same matcher we replace it (handles
/// port-changes / reinstalls).
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

/// Strip every entry we own from the hooks.json `hooks` object.
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
pub fn install_codex_hooks() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

    // 1. Make sure the codex_hooks feature flag is on in config.toml.
    ensure_codex_hooks_feature_enabled()?;

    // 2. Write all forwarder scripts.
    let mut script_paths: std::collections::HashMap<&'static str, String> =
        std::collections::HashMap::new();
    for spec in SCRIPT_SPECS {
        let path = write_script(spec, port)?;
        script_paths.insert(spec.file_name, path);
    }

    // 3. Update hooks.json.
    let mut hooks_json = read_codex_hooks_json()?;
    let obj = hooks_json
        .as_object_mut()
        .ok_or("hooks.json is not a valid JSON object")?;
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

    write_codex_hooks_json(&hooks_json)?;

    Ok("Codex CLI hooks installed".to_string())
}

#[tauri::command]
pub fn uninstall_codex_hooks() -> Result<String, String> {
    // 1. Remove forwarder scripts.
    let dir = get_codex_hooks_dir()?;
    if dir.exists() {
        for spec in SCRIPT_SPECS {
            let path = dir.join(spec.file_name);
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
        }
    }

    // 2. Strip our entries from hooks.json. Leave the file in place if other
    // hooks still live there; delete it if we emptied it out.
    let path = get_codex_hooks_json_path()?;
    if path.exists() {
        let mut hooks_json = read_codex_hooks_json()?;
        if let Some(obj) = hooks_json.as_object_mut() {
            if let Some(hooks) = obj.get_mut("hooks").and_then(|v| v.as_object_mut()) {
                remove_own_entries(hooks);
                if hooks.is_empty() {
                    obj.remove("hooks");
                }
            }
        }
        let is_empty = hooks_json
            .as_object()
            .map(|o| o.is_empty())
            .unwrap_or(true);
        if is_empty {
            let _ = fs::remove_file(&path);
        } else {
            write_codex_hooks_json(&hooks_json)?;
        }
    }

    // We intentionally do NOT touch the [features] codex_hooks flag in
    // config.toml; the user may want it on for hooks they configured
    // themselves.

    Ok("Codex CLI hooks removed".to_string())
}

#[tauri::command]
pub fn check_codex_hooks_installed() -> Result<bool, String> {
    // We consider hooks installed iff:
    //   1. The [features].codex_hooks flag is enabled in ~/.codex/config.toml.
    //      Without this, Codex silently won't load hooks even if hooks.json
    //      points at our scripts.
    //   2. At least one forwarder script exists on disk.
    //   3. hooks.json has at least one entry pointing at one of our scripts.
    if !is_codex_hooks_feature_enabled() {
        return Ok(false);
    }
    let dir = get_codex_hooks_dir()?;
    let any_script = SCRIPT_SPECS.iter().any(|s| dir.join(s.file_name).exists());
    if !any_script {
        return Ok(false);
    }
    let hooks_json = read_codex_hooks_json()?;
    let installed = hooks_json
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
