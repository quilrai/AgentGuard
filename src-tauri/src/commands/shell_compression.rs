use crate::PROXY_PORT;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ============================================================================
// Shared Hook Script Generation
// ============================================================================

/// Commands that the hooks will rewrite through shell compression.
/// Claude Code gets the full list; Cursor and Codex get subsets.
/// Full rewrite list covering all engine-supported command families.
/// Claude Code gets the broadest list including cat/head/tail/pytest/mypy.
const CLAUDE_REWRITE_COMMANDS: &[&str] = &[
    // VCS & collaboration
    "git", "gh",
    // Build tools
    "cargo", "make", "cmake", "ctest", "bazel", "blaze",
    // JS/TS ecosystem
    "npm", "pnpm", "yarn", "bun", "deno", "eslint", "prettier", "tsc",
    "vitest", "next", "vite", "playwright", "cypress",
    // Containers & orchestration
    "docker", "docker-compose", "kubectl", "helm",
    // Python
    "pip", "pip3", "ruff", "pytest", "mypy", "poetry", "uv",
    // Go
    "go", "golangci-lint", "golint",
    // Ruby
    "rubocop", "bundle", "rake", "rspec",
    // JVM
    "mvn", "gradle", "gradlew",
    // .NET
    "dotnet",
    // Mobile
    "flutter", "dart", "swift",
    // Other languages
    "zig", "mix", "iex", "composer",
    // Infrastructure
    "terraform", "ansible", "ansible-playbook", "aws",
    "systemctl", "journalctl",
    // Databases
    "prisma", "psql", "mysql", "mariadb",
    // Shell utilities
    "curl", "wget", "grep", "rg", "find", "ls",
    "cat", "head", "tail",
    // Environment
    "env", "printenv",
];

/// Cursor gets the same broad list minus a few agent-specific ones (cat/head/tail/pytest/mypy).
const CURSOR_REWRITE_COMMANDS: &[&str] = &[
    // VCS & collaboration
    "git", "gh",
    // Build tools
    "cargo", "make", "cmake", "ctest", "bazel", "blaze",
    // JS/TS ecosystem
    "npm", "pnpm", "yarn", "bun", "deno", "eslint", "prettier", "tsc",
    "vitest", "next", "vite", "playwright", "cypress",
    // Containers & orchestration
    "docker", "docker-compose", "kubectl", "helm",
    // Python
    "pip", "pip3", "ruff", "poetry", "uv",
    // Go
    "go", "golangci-lint", "golint",
    // Ruby
    "rubocop", "bundle", "rake", "rspec",
    // JVM
    "mvn", "gradle", "gradlew",
    // .NET
    "dotnet",
    // Mobile
    "flutter", "dart", "swift",
    // Other languages
    "zig", "mix", "iex", "composer",
    // Infrastructure
    "terraform", "ansible", "ansible-playbook", "aws",
    "systemctl", "journalctl",
    // Databases
    "prisma", "psql", "mysql", "mariadb",
    // Shell utilities
    "curl", "wget", "grep", "rg", "find", "ls",
    // Environment
    "env", "printenv",
];

/// Generate the compression hook shell script content.
/// This script reads the PreToolUse JSON from stdin, checks if the command
/// should be rewritten, and outputs the rewritten command as JSON.
///
/// The rewritten command is a bash snippet that:
/// 1. Curls the compression endpoint with the original command
/// 2. Writes the response body to a temp file while capturing the exit code header
/// 3. Prints the body and exits with the original command's exit code
fn generate_compression_hook_script(port: u16, commands: &[&str], backend_name: &str) -> String {
    // Build case patterns for bash: "git *|gh *|ls *|ls|env *|env|..."
    // Commands that can be invoked with no args need both "cmd *" and "cmd" patterns.
    // Commands that the engine supports when invoked with no arguments.
    // Must match the bare forms in shell_compression/patterns/mod.rs.
    let bare_ok = &[
        "ls", "env", "printenv", "make", "terraform", "eslint",
        "prettier", "vitest", "tsc", "ctest",
    ];
    let case_patterns: Vec<String> = commands.iter().flat_map(|cmd| {
        let mut pats = vec![format!("{}\\ *", cmd)];
        if bare_ok.contains(cmd) {
            pats.push(cmd.to_string());
        }
        pats
    }).collect();
    let case_line = case_patterns.join("|");

    format!(
        r#"#!/usr/bin/env bash
# LLMwatcher Shell Compression Hook
# Rewrites shell commands to route through the compression endpoint.
set -euo pipefail

INPUT=$(cat)

# Extract tool_name from the JSON input
TOOL=$(echo "$INPUT" | grep -o '"tool_name":"[^"]*"' | head -1 | cut -d'"' -f4)

# Only process Bash tool calls
if [ "$TOOL" != "Bash" ] && [ "$TOOL" != "bash" ]; then
  exit 0
fi

# Extract the command string
CMD=$(echo "$INPUT" | grep -o '"command":"[^"]*"' | head -1 | cut -d'"' -f4)

# Extract the top-level cwd from the hook payload (Claude Code includes it).
# Fall back to $(pwd) only if the payload didn't carry one.
PAYLOAD_CWD=$(echo "$INPUT" | grep -o '"cwd":"[^"]*"' | head -1 | cut -d'"' -f4)
EFFECTIVE_CWD="${{PAYLOAD_CWD:-$(pwd)}}"

# Don't rewrite if already going through compression
if echo "$CMD" | grep -qE "cli_compression|LLMWATCHER_ACTIVE"; then
  exit 0
fi

# Check if command matches our rewrite list
case "$CMD" in
  {case_line})
    # Escape command and cwd for JSON string values (\ -> \\, " -> \")
    ESCAPED_CMD=$(printf '%s' "$CMD" | sed 's/\\/\\\\/g; s/"/\\"/g')
    ESCAPED_CWD=$(printf '%s' "$EFFECTIVE_CWD" | sed 's/\\/\\\\/g; s/"/\\"/g')

    # Write JSON payload to a temp file to avoid quoting hell.
    _LMWH_PF=$(mktemp)
    printf '{{"command":"%s","cwd":"%s","backend":"{backend_name}"}}' "$ESCAPED_CMD" "$ESCAPED_CWD" > "$_LMWH_PF"

    # Also stash the raw command in a temp file so the rewrite can fall back
    # to executing it directly if the proxy is unreachable.
    _LMWH_CF=$(mktemp)
    printf '%s' "$CMD" > "$_LMWH_CF"

    # Build rewrite command that reads payload from the temp file.
    # $_LMWH_PF / $_LMWH_CF are expanded NOW (literal paths baked into REWRITE).
    # Variables prefixed with \$ are deferred for the rewrite's own execution.
    #
    # Failure handling:
    #   - curl failed (connect refused, DNS, stale port) -> run the original
    #     command via `bash $_LMWH_CF` so the agent never sees a phantom success.
    #   - HTTP 200 + X-Exit-Code header -> propagate the proxy's exit code.
    #   - Any other proxy response -> surface body + non-zero exit (do NOT
    #     re-run; the proxy already executed the command).
    REWRITE="set +e; _LMWH_T=\$(mktemp); _LMWH_HC=\$(curl -sS -o \"\$_LMWH_T\" -D \"\${{_LMWH_T}}.h\" -w '%{{http_code}}' -X POST -H 'Content-Type: application/json' -d @$_LMWH_PF http://localhost:{port}/cli_compression 2>/dev/null); _LMWH_CURL=\$?; if [ \$_LMWH_CURL -ne 0 ] || [ -z \"\$_LMWH_HC\" ] || [ \"\$_LMWH_HC\" = '000' ]; then echo '[llmwatcher: compression proxy unreachable, running raw command]' >&2; rm -f \"\$_LMWH_T\" \"\${{_LMWH_T}}.h\" $_LMWH_PF; bash $_LMWH_CF; _LMWH_RAW=\$?; rm -f $_LMWH_CF; exit \$_LMWH_RAW; fi; _LMWH_EC=\$(grep -i x-exit-code \"\${{_LMWH_T}}.h\" 2>/dev/null | tr -d '\\r' | cut -d' ' -f2); cat \"\$_LMWH_T\"; rm -f \"\$_LMWH_T\" \"\${{_LMWH_T}}.h\" $_LMWH_PF $_LMWH_CF; if [ \"\$_LMWH_HC\" = '200' ] && [ -n \"\$_LMWH_EC\" ]; then exit \$_LMWH_EC; fi; echo \"[llmwatcher: proxy returned HTTP \$_LMWH_HC]\" >&2; exit 1"

    # Escape the rewrite for JSON output (\ -> \\, " -> \")
    REWRITE_ESC=$(printf '%s' "$REWRITE" | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf '{{"command":"%s"}}' "$REWRITE_ESC"
    ;;
  *)
    exit 0
    ;;
esac
"#
    )
}

// ============================================================================
// Claude Code Compression Hook
// ============================================================================

fn get_claude_hooks_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".claude").join("hooks"))
}

fn get_claude_compression_script_path() -> Result<PathBuf, String> {
    Ok(get_claude_hooks_dir()?.join("llmwatcher-compress.sh"))
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

#[tauri::command]
pub fn install_compression_hook_claude() -> Result<String, String> {
    let port = *PROXY_PORT.lock().unwrap();

    // Ensure hooks directory exists
    let hooks_dir = get_claude_hooks_dir()?;
    if !hooks_dir.exists() {
        fs::create_dir_all(&hooks_dir)
            .map_err(|e| format!("Failed to create hooks directory: {}", e))?;
    }

    // Write the hook script
    let script_path = get_claude_compression_script_path()?;
    let script_content = generate_compression_hook_script(port, CLAUDE_REWRITE_COMMANDS, "claude");
    fs::write(&script_path, &script_content)
        .map_err(|e| format!("Failed to write hook script: {}", e))?;

    // Set executable permissions
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&script_path)
            .map_err(|e| format!("Failed to get script metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)
            .map_err(|e| format!("Failed to set script permissions: {}", e))?;
    }

    let script_path_str = script_path.to_str().ok_or("Invalid script path")?.to_string();

    // Update settings.json to add PreToolUse hook
    let mut settings = read_claude_settings()?;
    let obj = settings.as_object_mut()
        .ok_or("settings.json is not a valid JSON object")?;

    // Get or create hooks object
    if !obj.contains_key("hooks") {
        obj.insert("hooks".to_string(), serde_json::json!({}));
    }
    let hooks = obj.get_mut("hooks")
        .and_then(|v| v.as_object_mut())
        .ok_or("Failed to access hooks object")?;

    // Get or create PreToolUse array
    if !hooks.contains_key("PreToolUse") {
        hooks.insert("PreToolUse".to_string(), serde_json::json!([]));
    }
    let pre_tool_use = hooks.get_mut("PreToolUse")
        .and_then(|v| v.as_array_mut())
        .ok_or("Failed to access PreToolUse array")?;

    // Check if our hook is already installed
    let already_installed = pre_tool_use.iter().any(|entry| {
        entry.get("hooks")
            .and_then(|h| h.as_array())
            .map(|arr| arr.iter().any(|hook| {
                hook.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| s.contains("llmwatcher-compress"))
                    .unwrap_or(false)
            }))
            .unwrap_or(false)
    });

    if !already_installed {
        pre_tool_use.push(serde_json::json!({
            "matcher": "Bash|bash",
            "hooks": [{
                "type": "command",
                "command": script_path_str
            }]
        }));
    }

    write_claude_settings(&settings)?;

    Ok("Shell compression hook installed for Claude Code".to_string())
}

#[tauri::command]
pub fn uninstall_compression_hook_claude() -> Result<String, String> {
    // Remove script file
    let script_path = get_claude_compression_script_path()?;
    if script_path.exists() {
        let _ = fs::remove_file(&script_path);
    }

    // Remove from settings.json
    let mut settings = read_claude_settings()?;
    if let Some(obj) = settings.as_object_mut() {
        if let Some(hooks) = obj.get_mut("hooks").and_then(|v| v.as_object_mut()) {
            if let Some(pre_tool_use) = hooks.get_mut("PreToolUse").and_then(|v| v.as_array_mut()) {
                pre_tool_use.retain(|entry| {
                    !entry.get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|arr| arr.iter().any(|hook| {
                            hook.get("command")
                                .and_then(|c| c.as_str())
                                .map(|s| s.contains("llmwatcher-compress"))
                                .unwrap_or(false)
                        }))
                        .unwrap_or(false)
                });

                // Clean up empty PreToolUse array
                if pre_tool_use.is_empty() {
                    hooks.remove("PreToolUse");
                }
            }
            // Clean up empty hooks object
            if hooks.is_empty() {
                obj.remove("hooks");
            }
        }
    }

    write_claude_settings(&settings)?;

    Ok("Shell compression hook removed from Claude Code".to_string())
}

#[tauri::command]
pub fn check_compression_hook_claude() -> Result<bool, String> {
    let script_path = get_claude_compression_script_path()?;
    if !script_path.exists() {
        return Ok(false);
    }

    let settings = read_claude_settings()?;
    let installed = settings.get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().any(|entry| {
            entry.get("hooks")
                .and_then(|h| h.as_array())
                .map(|hooks| hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(|c| c.as_str())
                        .map(|s| s.contains("llmwatcher-compress"))
                        .unwrap_or(false)
                }))
                .unwrap_or(false)
        }))
        .unwrap_or(false);

    Ok(installed)
}

// ============================================================================
// Cursor Compression Hook
// ============================================================================

fn get_cursor_hooks_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".cursor"))
}

fn get_cursor_compression_script_path() -> Result<PathBuf, String> {
    Ok(get_cursor_hooks_dir()?.join("llmwatcher-compress.sh"))
}

fn get_cursor_hooks_json_path() -> Result<PathBuf, String> {
    Ok(get_cursor_hooks_dir()?.join("hooks.json"))
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct CursorHooksConfig {
    version: i32,
    hooks: HashMap<String, Vec<CursorHookEntry>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct CursorHookEntry {
    command: String,
}

#[tauri::command]
pub fn install_compression_hook_cursor() -> Result<String, String> {
    let port = *PROXY_PORT.lock().unwrap();

    // Ensure cursor directory exists
    let cursor_dir = get_cursor_hooks_dir()?;
    if !cursor_dir.exists() {
        fs::create_dir_all(&cursor_dir)
            .map_err(|e| format!("Failed to create ~/.cursor directory: {}", e))?;
    }

    // Write the hook script
    let script_path = get_cursor_compression_script_path()?;
    let script_content = generate_compression_hook_script(port, CURSOR_REWRITE_COMMANDS, "cursor-hooks");
    fs::write(&script_path, &script_content)
        .map_err(|e| format!("Failed to write hook script: {}", e))?;

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&script_path)
            .map_err(|e| format!("Failed to get script metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)
            .map_err(|e| format!("Failed to set script permissions: {}", e))?;
    }

    let script_path_str = script_path.to_str().ok_or("Invalid script path")?.to_string();

    // Update hooks.json
    let hooks_json_path = get_cursor_hooks_json_path()?;
    let mut config: CursorHooksConfig = if hooks_json_path.exists() {
        let content = fs::read_to_string(&hooks_json_path)
            .map_err(|e| format!("Failed to read hooks.json: {}", e))?;
        serde_json::from_str(&content).unwrap_or(CursorHooksConfig {
            version: 1,
            hooks: HashMap::new(),
        })
    } else {
        CursorHooksConfig {
            version: 1,
            hooks: HashMap::new(),
        }
    };

    if config.version == 0 {
        config.version = 1;
    }

    // Add to beforeShellExecution hook
    let hook_entry = CursorHookEntry {
        command: script_path_str.clone(),
    };

    let hook_list = config.hooks.entry("beforeShellExecution".to_string()).or_default();
    let already_exists = hook_list
        .iter()
        .any(|entry| entry.command.contains("llmwatcher-compress"));

    if !already_exists {
        hook_list.push(hook_entry);
    }

    let json_content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize hooks.json: {}", e))?;
    fs::write(&hooks_json_path, json_content)
        .map_err(|e| format!("Failed to write hooks.json: {}", e))?;

    Ok("Shell compression hook installed for Cursor".to_string())
}

#[tauri::command]
pub fn uninstall_compression_hook_cursor() -> Result<String, String> {
    // Remove script file
    let script_path = get_cursor_compression_script_path()?;
    if script_path.exists() {
        let _ = fs::remove_file(&script_path);
    }

    // Remove from hooks.json
    let hooks_json_path = get_cursor_hooks_json_path()?;
    if hooks_json_path.exists() {
        let content = fs::read_to_string(&hooks_json_path)
            .map_err(|e| format!("Failed to read hooks.json: {}", e))?;
        if let Ok(mut config) = serde_json::from_str::<CursorHooksConfig>(&content) {
            for (_hook_name, entries) in config.hooks.iter_mut() {
                entries.retain(|entry| !entry.command.contains("llmwatcher-compress"));
            }
            // Remove empty hook arrays
            config.hooks.retain(|_, entries| !entries.is_empty());

            let json_content = serde_json::to_string_pretty(&config)
                .map_err(|e| format!("Failed to serialize hooks.json: {}", e))?;
            fs::write(&hooks_json_path, json_content)
                .map_err(|e| format!("Failed to write hooks.json: {}", e))?;
        }
    }

    Ok("Shell compression hook removed from Cursor".to_string())
}

#[tauri::command]
pub fn check_compression_hook_cursor() -> Result<bool, String> {
    let script_path = get_cursor_compression_script_path()?;
    if !script_path.exists() {
        return Ok(false);
    }

    let hooks_json_path = get_cursor_hooks_json_path()?;
    if !hooks_json_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&hooks_json_path)
        .map_err(|e| format!("Failed to read hooks.json: {}", e))?;
    let config: CursorHooksConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse hooks.json: {}", e))?;

    let installed = config.hooks.values().any(|entries| {
        entries.iter().any(|entry| entry.command.contains("llmwatcher-compress"))
    });

    Ok(installed)
}

// ============================================================================
// Codex Compression Hook (instruction-based)
// ============================================================================

fn get_codex_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(PathBuf::from(home).join(".codex"))
}

fn get_codex_instructions_path() -> Result<PathBuf, String> {
    Ok(get_codex_dir()?.join("AGENTS.md"))
}

fn generate_codex_instructions(port: u16) -> String {
    format!(
        r#"# LLMwatcher Shell Compression

When running shell commands that produce verbose output, prefix them with the compression endpoint to save tokens:

```bash
curl -s -X POST -H 'Content-Type: application/json' -d '{{"command":"<your command>","cwd":"'$(pwd)'","backend":"codex"}}' http://localhost:{port}/cli_compression
```

Commands that benefit most from compression: git, cargo, npm, docker, kubectl, grep, find, ls, curl, pip, terraform, aws, and many more.

This is especially useful for commands that produce long output like `git log`, `git diff`, `cargo build`, `docker logs`, `kubectl describe`, etc.
"#
    )
}

#[tauri::command]
pub fn install_compression_hook_codex() -> Result<String, String> {
    let port = *PROXY_PORT.lock().unwrap();

    let codex_dir = get_codex_dir()?;
    if !codex_dir.exists() {
        fs::create_dir_all(&codex_dir)
            .map_err(|e| format!("Failed to create ~/.codex directory: {}", e))?;
    }

    let instructions_path = get_codex_instructions_path()?;
    let content = if instructions_path.exists() {
        let existing = fs::read_to_string(&instructions_path)
            .map_err(|e| format!("Failed to read AGENTS.md: {}", e))?;
        if existing.contains("LLMwatcher Shell Compression") {
            // Already installed, update in place
            let marker = "# LLMwatcher Shell Compression";
            if let Some(start) = existing.find(marker) {
                // Find the end of our section (next # header or end of file)
                let rest = &existing[start + marker.len()..];
                let end = rest.find("\n# ")
                    .map(|i| start + marker.len() + i)
                    .unwrap_or(existing.len());
                let mut new_content = existing[..start].to_string();
                new_content.push_str(&generate_codex_instructions(port));
                if end < existing.len() {
                    new_content.push_str(&existing[end..]);
                }
                new_content
            } else {
                existing
            }
        } else {
            format!("{}\n\n{}", existing.trim_end(), generate_codex_instructions(port))
        }
    } else {
        generate_codex_instructions(port)
    };

    fs::write(&instructions_path, content)
        .map_err(|e| format!("Failed to write AGENTS.md: {}", e))?;

    Ok("Shell compression instructions installed for Codex".to_string())
}

#[tauri::command]
pub fn uninstall_compression_hook_codex() -> Result<String, String> {
    let instructions_path = get_codex_instructions_path()?;
    if !instructions_path.exists() {
        return Ok("No Codex instructions to remove".to_string());
    }

    let content = fs::read_to_string(&instructions_path)
        .map_err(|e| format!("Failed to read AGENTS.md: {}", e))?;

    let marker = "# LLMwatcher Shell Compression";
    if let Some(start) = content.find(marker) {
        let rest = &content[start + marker.len()..];
        let end = rest.find("\n# ")
            .map(|i| start + marker.len() + i)
            .unwrap_or(content.len());

        let mut new_content = content[..start].trim_end().to_string();
        if end < content.len() {
            new_content.push_str(&content[end..]);
        }

        if new_content.trim().is_empty() {
            let _ = fs::remove_file(&instructions_path);
        } else {
            fs::write(&instructions_path, new_content)
                .map_err(|e| format!("Failed to write AGENTS.md: {}", e))?;
        }
    }

    Ok("Shell compression instructions removed from Codex".to_string())
}

#[tauri::command]
pub fn check_compression_hook_codex() -> Result<bool, String> {
    let instructions_path = get_codex_instructions_path()?;
    if !instructions_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&instructions_path)
        .map_err(|e| format!("Failed to read AGENTS.md: {}", e))?;

    Ok(content.contains("LLMwatcher Shell Compression"))
}
