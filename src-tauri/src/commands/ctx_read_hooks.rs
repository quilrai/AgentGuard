// Install / uninstall / check the ctx_read PreToolUse hook for Claude Code.
//
// When enabled this hook intercepts Read tool calls and routes them through
// the local server's `/ctx/pre_read` endpoint.  If the file is already
// cached and unchanged the server returns a compact stub (e.g.
// "F3=main.rs cached 5t 180L") that gets written to a temp file; the hook
// then redirects the Read to that temp file via `updatedInput`.
//
// Ordering:  DLP pre_read (guardian) → ctx_read → shell compression (Bash).
// The install logic ensures the ctx_read entry sits after the guardian
// pre_read entry but before the compression entry in PreToolUse.

use crate::SERVER_PORT;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ============================================================================
// Hook script
// ============================================================================

fn generate_ctx_read_hook_script(port: u16, backend_name: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
# LLMwatcher File-Read Caching Hook
# Intercepts Read tool calls and returns cached stubs for unchanged files.
set -euo pipefail

INPUT=$(cat)

# PreToolUse is already matched to Read in Claude settings, so we only need
# the file path. Parse the JSON payload properly so escaped characters in the
# request body do not truncate the extracted value.
if ! command -v perl >/dev/null 2>&1; then
  exit 0
fi

json_get() {{
  local path="$1"
  printf '%s' "$INPUT" | perl -MJSON::PP -e '
    use strict;
    use warnings;

    my $path = shift @ARGV;
    local $/;
    my $raw = <STDIN>;
    my $data = eval {{ JSON::PP::decode_json($raw) }};
    exit 0 unless $data;

    my $cur = $data;
    for my $part (split /\./, $path) {{
      if (ref($cur) eq "HASH" && exists $cur->{{$part}}) {{
        $cur = $cur->{{$part}};
      }} else {{
        $cur = undef;
        last;
      }}
    }}

    if (defined $cur && !ref($cur)) {{
      print $cur;
    }}
  ' "$path"
}}

FILE_PATH=$(json_get "tool_input.file_path")
if [ -z "$FILE_PATH" ]; then
  exit 0
fi

# Skip partial reads (offset/limit).  Applying a line range to a one-line
# stub would return wrong content, so let the native Read handle these.
if echo "$INPUT" | grep -qE '"(offset|limit)"[[:space:]]*:'; then
  exit 0
fi

# Skip if file does not exist (let Claude's native error handling deal with it)
if [ ! -f "$FILE_PATH" ]; then
  exit 0
fi

# Escape for JSON and write payload to temp file (avoids bash double-quote nesting)
ESCAPED_PATH=$(printf '%s' "$FILE_PATH" | sed 's/\\/\\\\/g; s/"/\\"/g')
_CTX_PF=$(mktemp)
printf '{{"path":"%s","backend":"{backend_name}"}}' "$ESCAPED_PATH" > "$_CTX_PF"

# Ask the server whether this file is cached + unchanged
_CTX_TMP=$(mktemp)
_CTX_HC=$(curl -sS -o "$_CTX_TMP" -w '%{{http_code}}' \
  -X POST -H 'Content-Type: application/json' \
  -d @"$_CTX_PF" \
  "http://localhost:{port}/ctx/pre_read" 2>/dev/null) || true
rm -f "$_CTX_PF"

# 200 = cached stub available → write to a deterministic file and redirect Read
if [ "$_CTX_HC" = "200" ]; then
  # Use a fixed stub directory so old stubs are overwritten, not leaked.
  _CTX_DIR="${{TMPDIR:-/tmp}}/llmwatcher-ctx-stubs"
  mkdir -p "$_CTX_DIR"
  # Derive a stable filename from the file path (md5 hash).
  _CTX_HASH=$(printf '%s' "$FILE_PATH" | md5sum 2>/dev/null | cut -d' ' -f1 || md5 -q -s "$FILE_PATH" 2>/dev/null || echo "fallback")
  _CTX_STUB="$_CTX_DIR/$_CTX_HASH"
  cat "$_CTX_TMP" > "$_CTX_STUB"
  rm -f "$_CTX_TMP"
  # Escape the stub path for JSON
  STUB_ESC=$(printf '%s' "$_CTX_STUB" | sed 's/\\/\\\\/g; s/"/\\"/g')
  printf '{{"hookSpecificOutput":{{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{{"file_path":"%s"}}}}}}\n' "$STUB_ESC"
  exit 0
fi

# 204 or anything else = allow the read through normally (first read / changed)
rm -f "$_CTX_TMP"
exit 0
"#
    )
}

// ============================================================================
// Paths
// ============================================================================

fn get_claude_hooks_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".claude").join("hooks"))
}

fn get_ctx_read_script_path() -> Result<PathBuf, String> {
    Ok(get_claude_hooks_dir()?.join("llmwatcher-ctx-read.sh"))
}

fn get_claude_settings_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

fn read_claude_settings() -> Result<serde_json::Value, String> {
    let path = get_claude_settings_path()?;
    if path.exists() {
        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read settings.json: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings.json: {e}"))
    } else {
        Ok(serde_json::json!({}))
    }
}

fn write_claude_settings(settings: &serde_json::Value) -> Result<(), String> {
    let path = get_claude_settings_path()?;
    let hooks_dir = get_claude_hooks_dir()?;
    if !hooks_dir.exists() {
        fs::create_dir_all(&hooks_dir)
            .map_err(|e| format!("Failed to create ~/.claude/hooks directory: {e}"))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write settings.json: {e}"))?;
    Ok(())
}

// ============================================================================
// Install / Uninstall / Check
// ============================================================================

const HOOK_MARKER: &str = "llmwatcher-ctx-read";

#[tauri::command]
pub fn install_ctx_read_hook_claude() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

    // Ensure hooks directory
    let hooks_dir = get_claude_hooks_dir()?;
    if !hooks_dir.exists() {
        fs::create_dir_all(&hooks_dir)
            .map_err(|e| format!("Failed to create hooks directory: {e}"))?;
    }

    // Write the hook script
    let script_path = get_ctx_read_script_path()?;
    let script_content = generate_ctx_read_hook_script(port, "claude");
    fs::write(&script_path, &script_content)
        .map_err(|e| format!("Failed to write hook script: {e}"))?;

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&script_path)
            .map_err(|e| format!("Failed to get script metadata: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)
            .map_err(|e| format!("Failed to set script permissions: {e}"))?;
    }

    let script_path_str = script_path
        .to_str()
        .ok_or("Invalid script path")?
        .to_string();

    // Update settings.json
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

    if !hooks.contains_key("PreToolUse") {
        hooks.insert("PreToolUse".to_string(), serde_json::json!([]));
    }
    let pre_tool_use = hooks
        .get_mut("PreToolUse")
        .and_then(|v| v.as_array_mut())
        .ok_or("Failed to access PreToolUse array")?;

    // Remove any existing ctx_read entry, then re-add.
    pre_tool_use.retain(|entry| !is_ctx_read_entry(entry));

    pre_tool_use.push(serde_json::json!({
        "matcher": "Read",
        "hooks": [{
            "type": "command",
            "command": script_path_str
        }]
    }));

    // Enforce canonical ordering (DLP → ctx_read → compression).
    super::hook_ordering::enforce_pretooluse_order(hooks);

    write_claude_settings(&settings)?;

    Ok("File read caching hook installed for Claude Code".to_string())
}

#[tauri::command]
pub fn uninstall_ctx_read_hook_claude() -> Result<String, String> {
    // Remove script file
    let script_path = get_ctx_read_script_path()?;
    if script_path.exists() {
        let _ = fs::remove_file(&script_path);
    }

    // Remove from settings.json
    let mut settings = read_claude_settings()?;
    if let Some(obj) = settings.as_object_mut() {
        if let Some(hooks) = obj.get_mut("hooks").and_then(|v| v.as_object_mut()) {
            if let Some(pre_tool_use) = hooks.get_mut("PreToolUse").and_then(|v| v.as_array_mut()) {
                pre_tool_use.retain(|entry| !is_ctx_read_entry(entry));

                if pre_tool_use.is_empty() {
                    hooks.remove("PreToolUse");
                }
            }
            if hooks.is_empty() {
                obj.remove("hooks");
            }
        }
    }

    write_claude_settings(&settings)?;
    Ok("File read caching hook removed from Claude Code".to_string())
}

#[tauri::command]
pub fn check_ctx_read_hook_claude() -> Result<bool, String> {
    let script_path = get_ctx_read_script_path()?;
    if !script_path.exists() {
        return Ok(false);
    }

    let settings = read_claude_settings()?;
    let installed = settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().any(is_ctx_read_entry))
        .unwrap_or(false);

    Ok(installed)
}

// ============================================================================
// Helpers
// ============================================================================

fn is_ctx_read_entry(entry: &serde_json::Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter().any(|hook| {
                hook.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| s.contains(HOOK_MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}
