// Codex CLI Hooks API Handlers
//
// Receivers for Codex CLI hooks (https://developers.openai.com/codex/hooks).
// Each route here is the receiver for a small bash forwarder script installed
// in `~/.codex/hooks/` that POSTs the JSON payload Codex writes to stdin.
//
// Hook coverage:
//   /user_prompt_submit  -- DLP scan + token-limit, blocks the prompt
//   /pre_bash            -- DLP scan command, blocks shell tool, logs tool call
//   /post_tool           -- updates the row created at PreToolUse with tool_response;
//                          for Codex shell compression, returns feedback JSON
//                          that replaces verbose Bash output after execution
//   /stop                -- closes the prompt row; if a transcript is available
//                          we try to extract real usage, otherwise we leave the
//                          estimated tokens minted at UserPromptSubmit time
//   /session_start       -- standalone log row capturing session_id, source, cwd
//
// Correlation:
//   - prompt-row keyed on Codex's `turn_id` when present (UserPromptSubmit and
//     Stop are both turn-scoped events that the Codex hooks docs list as
//     receiving `turn_id`). When `turn_id` is absent we fall back to a
//     synthesized `session_id:nanos` and let the Stop handler resolve "which
//     row to close" via `update_latest_agent_hook_with_usage`, which picks the
//     most recent row in this session that hasn't been closed yet
//     (`assistant_message_count = 0`).
//   - tool-row keyed on `tool_use_id` (PreToolUse -> PostToolUse).
//   Both flow through the shared DB methods `log_agent_hook_request` /
//   `update_agent_hook_output` with `backend = "codex-hooks"`.
//
// Endpoint labels distinguish row classes so the Stop query won't accidentally
// pick up a tool row in the same session:
//   - PROMPT_ENDPOINT  -> CodexPrompt   (UserPromptSubmit rows)
//   - TOOL_ENDPOINT    -> CodexTool     (PreToolUse / PostToolUse rows)
//   - SESSION_ENDPOINT -> CodexSession  (SessionStart rows)

use crate::database::{
    Database, RealUsage, DLP_ACTION_BLOCKED, DLP_ACTION_PASSED, DLP_ACTION_RATELIMITED,
};
use crate::dlp::{check_dlp_patterns, DlpDetection};
use crate::predefined_backend_settings::CustomBackendSettings;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

// ============================================================================
// Hook-specific Input Structures
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct UserPromptSubmitInput {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub prompt: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct PreToolUseInput {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub tool_name: String,
    pub tool_input: Value,
    #[serde(default)]
    pub tool_use_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct PostToolUseInput {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Value,
    #[serde(default)]
    pub tool_response: Value,
    #[serde(default)]
    pub tool_use_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct StopInput {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub stop_hook_active: bool,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct SessionStartInput {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

// ============================================================================
// Response Structures
// ============================================================================

/// Response shape for PreToolUse (Codex matches the Claude shape):
///   {"hookSpecificOutput": {
///       "hookEventName": "PreToolUse",
///       "permissionDecision": "allow"|"deny"|"ask",
///       "permissionDecisionReason": "..."}}
#[derive(Debug, Serialize)]
pub struct PreToolUseResponse {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: PreToolUseHookOutput,
}

#[derive(Debug, Serialize)]
pub struct PreToolUseHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    pub permission_decision: String,
    #[serde(
        rename = "permissionDecisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
}

/// Response shape for UserPromptSubmit:
///   {"decision": "block", "reason": "..."} to block, or empty object to allow.
#[derive(Debug, Serialize)]
pub struct UserPromptSubmitResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GenericResponse {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct PostToolUseReplacementResponse {
    #[serde(rename = "continue")]
    pub continue_: bool,
    pub decision: &'static str,
    pub reason: String,
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: PostToolUseReplacementHookOutput,
}

#[derive(Debug, Serialize)]
pub struct PostToolUseReplacementHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "additionalContext")]
    pub additional_context: &'static str,
}

// ============================================================================
// Extra Metadata for DB Storage
// ============================================================================

#[derive(Debug, Serialize)]
struct CodexHookMetadata {
    /// Joined-on by `log_agent_hook_request` / `update_agent_hook_output`.
    /// For prompt rows this is `session_id:nanos`; for tool rows it's `tool_use_id`.
    correlation_id: String,
    session_id: String,
    hook_event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

// ============================================================================
// State
// ============================================================================

#[derive(Clone)]
pub struct CodexHooksState {
    pub db: Database,
    pub settings: Arc<CustomBackendSettings>,
    pub http_client: reqwest::Client,
}

const BACKEND: &str = "codex-hooks";
const PROMPT_ENDPOINT: &str = "CodexPrompt";
const TOOL_ENDPOINT: &str = "CodexTool";
const SESSION_ENDPOINT: &str = "CodexSession";

/// Pick a stable per-turn correlation_id. Prefers Codex's `turn_id` (which the
/// hooks docs list as a common input field for turn-scoped events), and only
/// synthesizes a `session_id:nanos` fallback when it isn't present — the
/// fallback then forces Stop into the heuristic "latest open row in this
/// session" lookup, which can pick the wrong row if multiple turns are
/// concurrently open.
fn turn_correlation_id(session_id: &str, turn_id: Option<&str>) -> String {
    if let Some(t) = turn_id {
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}:{}", session_id, nanos)
}

// ============================================================================
// Helpers
// ============================================================================

fn count_words(text: &str) -> i32 {
    text.split_whitespace().count() as i32
}

fn estimate_tokens(text: &str) -> i32 {
    (count_words(text) as f32 * 1.5) as i32
}

fn format_detection_message(detections: &[DlpDetection]) -> String {
    let mut message = String::from("Blocked: Sensitive data detected:\n");
    for detection in detections {
        match (detection.absolute_line, detection.column) {
            (Some(line), Some(col)) => {
                message.push_str(&format!(
                    "- Line {}, col {}: {} — \"{}\"\n",
                    line, col, detection.pattern_name, detection.original_value
                ));
            }
            _ => {
                message.push_str(&format!(
                    "- {} ({}): \"{}\"\n",
                    detection.pattern_name, detection.pattern_type, detection.original_value
                ));
            }
        }
    }
    message
}

fn check_codex_token_limit(
    token_count: i32,
    settings: &CustomBackendSettings,
) -> (bool, Option<String>) {
    let max_tokens = settings.max_tokens_in_a_request;
    if max_tokens == 0 || token_count <= max_tokens as i32 {
        return (true, None);
    }
    (
        false,
        Some(format!(
            "Token limit exceeded: {} tokens (limit: {})",
            token_count, max_tokens
        )),
    )
}

fn json_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn json_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

#[derive(Debug, Clone)]
struct BashToolOutput {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
struct CodexPostToolCompression {
    replacement_output: String,
    original_tokens: usize,
    sent_tokens: usize,
    tokens_saved: usize,
}

fn json_text_value(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(json_text_value).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        Value::Object(obj) => obj
            .get("text")
            .or_else(|| obj.get("content"))
            .and_then(json_text_value),
        _ => None,
    }
}

fn json_text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(json_text_value)
}

fn json_i32_any(v: &Value, keys: &[&str]) -> Option<i32> {
    for key in keys {
        if let Some(n) = v.get(*key).and_then(|x| x.as_i64()) {
            return Some(n as i32);
        }
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            if let Ok(n) = s.parse::<i32>() {
                return Some(n);
            }
        }
    }
    for container in ["metadata", "result"] {
        if let Some(obj) = v.get(container) {
            if let Some(n) = json_i32_any(obj, keys) {
                return Some(n);
            }
        }
    }
    None
}

fn extract_bash_tool_output(tool_response: &Value) -> Option<BashToolOutput> {
    match tool_response {
        Value::String(s) => Some(BashToolOutput {
            stdout: s.clone(),
            stderr: String::new(),
            exit_code: None,
        }),
        Value::Object(_) => {
            let stdout = json_text(tool_response, "stdout")
                .or_else(|| json_text(tool_response, "output"))
                .or_else(|| json_text(tool_response, "text"))
                .or_else(|| json_text(tool_response, "content"))
                .unwrap_or_default();
            let stderr = json_text(tool_response, "stderr")
                .or_else(|| json_text(tool_response, "error"))
                .unwrap_or_default();
            if stdout.trim().is_empty() && stderr.trim().is_empty() {
                return None;
            }
            Some(BashToolOutput {
                stdout,
                stderr,
                exit_code: json_i32_any(
                    tool_response,
                    &[
                        "exit_code",
                        "exitCode",
                        "exit_status",
                        "status_code",
                        "code",
                    ],
                ),
            })
        }
        _ => None,
    }
}

fn should_consider_codex_bash_compression(command: &str) -> bool {
    !command.trim().is_empty() && !command.contains("cli_compression")
}

fn build_codex_post_tool_compression(
    settings: &CustomBackendSettings,
    input: &PostToolUseInput,
) -> Option<CodexPostToolCompression> {
    if input.tool_name != "Bash" || !settings.token_saving.shell_compression {
        return None;
    }

    let command = json_str(&input.tool_input, "command")?;
    if !should_consider_codex_bash_compression(&command) {
        return None;
    }

    let output = extract_bash_tool_output(&input.tool_response)?;
    let flags = crate::compression::AdvancedCompressionFlags::from(&settings.token_saving);
    let compressed = crate::shell_compression::compress_captured_output(
        &command,
        &output.stdout,
        &output.stderr,
        &flags,
    );

    if compressed.output.trim().is_empty()
        || compressed.compressed_tokens >= compressed.original_tokens
    {
        return None;
    }

    let exit_label = output
        .exit_code
        .map(|code| format!(", exit {code}"))
        .unwrap_or_default();
    let mut replacement_output = format!(
        "[AgentGuard: compressed Bash output{exit_label}; {}->{} tokens]\n{}",
        compressed.original_tokens,
        compressed.compressed_tokens,
        compressed.output.trim_end()
    );
    if let Some(code) = output.exit_code {
        if code != 0 && !replacement_output.contains("[exit:") {
            replacement_output.push_str(&format!("\n[exit: {code}]"));
        }
    }

    let sent_tokens = crate::shell_compression::tokens::count_tokens(&replacement_output);
    if sent_tokens >= compressed.original_tokens {
        return None;
    }

    Some(CodexPostToolCompression {
        replacement_output,
        original_tokens: compressed.original_tokens,
        sent_tokens,
        tokens_saved: compressed.original_tokens - sent_tokens,
    })
}

fn post_tool_replacement_response(output: String) -> PostToolUseReplacementResponse {
    PostToolUseReplacementResponse {
        continue_: false,
        decision: "block",
        reason: output,
        hook_specific_output: PostToolUseReplacementHookOutput {
            hook_event_name: "PostToolUse",
            additional_context:
                "AgentGuard replaced the completed Bash tool result with compressed output.",
        },
    }
}

fn parse_latest_turn_usage_from_text(content: &str) -> Option<RealUsage> {
    let mut usage = RealUsage::default();
    let mut found_any = false;
    let mut model: Option<String> = None;
    let mut stop_reason: Option<String> = None;

    for line in content.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ttype = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

        if ttype == "user" && found_any {
            break;
        }
        if ttype != "assistant" {
            continue;
        }
        let msg = match parsed.get("message") {
            Some(m) => m,
            None => continue,
        };
        if model.is_none() {
            model = json_str(msg, "model");
        }
        if stop_reason.is_none() {
            stop_reason = json_str(msg, "stop_reason");
        }
        if let Some(u) = msg.get("usage") {
            // Codex/OpenAI usage fields use `prompt_tokens` / `completion_tokens`
            // historically, but newer transcripts may match Anthropic's
            // `input_tokens` / `output_tokens`. Accept either.
            let input = json_i64(u, "input_tokens")
                .or_else(|| json_i64(u, "prompt_tokens"))
                .unwrap_or(0) as i32;
            let output = json_i64(u, "output_tokens")
                .or_else(|| json_i64(u, "completion_tokens"))
                .unwrap_or(0) as i32;
            usage.input_tokens += input;
            usage.output_tokens += output;
            usage.cache_read_tokens += json_i64(u, "cache_read_input_tokens").unwrap_or(0) as i32;
            usage.cache_creation_tokens +=
                json_i64(u, "cache_creation_input_tokens").unwrap_or(0) as i32;
            found_any = true;
        }
    }

    if !found_any {
        return None;
    }
    usage.model = model;
    usage.stop_reason = stop_reason;
    Some(usage)
}

fn read_tail_text(path: &str, max_bytes: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes as u64);

    file.seek(SeekFrom::Start(start)).ok()?;

    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;

    if start > 0 {
        if let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            buf.drain(..=pos);
        } else {
            return Some(String::new());
        }
    }

    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Try to read the latest assistant turn's usage out of a Codex transcript file.
/// Codex's transcript format is not yet stable; we make a best-effort attempt at
/// the same JSONL-style layout Claude Code uses (assistant lines with a `usage`
/// block) and fall back to `None` if nothing parses, in which case the prompt
/// row keeps the estimated tokens minted at UserPromptSubmit time.
fn read_latest_turn_usage(transcript_path: &str) -> Option<RealUsage> {
    const TAIL_WINDOWS: [usize; 4] = [256 * 1024, 1024 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024];

    let file_len = std::fs::metadata(transcript_path).ok()?.len() as usize;

    for max_bytes in TAIL_WINDOWS {
        let content = read_tail_text(transcript_path, max_bytes)?;
        if let Some(usage) = parse_latest_turn_usage_from_text(&content) {
            return Some(usage);
        }
        if file_len <= max_bytes {
            break;
        }
    }

    None
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /codex_hook/user_prompt_submit
async fn user_prompt_submit_handler(
    State(state): State<CodexHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: UserPromptSubmitInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CODEX_HOOK] user_prompt_submit parse error: {}", e);
            return (
                StatusCode::OK,
                Json(UserPromptSubmitResponse {
                    decision: None,
                    reason: None,
                }),
            );
        }
    };
    println!(
        "[CODEX_HOOK] user_prompt_submit - session: {}",
        input.session_id
    );

    let token_count = estimate_tokens(&input.prompt);
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Prefer Codex's turn_id so Stop can look up the exact row directly. We
    // fall back to session_id:nanos when turn_id is missing, in which case
    // Stop will resort to the latest-open-row-in-session heuristic.
    let correlation_id = turn_correlation_id(&input.session_id, input.turn_id.as_deref());

    let metadata = CodexHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: None,
        tool_use_id: None,
        turn_id: input.turn_id.clone(),
        source: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Token limit
    let (token_allowed, token_error) = check_codex_token_limit(token_count, &state.settings);
    if !token_allowed {
        let response = UserPromptSubmitResponse {
            decision: Some("block".to_string()),
            reason: token_error.clone(),
        };
        let response_body_json = serde_json::to_string(&response).unwrap_or_default();
        let _ = state.db.log_agent_hook_request(
            BACKEND,
            &correlation_id,
            PROMPT_ENDPOINT,
            input.model.as_deref().unwrap_or(""),
            token_count,
            0,
            &request_body_json,
            &response_body_json,
            429,
            metadata_json.as_deref(),
            None,
            None,
            DLP_ACTION_RATELIMITED,
        );
        return (StatusCode::OK, Json(response));
    }

    // DLP
    let detections = if state.settings.dlp_enabled {
        check_dlp_patterns(&input.prompt)
    } else {
        Vec::new()
    };
    let is_blocked = !detections.is_empty();

    let response = if is_blocked {
        UserPromptSubmitResponse {
            decision: Some("block".to_string()),
            reason: Some(format_detection_message(&detections)),
        }
    } else {
        UserPromptSubmitResponse {
            decision: None,
            reason: None,
        }
    };
    let response_body_json = serde_json::to_string(&response).unwrap_or_default();

    let response_status = if is_blocked { 403 } else { 200 };
    let dlp_action = if is_blocked {
        DLP_ACTION_BLOCKED
    } else {
        DLP_ACTION_PASSED
    };

    if let Ok(request_id) = state.db.log_agent_hook_request(
        BACKEND,
        &correlation_id,
        PROMPT_ENDPOINT,
        input.model.as_deref().unwrap_or(""),
        token_count,
        0,
        &request_body_json,
        &response_body_json,
        response_status,
        metadata_json.as_deref(),
        None,
        None,
        dlp_action,
    ) {
        if !detections.is_empty() {
            let _ = state.db.log_dlp_detections(request_id, &detections);
        }
    }

    (StatusCode::OK, Json(response))
}

/// Build the PreToolUse "deny" response. Only used for the blocked path —
/// Codex's allowed path expects empty stdout (exit 0, no JSON).
fn pre_tool_deny_response(reason: Option<String>) -> PreToolUseResponse {
    PreToolUseResponse {
        hook_specific_output: PreToolUseHookOutput {
            hook_event_name: "PreToolUse",
            permission_decision: "deny".to_string(),
            permission_decision_reason: reason,
        },
    }
}

/// Shared body for PreToolUse handlers (Bash only at the moment — Codex's hook
/// surface doesn't yet expose Read/Write/MCP). Computes token-limit /
/// DLP and logs the row keyed on `tool_use_id` (or a synthesized
/// session+tool key if absent).
/// Returns `Some(deny_response)` when blocked, `None` when allowed.
/// Codex expects empty stdout on the allowed path, so callers must
/// return an empty HTTP body when this returns `None`.
fn handle_pre_tool(
    state: &CodexHooksState,
    input: &PreToolUseInput,
    scanned_text: &str,
    log_as_tool_call: bool,
) -> Option<PreToolUseResponse> {
    let token_count = estimate_tokens(scanned_text);
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    let correlation_id = input
        .tool_use_id
        .clone()
        .unwrap_or_else(|| format!("{}-{}", input.session_id, input.tool_name));

    let metadata = CodexHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: Some(input.tool_name.clone()),
        tool_use_id: input.tool_use_id.clone(),
        turn_id: input.turn_id.clone(),
        source: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Token limit
    let (token_allowed, token_error) = check_codex_token_limit(token_count, &state.settings);
    if !token_allowed {
        let response = pre_tool_deny_response(token_error.clone());
        let response_body_json = serde_json::to_string(&response).unwrap_or_default();
        let _ = state.db.log_agent_hook_request(
            BACKEND,
            &correlation_id,
            TOOL_ENDPOINT,
            input.model.as_deref().unwrap_or(""),
            token_count,
            0,
            &request_body_json,
            &response_body_json,
            429,
            metadata_json.as_deref(),
            None,
            None,
            DLP_ACTION_RATELIMITED,
        );
        return Some(response);
    }

    // DLP
    let detections = if state.settings.dlp_enabled {
        check_dlp_patterns(scanned_text)
    } else {
        Vec::new()
    };
    let is_blocked = !detections.is_empty();

    let response = if is_blocked {
        Some(pre_tool_deny_response(Some(format_detection_message(
            &detections,
        ))))
    } else {
        None
    };
    let response_body_json = response
        .as_ref()
        .map(|r| serde_json::to_string(r).unwrap_or_default())
        .unwrap_or_else(|| "{}".to_string());

    let response_status = if is_blocked { 403 } else { 200 };
    let dlp_action = if is_blocked {
        DLP_ACTION_BLOCKED
    } else {
        DLP_ACTION_PASSED
    };

    if let Ok(request_id) = state.db.log_agent_hook_request(
        BACKEND,
        &correlation_id,
        TOOL_ENDPOINT,
        input.model.as_deref().unwrap_or(""),
        token_count,
        0,
        &request_body_json,
        &response_body_json,
        response_status,
        metadata_json.as_deref(),
        None,
        None,
        dlp_action,
    ) {
        if !detections.is_empty() {
            let _ = state.db.log_dlp_detections(request_id, &detections);
        }
        if log_as_tool_call {
            let tool_call = crate::requestresponsemetadata::ToolCall {
                id: correlation_id.clone(),
                name: input.tool_name.clone(),
                input: input.tool_input.clone(),
            };
            let _ = state.db.log_tool_calls(request_id, &[tool_call]);
        }
    }

    response
}

/// POST /codex_hook/pre_bash
/// Returns deny JSON when blocked, empty body when allowed (Codex expects no
/// stdout on the success path).
async fn pre_bash_handler(
    State(state): State<CodexHooksState>,
    Json(raw_json): Json<Value>,
) -> axum::response::Response {
    let input: PreToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CODEX_HOOK] pre_bash parse error: {}", e);
            // Fail-open: empty body so Codex proceeds.
            return (StatusCode::OK, "").into_response();
        }
    };
    let command = json_str(&input.tool_input, "command").unwrap_or_default();
    println!("[CODEX_HOOK] pre_bash - command: {}", command);
    let deny = handle_pre_tool(&state, &input, &command, true);
    if deny.is_some() {
        return (StatusCode::OK, Json(deny.unwrap())).into_response();
    }

    // Dependency protection: only block_malicious is actionable for Codex
    // (Codex expects empty stdout on allow, so we can't send info messages)
    let dep = &state.settings.dependency_protection;
    if dep.block_malicious_packages {
        let packages = crate::dep_protection::extract_packages_from_command_with_context(
            &command,
            input.cwd.as_deref(),
        );
        if !packages.is_empty() {
            let dep_result = crate::dep_protection::check_dependencies(
                &state.http_client,
                &packages,
                true,
                false, // inform not actionable for Codex
            )
            .await;
            if dep_result.should_block {
                return (
                    StatusCode::OK,
                    Json(pre_tool_deny_response(dep_result.block_reason)),
                )
                    .into_response();
            }
        }
    }

    (StatusCode::OK, "").into_response()
}

/// POST /codex_hook/post_tool
/// Updates the row created at PreToolUse with the tool response. Falls back to
/// creating a fresh row if PreToolUse never ran.
async fn post_tool_handler(
    State(state): State<CodexHooksState>,
    Json(raw_json): Json<Value>,
) -> axum::response::Response {
    let input: PostToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CODEX_HOOK] post_tool parse error: {}", e);
            return (StatusCode::OK, "").into_response();
        }
    };
    println!(
        "[CODEX_HOOK] post_tool - tool: {}, tool_use_id: {:?}",
        input.tool_name, input.tool_use_id
    );

    let correlation_id = input
        .tool_use_id
        .clone()
        .unwrap_or_else(|| format!("{}-{}", input.session_id, input.tool_name));

    let response_text = serde_json::to_string(&input.tool_response).unwrap_or_default();
    let compression = build_codex_post_tool_compression(&state.settings, &input);
    let db_response_text = compression
        .as_ref()
        .map(|c| c.replacement_output.as_str())
        .unwrap_or(response_text.as_str());
    let output_tokens = compression
        .as_ref()
        .map(|c| c.sent_tokens as i32)
        .unwrap_or_else(|| estimate_tokens(&response_text));

    let updated = state
        .db
        .update_agent_hook_output(
            BACKEND,
            &correlation_id,
            output_tokens,
            Some(db_response_text),
            None,
        )
        .ok()
        .unwrap_or(false);

    if !updated {
        let request_body_json = serde_json::to_string(&input.tool_input).unwrap_or_default();
        let metadata = CodexHookMetadata {
            correlation_id: correlation_id.clone(),
            session_id: input.session_id.clone(),
            hook_event_name: input.hook_event_name.clone(),
            cwd: input.cwd.clone(),
            transcript_path: input.transcript_path.clone(),
            tool_name: Some(input.tool_name.clone()),
            tool_use_id: input.tool_use_id.clone(),
            turn_id: input.turn_id.clone(),
            source: None,
        };
        let metadata_json = serde_json::to_string(&metadata).ok();

        if let Ok(request_id) = state.db.log_agent_hook_request(
            BACKEND,
            &correlation_id,
            TOOL_ENDPOINT,
            input.model.as_deref().unwrap_or(""),
            0,
            output_tokens,
            &request_body_json,
            db_response_text,
            200,
            metadata_json.as_deref(),
            None,
            None,
            DLP_ACTION_PASSED,
        ) {
            let tool_call = crate::requestresponsemetadata::ToolCall {
                id: correlation_id.clone(),
                name: input.tool_name.clone(),
                input: input.tool_input.clone(),
            };
            let _ = state.db.log_tool_calls(request_id, &[tool_call]);
        }
    }

    if let Some(compression) = compression.as_ref() {
        let meta_json = format!(
            "{{\"shell_compression\":{},\"original_tokens\":{},\"compressed_tokens\":{}}}",
            compression.tokens_saved, compression.original_tokens, compression.sent_tokens
        );
        let _ = state.db.update_agent_hook_token_saving(
            BACKEND,
            &correlation_id,
            compression.tokens_saved as i32,
            Some(&meta_json),
        );
    }

    // ---- Symbol extraction (best-effort) ----
    extract_symbols_for_tool(
        &state.db,
        &input.tool_name,
        &input.tool_input,
        input.cwd.as_deref(),
    );

    if let Some(compression) = compression {
        return (
            StatusCode::OK,
            Json(post_tool_replacement_response(
                compression.replacement_output,
            )),
        )
            .into_response();
    }

    (StatusCode::OK, "").into_response()
}

/// If the tool touched a file, extract symbols via tree-sitter (best-effort).
fn extract_symbols_for_tool(
    db: &crate::database::Database,
    tool_name: &str,
    tool_input: &Value,
    cwd: Option<&str>,
) {
    let is_file_tool = matches!(tool_name, "Read" | "Write" | "Edit" | "Bash");
    if !is_file_tool {
        return;
    }

    // For Bash tool, try to extract file paths from the command.
    let file_paths: Vec<String> = if tool_name == "Bash" {
        // Bash paths are extracted by the garden's extract_paths_from_bash,
        // but here we do a simpler check: look for paths in the command that
        // are supported extensions.
        let cmd = json_str(tool_input, "command").unwrap_or_default();
        let cwd_path = std::path::Path::new(cwd.unwrap_or("."));
        crate::symbols::paths_from_bash_for_symbols(&cmd, cwd_path)
    } else {
        json_str(tool_input, "file_path")
            .or_else(|| json_str(tool_input, "path"))
            .into_iter()
            .collect()
    };

    let cwd = match cwd {
        Some(c) => c,
        None => return,
    };

    for file_path in file_paths {
        if !crate::symbols::is_supported_extension(&file_path) {
            continue;
        }
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let symbols = crate::symbols::extract_symbols(&file_path, &content);
        if symbols.is_empty() {
            continue;
        }
        let rel_path = if file_path.starts_with(cwd) {
            file_path[cwd.len()..].trim_start_matches('/').to_string()
        } else {
            file_path.clone()
        };
        let _ = db.upsert_file_symbols(cwd, &rel_path, &symbols);
    }
}

/// POST /codex_hook/stop
/// Closes the prompt row. Routing depends on what's available:
///   1. If `turn_id` is present, look up the exact row created at
///      UserPromptSubmit time via `update_agent_hook_output`.
///         - With transcript usage: overwrite mode (real numbers replace
///           estimates, including input_tokens).
///         - Without transcript usage: additive mode (preserves the existing
///           estimated input_tokens, adds the estimated output tokens from
///           `last_assistant_message`, sets assistant_message_count = 1).
///   2. If `turn_id` is missing, fall back to the session-heuristic helpers
///      (`update_latest_agent_hook_with_usage` for the overwrite path,
///      `close_latest_agent_hook_row_additive` for the additive path).
async fn stop_handler(
    State(state): State<CodexHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: StopInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CODEX_HOOK] stop parse error: {}", e);
            return (
                StatusCode::OK,
                Json(GenericResponse {
                    status: "ok".to_string(),
                }),
            );
        }
    };
    println!(
        "[CODEX_HOOK] stop - session: {}, turn_id: {:?}",
        input.session_id, input.turn_id
    );

    let usage = if let Some(path) = input.transcript_path.as_ref() {
        // The transcript may be flushed slightly after Stop fires; one quick
        // retry covers the common race without blocking the agent.
        match read_latest_turn_usage(path) {
            Some(u) => Some(u),
            None => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                read_latest_turn_usage(path)
            }
        }
    } else {
        None
    };

    let turn_id = input.turn_id.as_deref().filter(|s| !s.is_empty());
    let estimated_output = input
        .last_assistant_message
        .as_deref()
        .map(estimate_tokens)
        .unwrap_or(0);
    let response_text = input.last_assistant_message.as_deref();

    let updated = match (turn_id, usage.as_ref()) {
        // Common path: turn_id known, transcript usage parsed.
        (Some(tid), Some(u)) => state
            .db
            .update_agent_hook_output(BACKEND, tid, 0, response_text, Some(u))
            .ok()
            .unwrap_or(false),
        // turn_id known, no transcript: additive mode preserves input_tokens.
        (Some(tid), None) => state
            .db
            .update_agent_hook_output(BACKEND, tid, estimated_output, response_text, None)
            .ok()
            .unwrap_or(false),
        // No turn_id, transcript present: heuristic overwrite (real numbers
        // are authoritative so clobbering estimates is fine).
        (None, Some(u)) => state
            .db
            .update_latest_agent_hook_with_usage(
                BACKEND,
                &input.session_id,
                PROMPT_ENDPOINT,
                u,
                response_text,
            )
            .ok()
            .unwrap_or(false),
        // No turn_id, no transcript: heuristic additive close that preserves
        // the input_tokens estimate from UserPromptSubmit.
        (None, None) => state
            .db
            .close_latest_agent_hook_row_additive(
                BACKEND,
                &input.session_id,
                PROMPT_ENDPOINT,
                estimated_output,
                response_text,
                input.model.as_deref(),
                Some("stop"),
            )
            .ok()
            .unwrap_or(false),
    };

    if !updated {
        println!(
            "[CODEX_HOOK] stop - no open prompt row found for session: {} turn_id: {:?} (was the UserPromptSubmit hook installed?)",
            input.session_id, input.turn_id
        );
    }

    (
        StatusCode::OK,
        Json(GenericResponse {
            status: "ok".to_string(),
        }),
    )
}

/// POST /codex_hook/session_start
/// Standalone log row capturing session metadata.
async fn session_start_handler(
    State(state): State<CodexHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: SessionStartInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CODEX_HOOK] session_start parse error: {}", e);
            return (
                StatusCode::OK,
                Json(GenericResponse {
                    status: "ok".to_string(),
                }),
            );
        }
    };
    println!(
        "[CODEX_HOOK] session_start - session: {}, source: {:?}",
        input.session_id, input.source
    );

    let request_body_json = serde_json::to_string(&input).unwrap_or_default();
    // Use session_id + ":start" so the prompt-row keyed on `session_id` doesn't
    // collide with this metadata row in the upsert path.
    let correlation_id = format!("{}:start", input.session_id);
    let metadata = CodexHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: None,
        tool_use_id: None,
        turn_id: None,
        source: input.source.clone(),
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    let _ = state.db.log_agent_hook_request(
        BACKEND,
        &correlation_id,
        SESSION_ENDPOINT,
        input.model.as_deref().unwrap_or(""),
        0,
        0,
        &request_body_json,
        "{}",
        200,
        metadata_json.as_deref(),
        None,
        None,
        DLP_ACTION_PASSED,
    );

    (
        StatusCode::OK,
        Json(GenericResponse {
            status: "ok".to_string(),
        }),
    )
}

// ============================================================================
// Router
// ============================================================================

pub fn create_codex_hooks_router(
    db: Database,
    settings: CustomBackendSettings,
    http_client: reqwest::Client,
) -> Router {
    let state = CodexHooksState {
        db,
        settings: Arc::new(settings),
        http_client,
    };

    Router::new()
        .route("/user_prompt_submit", post(user_prompt_submit_handler))
        .route("/pre_bash", post(pre_bash_handler))
        .route("/post_tool", post(post_tool_handler))
        .route("/stop", post(stop_handler))
        .route("/session_start", post(session_start_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predefined_backend_settings::TokenSavingSettings;
    use serde_json::json;

    fn post_tool_input(command: &str, stdout: String) -> PostToolUseInput {
        PostToolUseInput {
            session_id: "session".to_string(),
            transcript_path: None,
            cwd: Some("/tmp/project".to_string()),
            hook_event_name: "PostToolUse".to_string(),
            model: Some("gpt-test".to_string()),
            turn_id: Some("turn".to_string()),
            tool_name: "Bash".to_string(),
            tool_input: json!({ "command": command }),
            tool_response: json!({
                "stdout": stdout,
                "stderr": "",
                "exit_code": 0
            }),
            tool_use_id: Some("tool".to_string()),
        }
    }

    fn compression_settings(enabled: bool) -> CustomBackendSettings {
        CustomBackendSettings {
            token_saving: TokenSavingSettings {
                shell_compression: enabled,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn codex_post_tool_compresses_long_bash_output() {
        let stdout = (0..80)
            .map(|i| format!("line {i}: repeated build output with enough words to count"))
            .collect::<Vec<_>>()
            .join("\n");
        let input = post_tool_input("npm test", stdout);

        let compressed =
            build_codex_post_tool_compression(&compression_settings(true), &input).unwrap();

        assert!(compressed.replacement_output.contains("[AgentGuard:"));
        assert!(compressed.tokens_saved > 0);
        assert!(compressed.sent_tokens < compressed.original_tokens);
    }

    #[test]
    fn codex_post_tool_compression_respects_backend_setting() {
        let stdout = (0..80)
            .map(|i| format!("line {i}: repeated build output with enough words to count"))
            .collect::<Vec<_>>()
            .join("\n");
        let input = post_tool_input("npm test", stdout);

        let compressed = build_codex_post_tool_compression(&compression_settings(false), &input);

        assert!(compressed.is_none());
    }

    #[test]
    fn codex_post_tool_compresses_content_array_response() {
        let stdout = (0..80)
            .map(|i| format!("line {i}: repeated build output with enough words to count"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut input = post_tool_input("npm test", String::new());
        input.tool_response = json!({
            "content": [{ "type": "text", "text": stdout }],
            "metadata": { "exit_code": 1 }
        });

        let compressed =
            build_codex_post_tool_compression(&compression_settings(true), &input).unwrap();

        assert!(compressed.replacement_output.contains("exit 1"));
        assert!(compressed.replacement_output.contains("[exit: 1]"));
    }

    #[test]
    fn post_tool_replacement_uses_codex_feedback_shape() {
        let value =
            serde_json::to_value(post_tool_replacement_response("short".to_string())).unwrap();

        assert_eq!(value["continue"], false);
        assert_eq!(value["decision"], "block");
        assert_eq!(value["reason"], "short");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(
            value["hookSpecificOutput"]["additionalContext"],
            "AgentGuard replaced the completed Bash tool result with compressed output."
        );
    }
}
