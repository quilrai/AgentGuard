use crate::SERVER_PORT;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ============================================================================
// Shared Hook Script Generation
// ============================================================================

/// Commands that the hooks will rewrite through shell compression.
/// Claude Code gets the full list; Cursor gets a subset. Codex uses
/// PostToolUse result replacement in `codex_hooks.rs` instead of this
/// PreToolUse command-rewrite path.
/// Full rewrite list covering all engine-supported command families.
/// Claude Code gets the broadest list including cat/head/tail/pytest/mypy.
/// Bump this when `generate_compression_hook_script` changes materially.
const COMPRESSION_HOOK_VERSION_MARKER: &str = "LLMWATCHER_COMPRESS_HOOK_VERSION=3";

const CLAUDE_REWRITE_COMMANDS: &[&str] = &[
    // VCS & collaboration
    "git",
    "gh",
    // Build tools
    "cargo",
    "make",
    "cmake",
    "ctest",
    "bazel",
    "blaze",
    // JS/TS ecosystem
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "deno",
    "eslint",
    "prettier",
    "tsc",
    "vitest",
    "next",
    "vite",
    "playwright",
    "cypress",
    // Containers & orchestration
    "docker",
    "docker-compose",
    "kubectl",
    "helm",
    // Python
    "pip",
    "pip3",
    "ruff",
    "pytest",
    "mypy",
    "poetry",
    "uv",
    // Go
    "go",
    "golangci-lint",
    "golint",
    // Ruby
    "rubocop",
    "bundle",
    "rake",
    "rspec",
    // JVM
    "mvn",
    "gradle",
    "gradlew",
    // .NET
    "dotnet",
    // Mobile
    "flutter",
    "dart",
    "swift",
    // Other languages
    "zig",
    "mix",
    "iex",
    "composer",
    // Infrastructure
    "terraform",
    "ansible",
    "ansible-playbook",
    "aws",
    "systemctl",
    "journalctl",
    // Databases
    "prisma",
    "psql",
    "mysql",
    "mariadb",
    // Shell utilities
    "curl",
    "wget",
    "grep",
    "rg",
    "find",
    "ls",
    "cat",
    "head",
    "tail",
    // Environment
    "env",
    "printenv",
];

/// Cursor gets the same broad list minus a few agent-specific ones (cat/head/tail/pytest/mypy).
const CURSOR_REWRITE_COMMANDS: &[&str] = &[
    // VCS & collaboration
    "git",
    "gh",
    // Build tools
    "cargo",
    "make",
    "cmake",
    "ctest",
    "bazel",
    "blaze",
    // JS/TS ecosystem
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "deno",
    "eslint",
    "prettier",
    "tsc",
    "vitest",
    "next",
    "vite",
    "playwright",
    "cypress",
    // Containers & orchestration
    "docker",
    "docker-compose",
    "kubectl",
    "helm",
    // Python
    "pip",
    "pip3",
    "ruff",
    "poetry",
    "uv",
    // Go
    "go",
    "golangci-lint",
    "golint",
    // Ruby
    "rubocop",
    "bundle",
    "rake",
    "rspec",
    // JVM
    "mvn",
    "gradle",
    "gradlew",
    // .NET
    "dotnet",
    // Mobile
    "flutter",
    "dart",
    "swift",
    // Other languages
    "zig",
    "mix",
    "iex",
    "composer",
    // Infrastructure
    "terraform",
    "ansible",
    "ansible-playbook",
    "aws",
    "systemctl",
    "journalctl",
    // Databases
    "prisma",
    "psql",
    "mysql",
    "mariadb",
    // Shell utilities
    "curl",
    "wget",
    "grep",
    "rg",
    "find",
    "ls",
    // Environment
    "env",
    "printenv",
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
        "ls",
        "env",
        "printenv",
        "make",
        "terraform",
        "eslint",
        "prettier",
        "vitest",
        "tsc",
        "ctest",
    ];
    let case_patterns: Vec<String> = commands
        .iter()
        .flat_map(|cmd| {
            let mut pats = vec![format!("{}\\ *", cmd)];
            if bare_ok.contains(cmd) {
                pats.push(cmd.to_string());
            }
            pats
        })
        .collect();
    let case_line = case_patterns.join("|");
    let response_style = if backend_name == "cursor-hooks" {
        "cursor"
    } else {
        "claude"
    };

    format!(
        r#"#!/usr/bin/env bash
# LLMwatcher Shell Compression Hook
# {version_marker}
# Rewrites shell commands to route through the compression endpoint.
set -euo pipefail
umask 077

INPUT=$(cat)

# The hook is installed on pre-tool shell hooks: Claude Code uses PreToolUse
# and Cursor uses preToolUse. Parse the command payload robustly here. Using
# grep/cut breaks as soon as the command itself contains quoted strings
# (for example curl -H "...").
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

CMD=$(json_get "tool_input.command")
if [ -z "$CMD" ]; then
  # Cursor's shell-specific beforeShellExecution shape puts the command at the
  # root. We install modern Cursor compression on preToolUse, but this fallback
  # keeps stale or manually-wired hooks fail-open-compatible.
  CMD=$(json_get "command")
fi
if [ -z "$CMD" ]; then
  exit 0
fi

# Extract the top-level cwd from the hook payload (Claude Code includes it).
# Fall back to $(pwd) only if the payload didn't carry one.
PAYLOAD_CWD=$(json_get "cwd")
EFFECTIVE_CWD="${{PAYLOAD_CWD:-$(pwd)}}"

# Agent shell tools are Bash-compatible even when the user's login shell is fish
# or zsh. Use a POSIX shell for the proxied command by default, while allowing
# an explicit override for debugging or agent-specific needs.
default_exec_shell() {{
  if [ -x /bin/bash ]; then
    printf '%s' /bin/bash
    return
  fi
  if command -v bash >/dev/null 2>&1; then
    command -v bash
    return
  fi
  if [ -x /bin/sh ]; then
    printf '%s' /bin/sh
    return
  fi
  printf '%s' "${{SHELL:-/bin/sh}}"
}}
EFFECTIVE_SHELL="${{LLMWATCHER_SHELL:-$(default_exec_shell)}}"

# Don't rewrite if already going through compression
if echo "$CMD" | grep -qE "cli_compression|LLMWATCHER_ACTIVE"; then
  exit 0
fi

# Check if command matches our rewrite list
case "$CMD" in
  {case_line})
    # Write JSON payload to a temp file using a real JSON encoder. This keeps
    # embedded newlines intact as \n instead of collapsing `\` line-continuations
    # into `\ `, which breaks multiline curl commands under fish.
    _LMWH_PF=$(mktemp)
    perl -MJSON::PP -e '
      use strict;
      use warnings;

      my %env;
      for my $key (keys %ENV) {{
        my $value = $ENV{{$key}};
        next unless defined $value;
        my $ok = eval {{ JSON::PP::encode_json({{ $key => $value }}); 1 }};
        next unless $ok;
        $env{{$key}} = $value;
      }}

      my $payload = {{
        command => $ARGV[0],
        cwd => $ARGV[1],
        backend => $ARGV[2],
        shell => $ARGV[3],
        env => \%env,
      }};
      my $json = eval {{ JSON::PP::encode_json($payload) }};
      if (!$json) {{
        delete $payload->{{env}};
        $json = JSON::PP::encode_json($payload);
      }}
      print $json;
    ' "$CMD" "$EFFECTIVE_CWD" "{backend_name}" "$EFFECTIVE_SHELL" > "$_LMWH_PF"

    # Also stash the raw command in a temp file so the rewrite can fall back
    # to executing it directly if the proxy is unreachable, using the same
    # shell the proxy would have used.
    _LMWH_CF=$(mktemp)
    printf '%s' "$CMD" > "$_LMWH_CF"
    _LMWH_SF=$(mktemp)
    printf '%s' "$EFFECTIVE_SHELL" > "$_LMWH_SF"

    # Build rewrite command that reads payload from the temp file.
    # $_LMWH_PF / $_LMWH_CF are expanded NOW (literal paths baked into REWRITE).
    # Variables prefixed with \$ are deferred for the rewrite's own execution.
    #
    # Failure handling:
    #   - curl failed (connect refused, DNS, stale port) -> run the original
    #     command via the same shell so the agent never sees a phantom success.
    #   - HTTP 200 + X-Exit-Code header -> propagate the proxy's exit code.
    #   - Any other proxy response -> surface body + non-zero exit (do NOT
    #     re-run; the proxy already executed the command).
    REWRITE="set +e; _LMWH_T=\$(mktemp); _LMWH_HC=\$(curl -sS -o \"\$_LMWH_T\" -D \"\${{_LMWH_T}}.h\" -w '%{{http_code}}' -X POST -H 'Content-Type: application/json' --data-binary @$_LMWH_PF http://localhost:{port}/cli_compression 2>/dev/null); _LMWH_CURL=\$?; if [ \$_LMWH_CURL -ne 0 ] || [ -z \"\$_LMWH_HC\" ] || [ \"\$_LMWH_HC\" = '000' ]; then echo '[llmwatcher: compression proxy unreachable, running raw command]' >&2; _LMWH_SH=\$(cat \"$_LMWH_SF\" 2>/dev/null); if [ -z \"\$_LMWH_SH\" ]; then _LMWH_SH=/bin/bash; fi; rm -f \"\$_LMWH_T\" \"\${{_LMWH_T}}.h\" \"$_LMWH_PF\" \"$_LMWH_SF\"; \"\$_LMWH_SH\" \"$_LMWH_CF\"; _LMWH_RAW=\$?; rm -f \"$_LMWH_CF\"; exit \$_LMWH_RAW; fi; _LMWH_EC=\$(grep -i x-exit-code \"\${{_LMWH_T}}.h\" 2>/dev/null | tr -d '\\r' | cut -d' ' -f2); cat \"\$_LMWH_T\"; rm -f \"\$_LMWH_T\" \"\${{_LMWH_T}}.h\" \"$_LMWH_PF\" \"$_LMWH_CF\" \"$_LMWH_SF\"; if [ \"\$_LMWH_HC\" = '200' ] && [ -n \"\$_LMWH_EC\" ]; then exit \$_LMWH_EC; fi; echo \"[llmwatcher: proxy returned HTTP \$_LMWH_HC]\" >&2; exit 1"

    # Escape the rewrite for JSON output (\ -> \\, " -> \")
    REWRITE_ESC=$(printf '%s' "$REWRITE" | sed 's/\\/\\\\/g; s/"/\\"/g')
    if [ "{response_style}" = "cursor" ]; then
      printf '{{"permission":"allow","updated_input":{{"command":"%s"}}}}' "$REWRITE_ESC"
    else
      printf '{{"hookSpecificOutput":{{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{{"command":"%s"}}}}}}' "$REWRITE_ESC"
    fi
    ;;
  *)
    exit 0
    ;;
esac
"#,
        version_marker = COMPRESSION_HOOK_VERSION_MARKER,
        response_style = response_style,
    )
}

fn compression_script_is_current(path: &PathBuf, port: u16) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    content.contains(COMPRESSION_HOOK_VERSION_MARKER)
        && content.contains(&format!("http://localhost:{port}/cli_compression"))
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
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings.json: {}", e))
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
    fs::write(&path, content).map_err(|e| format!("Failed to write settings.json: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn install_compression_hook_claude() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

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

    let script_path_str = script_path
        .to_str()
        .ok_or("Invalid script path")?
        .to_string();

    // Update settings.json to add PreToolUse hook
    let mut settings = read_claude_settings()?;
    let obj = settings
        .as_object_mut()
        .ok_or("settings.json is not a valid JSON object")?;

    // Get or create hooks object
    if !obj.contains_key("hooks") {
        obj.insert("hooks".to_string(), serde_json::json!({}));
    }
    let hooks = obj
        .get_mut("hooks")
        .and_then(|v| v.as_object_mut())
        .ok_or("Failed to access hooks object")?;

    // Get or create PreToolUse array
    if !hooks.contains_key("PreToolUse") {
        hooks.insert("PreToolUse".to_string(), serde_json::json!([]));
    }
    let pre_tool_use = hooks
        .get_mut("PreToolUse")
        .and_then(|v| v.as_array_mut())
        .ok_or("Failed to access PreToolUse array")?;

    // Remove any existing compress hook entry so we can re-add a fresh one.
    pre_tool_use.retain(|entry| {
        let is_ours = entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|arr| {
                arr.iter().any(|hook| {
                    hook.get("command")
                        .and_then(|c| c.as_str())
                        .map(|s| s.contains("llmwatcher-compress"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        !is_ours
    });

    pre_tool_use.push(serde_json::json!({
        "matcher": "Bash|bash",
        "hooks": [{
            "type": "command",
            "command": script_path_str
        }]
    }));

    // Enforce canonical ordering (DLP → ctx_read → compression).
    {
        let hooks = obj
            .get_mut("hooks")
            .and_then(|v| v.as_object_mut())
            .ok_or("Failed to access hooks object")?;
        super::hook_ordering::enforce_pretooluse_order(hooks);
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
                    !entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|arr| {
                            arr.iter().any(|hook| {
                                hook.get("command")
                                    .and_then(|c| c.as_str())
                                    .map(|s| s.contains("llmwatcher-compress"))
                                    .unwrap_or(false)
                            })
                        })
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
    let installed = claude_compression_hook_installed()?;
    if installed {
        let port = *SERVER_PORT.lock().unwrap();
        let script_path = get_claude_compression_script_path()?;
        if !compression_script_is_current(&script_path, port) {
            install_compression_hook_claude()?;
        }
    }
    Ok(installed)
}

fn claude_compression_hook_installed() -> Result<bool, String> {
    let script_path = get_claude_compression_script_path()?;
    if !script_path.exists() {
        return Ok(false);
    }

    let settings = read_claude_settings()?;
    let installed = settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hooks| {
                        hooks.iter().any(|hook| {
                            hook.get("command")
                                .and_then(|c| c.as_str())
                                .map(|s| s.contains("llmwatcher-compress"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
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
    hooks: HashMap<String, Vec<serde_json::Value>>,
}

#[tauri::command]
pub fn install_compression_hook_cursor() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

    // Ensure cursor directory exists
    let cursor_dir = get_cursor_hooks_dir()?;
    if !cursor_dir.exists() {
        fs::create_dir_all(&cursor_dir)
            .map_err(|e| format!("Failed to create ~/.cursor directory: {}", e))?;
    }

    // Write the hook script
    let script_path = get_cursor_compression_script_path()?;
    let script_content =
        generate_compression_hook_script(port, CURSOR_REWRITE_COMMANDS, "cursor-hooks");
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

    let script_path_str = script_path
        .to_str()
        .ok_or("Invalid script path")?
        .to_string();

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

    // Remove old Cursor compression entries from any hook event first. Earlier
    // versions installed this on beforeShellExecution; current Cursor supports
    // command mutation through preToolUse + updated_input.
    for entries in config.hooks.values_mut() {
        entries.retain(|entry| !cursor_hook_entry_has_command(entry, "llmwatcher-compress"));
    }
    config.hooks.retain(|_, entries| !entries.is_empty());

    // Add to preToolUse with Shell matcher.
    let hook_entry = serde_json::json!({
        "command": script_path_str,
        "matcher": "Shell"
    });

    let hook_list = config.hooks.entry("preToolUse".to_string()).or_default();
    hook_list.push(hook_entry);

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
                entries
                    .retain(|entry| !cursor_hook_entry_has_command(entry, "llmwatcher-compress"));
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
    let installed = cursor_compression_hook_installed()?;
    if installed {
        let port = *SERVER_PORT.lock().unwrap();
        let script_path = get_cursor_compression_script_path()?;
        if !compression_script_is_current(&script_path, port)
            || !cursor_compression_hook_wiring_current()?
        {
            install_compression_hook_cursor()?;
        }
    }
    Ok(installed)
}

fn cursor_compression_hook_installed() -> Result<bool, String> {
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
    let config: CursorHooksConfig =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse hooks.json: {}", e))?;

    let installed = config.hooks.values().any(|entries| {
        entries
            .iter()
            .any(|entry| cursor_hook_entry_has_command(entry, "llmwatcher-compress"))
    });

    Ok(installed)
}

fn cursor_hook_entry_has_command(entry: &serde_json::Value, needle: &str) -> bool {
    entry
        .get("command")
        .and_then(|command| command.as_str())
        .map(|command| command.contains(needle))
        .unwrap_or(false)
}

fn cursor_compression_hook_wiring_current() -> Result<bool, String> {
    let hooks_json_path = get_cursor_hooks_json_path()?;
    if !hooks_json_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&hooks_json_path)
        .map_err(|e| format!("Failed to read hooks.json: {}", e))?;
    let config: CursorHooksConfig =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse hooks.json: {}", e))?;

    Ok(cursor_config_has_current_compression_wiring(&config))
}

fn cursor_config_has_current_compression_wiring(config: &CursorHooksConfig) -> bool {
    config
        .hooks
        .get("preToolUse")
        .map(|entries| {
            entries.iter().any(|entry| {
                cursor_hook_entry_has_command(entry, "llmwatcher-compress")
                    && entry.get("matcher").and_then(|m| m.as_str()) == Some("Shell")
            })
        })
        .unwrap_or(false)
}

pub fn migrate_installed_compression_hooks() -> Result<(), String> {
    let port = *SERVER_PORT.lock().unwrap();
    let mut errors = Vec::new();

    match claude_compression_hook_installed() {
        Ok(true) => match get_claude_compression_script_path() {
            Ok(path) if !compression_script_is_current(&path, port) => {
                if let Err(err) = install_compression_hook_claude() {
                    errors.push(format!("Claude: {err}"));
                }
            }
            Ok(_) => {}
            Err(err) => errors.push(format!("Claude: {err}")),
        },
        Ok(false) => {}
        Err(err) => errors.push(format!("Claude: {err}")),
    }

    match cursor_compression_hook_installed() {
        Ok(true) => match get_cursor_compression_script_path() {
            Ok(path) => match cursor_compression_hook_wiring_current() {
                Ok(wiring_current)
                    if !compression_script_is_current(&path, port) || !wiring_current =>
                {
                    if let Err(err) = install_compression_hook_cursor() {
                        errors.push(format!("Cursor: {err}"));
                    }
                }
                Ok(_) => {}
                Err(err) => errors.push(format!("Cursor: {err}")),
            },
            Err(err) => errors.push(format!("Cursor: {err}")),
        },
        Ok(false) => {}
        Err(err) => errors.push(format!("Cursor: {err}")),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{}_{}", name, nonce));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_script(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("llmwatcher-compress.sh");
        fs::write(&path, content).unwrap();

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
        }

        path
    }

    fn run_script_with_env(
        script: &Path,
        input: &Value,
        tmpdir: &Path,
        envs: &[(&str, &str)],
    ) -> Output {
        let input_bytes = serde_json::to_vec(input).unwrap();
        let mut command = Command::new("bash");
        command
            .arg(script)
            .env("TMPDIR", tmpdir)
            .env_remove("LLMWATCHER_SHELL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in envs {
            command.env(key, value);
        }

        let mut child = command.spawn().unwrap();

        child.stdin.take().unwrap().write_all(&input_bytes).unwrap();
        child.wait_with_output().unwrap()
    }

    fn run_script(script: &Path, input: &Value, tmpdir: &Path, shell: &str) -> Output {
        run_script_with_env(script, input, tmpdir, &[("LLMWATCHER_SHELL", shell)])
    }

    fn expected_default_exec_shell() -> String {
        if Path::new("/bin/bash").exists() {
            return "/bin/bash".to_string();
        }
        let output = Command::new("bash")
            .arg("-lc")
            .arg("command -v bash")
            .output();
        if let Ok(output) = output {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return path;
            }
        }
        if Path::new("/bin/sh").exists() {
            return "/bin/sh".to_string();
        }
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }

    fn rewrite_from_output(output: &Output) -> String {
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        if let Some(command) = response["hookSpecificOutput"]["updatedInput"]["command"].as_str() {
            return command.to_string();
        }
        response["updated_input"]["command"]
            .as_str()
            .unwrap_or_else(|| panic!("missing rewritten command in response: {response}"))
            .to_string()
    }

    fn response_from_output(output: &Output) -> Value {
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    #[test]
    fn generated_script_has_migration_marker() {
        let dir = temp_dir("compression_hook_marker");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let content = fs::read_to_string(&script).unwrap();
        assert!(content.contains(COMPRESSION_HOOK_VERSION_MARKER));
        assert!(compression_script_is_current(&script, 8123));
        assert!(!compression_script_is_current(&script, 8124));

        let _ = fs::remove_dir_all(&dir);
    }

    fn extract_payload_path(rewrite: &str) -> String {
        let marker = "--data-binary @";
        let start = rewrite.find(marker).unwrap() + marker.len();
        let end = rewrite[start..].find(' ').unwrap() + start;
        rewrite[start..end].to_string()
    }

    #[test]
    fn multiline_command_payload_round_trips_exactly() {
        let dir = temp_dir("compression_hook_multiline");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let command = r#"curl -sS -X POST "https://example.com/openai_compatible/v1/chat/completions" \
  -H "Authorization: Bearer TEST" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-5.4-nano",
    "messages": [
      {"role": "user", "content": "hello"}
    ]
  }' -w "\n---HTTP %{http_code}---\n" | head -c 10000"#;

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": command }
        });

        let output = run_script(&script, &input, &dir, "/bin/sh");
        let rewrite = rewrite_from_output(&output);
        assert!(rewrite.contains("--data-binary @"));

        let payload_path = extract_payload_path(&rewrite);
        let payload_raw = fs::read_to_string(&payload_path).unwrap();
        let payload: Value = serde_json::from_str(&payload_raw).unwrap();

        assert_eq!(payload["command"].as_str(), Some(command));
        assert_eq!(payload["cwd"].as_str(), Some("/tmp/project"));
        assert_eq!(payload["backend"].as_str(), Some("claude"));
        assert_eq!(payload["shell"].as_str(), Some("/bin/sh"));
        assert!(payload["env"]["PATH"].as_str().is_some());
        assert!(
            payload_raw.contains("\\n  -H"),
            "payload should JSON-escape command newlines: {payload_raw}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_login_shell_does_not_become_command_shell_by_default() {
        let dir = temp_dir("compression_hook_default_shell");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": "npm run build" }
        });

        let output = run_script_with_env(&script, &input, &dir, &[("SHELL", "/usr/bin/fish")]);
        let rewrite = rewrite_from_output(&output);
        let payload_path = extract_payload_path(&rewrite);
        let payload_raw = fs::read_to_string(&payload_path).unwrap();
        let payload: Value = serde_json::from_str(&payload_raw).unwrap();

        assert_eq!(
            payload["shell"].as_str(),
            Some(expected_default_exec_shell().as_str())
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn payload_forwards_agent_environment() {
        let dir = temp_dir("compression_hook_env");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": "npm run build" }
        });
        let inherited_path = std::env::var("PATH").unwrap_or_default();
        let agent_path = format!("/tmp/agent-node-bin:{inherited_path}");

        let output = run_script_with_env(
            &script,
            &input,
            &dir,
            &[("PATH", agent_path.as_str()), ("AGENT_ONLY_VAR", "present")],
        );
        let rewrite = rewrite_from_output(&output);
        let payload_path = extract_payload_path(&rewrite);
        let payload_raw = fs::read_to_string(&payload_path).unwrap();
        let payload: Value = serde_json::from_str(&payload_raw).unwrap();

        assert_eq!(payload["env"]["PATH"].as_str(), Some(agent_path.as_str()));
        assert_eq!(payload["env"]["AGENT_ONLY_VAR"].as_str(), Some("present"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn proxy_commands_are_not_rewritten() {
        let dir = temp_dir("compression_hook_proxy_guard");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": "curl -sS http://localhost:8123/cli_compression" }
        });

        let output = run_script(&script, &input, &dir, "/bin/sh");
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_matching_commands_are_not_rewritten() {
        let dir = temp_dir("compression_hook_non_match");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": "echo hello" }
        });

        let output = run_script(&script, &input, &dir, "/bin/sh");
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bare_rewrite_commands_still_match_without_arguments() {
        let dir = temp_dir("compression_hook_bare");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CLAUDE_REWRITE_COMMANDS, "claude"),
        );

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": "ls" }
        });

        let output = run_script(&script, &input, &dir, "/bin/sh");
        let rewrite = rewrite_from_output(&output);

        assert!(rewrite.contains("http://localhost:8123/cli_compression"));
        assert!(rewrite.contains("compression proxy unreachable, running raw command"));
        assert!(rewrite.contains("x-exit-code"));

        let _ = fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------------------
    // Real-world multi-line / tricky command payload round-trip tests.
    // These guarantee that whatever the agent sends us actually arrives
    // at /cli_compression byte-for-byte, regardless of quoting, newlines,
    // or shell variant.
    // ------------------------------------------------------------------

    fn payload_round_trips(
        command: &str,
        shell: &str,
        backend_commands: &[&str],
        backend_name: &str,
    ) {
        let dir = temp_dir(&format!("compression_hook_rt_{backend_name}"));
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, backend_commands, backend_name),
        );

        let input = json!({
            "tool_name": "Bash",
            "cwd": "/tmp/project",
            "tool_input": { "command": command }
        });

        let output = run_script(&script, &input, &dir, shell);
        let rewrite = rewrite_from_output(&output);
        assert!(
            rewrite.contains("--data-binary @"),
            "rewrite missing --data-binary: {rewrite}"
        );

        let payload_path = extract_payload_path(&rewrite);
        let payload_raw = fs::read_to_string(&payload_path).unwrap();
        let payload: Value = serde_json::from_str(&payload_raw)
            .unwrap_or_else(|e| panic!("payload is not valid JSON: {e}\n{payload_raw}"));

        assert_eq!(
            payload["command"].as_str(),
            Some(command),
            "command mismatch\npayload: {payload_raw}"
        );
        assert_eq!(payload["backend"].as_str(), Some(backend_name));
        assert_eq!(payload["shell"].as_str(), Some(shell));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiline_rg_with_many_flags_round_trips() {
        let command = r#"rg --type rust \
  --glob '!target/*' \
  --glob '!node_modules/*' \
  -n --column \
  -C 2 \
  'fn\s+compress_output' \
  src/ tests/"#;
        payload_round_trips(command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn multiline_heredoc_psql_round_trips() {
        let command = "psql -U postgres -d app <<'SQL'\nSELECT id, email, created_at\nFROM users\nWHERE email LIKE '%@example.com'\n  AND created_at > now() - interval '7 days'\nORDER BY created_at DESC\nLIMIT 50;\nSQL";
        payload_round_trips(command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn curl_with_jq_pipeline_round_trips() {
        let command = r#"curl -sS "https://api.github.com/repos/anthropics/claude-code/issues?state=open" \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  | jq '[.[] | {n: .number, t: .title, u: .user.login}]' \
  | head -n 40"#;
        payload_round_trips(command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn command_with_shell_metachars_round_trips() {
        // $, backticks, \n inside single quotes, globs, process substitution.
        let command = "grep -rn \"$PATTERN\" . | awk 'NR==1 {print $0; next} {printf \"%s\\n\", $0}' | tee /tmp/out.txt";
        payload_round_trips(command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn command_with_embedded_double_quotes_and_backslashes_round_trips() {
        // Backslashes + nested quotes in -d body — the original bug was sed
        // escape lossiness exactly here.
        let command = r#"curl -X POST http://localhost:3000/api \
  -H "Content-Type: application/json" \
  -d "{\"path\":\"C:\\\\Users\\\\me\",\"msg\":\"hello \\\"world\\\"\"}""#;
        payload_round_trips(command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn fish_shell_multiline_curl_round_trips() {
        // The perl-JSON fix was motivated by fish — keep a guard.
        let command = r#"curl -sS -X POST "https://example.com/v1/completions" \
  -H "Authorization: Bearer TEST" \
  -H "Content-Type: application/json" \
  -d '{"model":"x","messages":[{"role":"user","content":"hi"}]}'"#;
        payload_round_trips(command, "/usr/bin/fish", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn cursor_backend_round_trips_multiline_command() {
        let command = r#"docker run --rm \
  -v "$PWD":/work \
  -w /work \
  python:3.12-slim \
  python -c 'import json,sys; print(json.dumps({"ok": True}))'"#;
        payload_round_trips(command, "/bin/sh", CURSOR_REWRITE_COMMANDS, "cursor-hooks");
    }

    #[test]
    fn cursor_script_uses_updated_input_shape() {
        let dir = temp_dir("compression_hook_cursor_shape");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CURSOR_REWRITE_COMMANDS, "cursor-hooks"),
        );

        let input = json!({
            "tool_name": "Shell",
            "cwd": "/tmp/project",
            "tool_input": { "command": "npm test" }
        });

        let output = run_script(&script, &input, &dir, "/bin/sh");
        let response = response_from_output(&output);

        assert_eq!(response["permission"].as_str(), Some("allow"));
        assert!(response["updated_input"]["command"]
            .as_str()
            .unwrap()
            .contains("http://localhost:8123/cli_compression"));
        assert!(response.get("hookSpecificOutput").is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursor_wiring_requires_pre_tool_use_shell_matcher() {
        let mut config = CursorHooksConfig {
            version: 1,
            hooks: HashMap::new(),
        };
        config.hooks.insert(
            "beforeShellExecution".to_string(),
            vec![serde_json::json!({
                "command": "/tmp/llmwatcher-compress.sh"
            })],
        );
        assert!(!cursor_config_has_current_compression_wiring(&config));

        config.hooks.insert(
            "preToolUse".to_string(),
            vec![serde_json::json!({
                "command": "/tmp/llmwatcher-compress.sh",
                "matcher": "Shell"
            })],
        );
        assert!(cursor_config_has_current_compression_wiring(&config));
    }

    #[test]
    fn cursor_script_accepts_legacy_root_command_payload() {
        let dir = temp_dir("compression_hook_cursor_root_command");
        let script = write_script(
            &dir,
            &generate_compression_hook_script(8123, CURSOR_REWRITE_COMMANDS, "cursor-hooks"),
        );

        let input = json!({
            "hook_event_name": "beforeShellExecution",
            "cwd": "/tmp/project",
            "command": "npm test"
        });

        let output = run_script(&script, &input, &dir, "/bin/sh");
        let response = response_from_output(&output);

        assert_eq!(response["permission"].as_str(), Some("allow"));
        assert!(response["updated_input"]["command"]
            .as_str()
            .unwrap()
            .contains("http://localhost:8123/cli_compression"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn large_multiline_command_round_trips() {
        // 2+ KB command to catch any size-related bugs in the pipeline.
        let body: String = (0..40)
            .map(|i| format!("    \"key_{i:03}\": \"value with some text number {i}\""))
            .collect::<Vec<_>>()
            .join(",\n");
        let command = format!(
            "curl -sS -X POST https://api.example.com/bulk \\\n  -H 'Content-Type: application/json' \\\n  -d '{{\n{body}\n  }}'"
        );
        payload_round_trips(&command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }

    #[test]
    fn command_with_literal_tabs_round_trips() {
        // Real tabs in args (e.g. awk -F'\t') must survive JSON encoding.
        // Use `grep` as the first token so the command matches the rewrite set.
        let command = "grep -P '\tERROR\t' /var/log/app.log | cut -f 1,3 | sort -u";
        payload_round_trips(command, "/bin/sh", CLAUDE_REWRITE_COMMANDS, "claude");
    }
}
