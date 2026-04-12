// Claude Code Hooks API Handlers
//
// Implements endpoints for Claude Code hooks (the equivalent of cursor_hooks.rs).
// Each route here is the receiver for a small bash forwarder script installed in
// `~/.claude/hooks/` that POSTs the JSON payload Claude Code writes to stdin.
//
// Hook coverage:
//   /user_prompt_submit  -- DLP scan + token-limit, blocks the prompt
//   /pre_bash            -- DLP scan command, blocks shell tool, logs tool call
//   /pre_read            -- DLP scan file slice (respects offset/limit), blocks Read
//   /pre_write           -- DLP scan content/new_string for Write/Edit/MultiEdit/NotebookEdit
//   /pre_mcp             -- DLP scan MCP tool input, blocks
//   /post_tool           -- updates the row created at PreToolUse with tool_response
//   /stop                -- reads the transcript JSONL and updates the prompt row
//                          with real Anthropic-API usage (input/output/cache tokens)
//   /session_start       -- standalone log row capturing session_id, source, cwd
//   /session_end         -- standalone log row marking session end
//
// Correlation:
//   - prompt-row keyed on a unique-per-turn `correlation_id` we mint at
//     UserPromptSubmit time (`session_id:nanos`). Claude Code does not give
//     us a stable per-turn ID, so the Stop handler resolves "which row to
//     close" via `update_latest_agent_hook_with_usage`, which picks the most
//     recent row in this session that hasn't been closed yet
//     (`assistant_message_count = 0`). `session_id` lives in `extra_metadata`
//     for filtering.
//   - tool-row   keyed on `tool_use_id`    (PreToolUse -> PostToolUse)
//   Both flow through the shared DB methods `log_agent_hook_request` /
//   `update_agent_hook_output` with `backend = "claude-hooks"`.
//
// Endpoint labels distinguish row classes so the Stop query won't accidentally
// pick up a tool row in the same session:
//   - PROMPT_ENDPOINT  -> ClaudePrompt   (UserPromptSubmit rows)
//   - TOOL_ENDPOINT    -> ClaudeTool     (PreToolUse / PostToolUse rows)
//   - SESSION_ENDPOINT -> ClaudeSession  (SessionStart / SessionEnd rows)

use crate::database::{
    Database, RealUsage, DLP_ACTION_BLOCKED, DLP_ACTION_PASSED, DLP_ACTION_RATELIMITED,
};
use crate::dlp::{check_dlp_patterns, DlpDetection};
use crate::predefined_backend_settings::CustomBackendSettings;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ============================================================================
// Common Input Fields (present in every Claude Code hook payload)
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CommonHookFields {
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub permission_mode: Option<String>,
    pub hook_event_name: String,
}

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
    #[serde(default)]
    pub permission_mode: Option<String>,
    pub hook_event_name: String,
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
    #[serde(default)]
    pub permission_mode: Option<String>,
    pub hook_event_name: String,
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
    #[serde(default)]
    pub permission_mode: Option<String>,
    pub hook_event_name: String,
    pub tool_name: String,
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
    #[serde(default)]
    pub permission_mode: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub stop_hook_active: bool,
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
    pub source: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct SessionEndInput {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub hook_event_name: String,
    #[serde(default)]
    pub matcher_value: Option<String>,
}

// ============================================================================
// Response Structures
// ============================================================================

/// Response shape for PreToolUse (Claude Code spec):
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

// ============================================================================
// Extra Metadata for DB Storage
// ============================================================================

#[derive(Debug, Serialize)]
struct ClaudeHookMetadata {
    /// Joined-on by `log_agent_hook_request` / `update_agent_hook_output`.
    /// For prompt rows this is `session_id`; for tool rows it's `tool_use_id`.
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
    file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

// ============================================================================
// State
// ============================================================================

#[derive(Clone)]
pub struct ClaudeHooksState {
    pub db: Database,
    pub settings: Arc<CustomBackendSettings>,
    pub ctx_read_cache: Option<Arc<Mutex<crate::ctx_read::cache::SessionCache>>>,
}

const BACKEND: &str = "claude-hooks";
const PROMPT_ENDPOINT: &str = "ClaudePrompt";
const TOOL_ENDPOINT: &str = "ClaudeTool";
const SESSION_ENDPOINT: &str = "ClaudeSession";

/// Mint a unique-per-turn correlation_id from the session_id and a
/// monotonic-ish nanos suffix. Used by UserPromptSubmit so each turn gets its
/// own DB row.
fn new_turn_correlation_id(session_id: &str) -> String {
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
        message.push_str(&format!(
            "- {} ({}): \"{}\"\n",
            detection.pattern_name, detection.pattern_type, detection.original_value
        ));
    }
    message
}

fn check_claude_token_limit(
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

/// Extract a `String` field from a JSON object if present.
fn json_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn json_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

/// Read a slice of a file (honoring offset and limit, both line-based, matching
/// Claude Code's Read tool semantics). Returns `None` if the file can't be read.
fn read_file_slice(file_path: &str, offset: Option<i64>, limit: Option<i64>) -> Option<String> {
    let content = std::fs::read_to_string(file_path).ok()?;
    if offset.is_none() && limit.is_none() {
        return Some(content);
    }
    let start = offset.unwrap_or(0).max(0) as usize;
    let take = limit.map(|l| l.max(0) as usize);
    let mut out = String::new();
    let mut count = 0usize;
    for (i, line) in content.lines().enumerate() {
        if i < start {
            continue;
        }
        if let Some(t) = take {
            if count >= t {
                break;
            }
        }
        out.push_str(line);
        out.push('\n');
        count += 1;
    }
    Some(out)
}

/// For Write/Edit/MultiEdit/NotebookEdit, return the *new content the agent is
/// about to commit*, concatenated for DLP scanning.
fn extract_write_content(tool_name: &str, tool_input: &Value) -> String {
    match tool_name {
        "Write" => json_str(tool_input, "content").unwrap_or_default(),
        "Edit" => json_str(tool_input, "new_string").unwrap_or_default(),
        "MultiEdit" => {
            let mut buf = String::new();
            if let Some(arr) = tool_input.get("edits").and_then(|v| v.as_array()) {
                for e in arr {
                    if let Some(s) = json_str(e, "new_string") {
                        buf.push_str(&s);
                        buf.push('\n');
                    }
                }
            }
            buf
        }
        "NotebookEdit" => json_str(tool_input, "new_source").unwrap_or_default(),
        _ => String::new(),
    }
}

/// Try to read the latest assistant turn's usage out of the transcript JSONL.
/// Walks lines from the end backwards, summing usage across all assistant
/// messages until we hit a `type: user` line (= start of the current turn).
/// Returns `None` if the file can't be read or no assistant message is found.
fn read_latest_turn_usage(transcript_path: &str) -> Option<RealUsage> {
    let content = std::fs::read_to_string(transcript_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    let mut usage = RealUsage::default();
    let mut found_any = false;
    let mut model: Option<String> = None;
    let mut stop_reason: Option<String> = None;

    for line in lines.iter().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ttype = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Stop walking once we cross into the prior user turn.
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
            usage.input_tokens += json_i64(u, "input_tokens").unwrap_or(0) as i32;
            usage.output_tokens += json_i64(u, "output_tokens").unwrap_or(0) as i32;
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

// ============================================================================
// Handlers
// ============================================================================

/// POST /claude_hook/user_prompt_submit
async fn user_prompt_submit_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: UserPromptSubmitInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] user_prompt_submit parse error: {}", e);
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
        "[CLAUDE_HOOK] user_prompt_submit - session: {}",
        input.session_id
    );

    let token_count = estimate_tokens(&input.prompt);
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Each turn gets its own row. The Stop handler will resolve "which row to
    // close" via update_latest_agent_hook_with_usage(session_id, ...) since
    // Claude Code doesn't expose a per-turn ID.
    let correlation_id = new_turn_correlation_id(&input.session_id);

    let metadata = ClaudeHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: None,
        tool_use_id: None,
        file_path: None,
        source: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Token limit
    let (token_allowed, token_error) = check_claude_token_limit(token_count, &state.settings);
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
            "",
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
        "",
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

/// Build the standard PreToolUse "deny" or "allow" response.
fn pre_tool_response(blocked: bool, reason: Option<String>) -> PreToolUseResponse {
    PreToolUseResponse {
        hook_specific_output: PreToolUseHookOutput {
            hook_event_name: "PreToolUse",
            permission_decision: if blocked {
                "deny".to_string()
            } else {
                "allow".to_string()
            },
            permission_decision_reason: reason,
        },
    }
}

/// Shared body for PreToolUse handlers (Bash / Read / Write / MCP). Computes
/// token-limit / DLP and returns the right response, while logging
/// the row keyed on `tool_use_id` (or session_id if absent).
#[allow(clippy::too_many_arguments)]
fn handle_pre_tool(
    state: &ClaudeHooksState,
    input: &PreToolUseInput,
    scanned_text: &str,
    file_path: Option<String>,
    log_as_tool_call: bool,
) -> PreToolUseResponse {
    let token_count = estimate_tokens(scanned_text);
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    let correlation_id = input
        .tool_use_id
        .clone()
        .unwrap_or_else(|| format!("{}-{}", input.session_id, input.tool_name));

    let metadata = ClaudeHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: Some(input.tool_name.clone()),
        tool_use_id: input.tool_use_id.clone(),
        file_path,
        source: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Token limit
    let (token_allowed, token_error) = check_claude_token_limit(token_count, &state.settings);
    if !token_allowed {
        let response = pre_tool_response(true, token_error.clone());
        let response_body_json = serde_json::to_string(&response).unwrap_or_default();
        let _ = state.db.log_agent_hook_request(
            BACKEND,
            &correlation_id,
            TOOL_ENDPOINT,
            "",
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
        return response;
    }

    // DLP
    let detections = if state.settings.dlp_enabled {
        check_dlp_patterns(scanned_text)
    } else {
        Vec::new()
    };
    let is_blocked = !detections.is_empty();

    let response = if is_blocked {
        pre_tool_response(true, Some(format_detection_message(&detections)))
    } else {
        pre_tool_response(false, None)
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
        TOOL_ENDPOINT,
        "",
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

/// POST /claude_hook/pre_bash
async fn pre_bash_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: PreToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] pre_bash parse error: {}", e);
            return (StatusCode::OK, Json(pre_tool_response(false, None)));
        }
    };
    let command = json_str(&input.tool_input, "command").unwrap_or_default();
    println!("[CLAUDE_HOOK] pre_bash - command: {}", command);
    let response = handle_pre_tool(&state, &input, &command, None, true);
    (StatusCode::OK, Json(response))
}

/// POST /claude_hook/pre_read
async fn pre_read_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: PreToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] pre_read parse error: {}", e);
            return (StatusCode::OK, Json(pre_tool_response(false, None)));
        }
    };
    let file_path = json_str(&input.tool_input, "file_path").unwrap_or_default();
    let offset = json_i64(&input.tool_input, "offset");
    let limit = json_i64(&input.tool_input, "limit");
    println!("[CLAUDE_HOOK] pre_read - file: {}", file_path);

    let scanned = read_file_slice(&file_path, offset, limit).unwrap_or_default();
    let response = handle_pre_tool(&state, &input, &scanned, Some(file_path), true);
    (StatusCode::OK, Json(response))
}

/// POST /claude_hook/pre_write
/// Matches Write | Edit | MultiEdit | NotebookEdit.
async fn pre_write_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: PreToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] pre_write parse error: {}", e);
            return (StatusCode::OK, Json(pre_tool_response(false, None)));
        }
    };
    let file_path = json_str(&input.tool_input, "file_path")
        .or_else(|| json_str(&input.tool_input, "notebook_path"));
    let scanned = extract_write_content(&input.tool_name, &input.tool_input);
    println!(
        "[CLAUDE_HOOK] pre_write - tool: {}, file: {:?}",
        input.tool_name, file_path
    );
    let response = handle_pre_tool(&state, &input, &scanned, file_path, true);
    (StatusCode::OK, Json(response))
}

/// POST /claude_hook/pre_mcp
/// Matches `mcp__*` tool calls.
async fn pre_mcp_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: PreToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] pre_mcp parse error: {}", e);
            return (StatusCode::OK, Json(pre_tool_response(false, None)));
        }
    };
    let scanned = serde_json::to_string(&input.tool_input).unwrap_or_default();
    println!("[CLAUDE_HOOK] pre_mcp - tool: {}", input.tool_name);
    let response = handle_pre_tool(&state, &input, &scanned, None, true);
    (StatusCode::OK, Json(response))
}

/// POST /claude_hook/post_tool
/// Updates the row created at PreToolUse with the tool response, latency, and
/// success/failure. Falls back to creating a fresh row if PreToolUse never ran.
async fn post_tool_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: PostToolUseInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] post_tool parse error: {}", e);
            return (
                StatusCode::OK,
                Json(GenericResponse {
                    status: "ok".to_string(),
                }),
            );
        }
    };
    println!(
        "[CLAUDE_HOOK] post_tool - tool: {}, tool_use_id: {:?}",
        input.tool_name, input.tool_use_id
    );

    let correlation_id = input
        .tool_use_id
        .clone()
        .unwrap_or_else(|| format!("{}-{}", input.session_id, input.tool_name));

    let response_text = serde_json::to_string(&input.tool_response).unwrap_or_default();
    let output_tokens = estimate_tokens(&response_text);

    // Try to update the existing PreToolUse row.
    let updated = state
        .db
        .update_agent_hook_output(
            BACKEND,
            &correlation_id,
            output_tokens,
            Some(&response_text),
            None,
        )
        .ok()
        .unwrap_or(false);

    // If no Pre row exists, create one as a standalone log so the tool call
    // isn't lost (this can happen when only PostToolUse hooks are installed).
    if !updated {
        let request_body_json = serde_json::to_string(&input.tool_input).unwrap_or_default();
        let metadata = ClaudeHookMetadata {
            correlation_id: correlation_id.clone(),
            session_id: input.session_id.clone(),
            hook_event_name: input.hook_event_name.clone(),
            cwd: input.cwd.clone(),
            transcript_path: input.transcript_path.clone(),
            tool_name: Some(input.tool_name.clone()),
            tool_use_id: input.tool_use_id.clone(),
            file_path: json_str(&input.tool_input, "file_path"),
            source: None,
        };
        let metadata_json = serde_json::to_string(&metadata).ok();

        if let Ok(request_id) = state.db.log_agent_hook_request(
            BACKEND,
            &correlation_id,
            TOOL_ENDPOINT,
            "",
            0,
            output_tokens,
            &request_body_json,
            &response_text,
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

    // ---- Symbol extraction (best-effort, fire-and-forget) ----
    extract_symbols_for_tool(
        &state.db,
        &input.tool_name,
        &input.tool_input,
        input.cwd.as_deref(),
    );

    (
        StatusCode::OK,
        Json(GenericResponse {
            status: "ok".to_string(),
        }),
    )
}

/// If the tool touched a file (Read/Write/Edit), read it from disk and
/// extract symbols via tree-sitter. Failures are silently ignored — symbol
/// data is supplementary, never blocking.
fn extract_symbols_for_tool(
    db: &crate::database::Database,
    tool_name: &str,
    tool_input: &Value,
    cwd: Option<&str>,
) {
    // Only process file-touching tools.
    let is_file_tool = matches!(
        tool_name,
        "Read" | "Write" | "Edit" | "NotebookEdit" | "read_file" | "write_file" | "edit_file"
    );
    if !is_file_tool {
        return;
    }

    let file_path = match json_str(tool_input, "file_path").or_else(|| json_str(tool_input, "path"))
    {
        Some(p) => p,
        None => return,
    };

    if !crate::symbols::is_supported_extension(&file_path) {
        return;
    }

    let cwd = match cwd {
        Some(c) => c.to_string(),
        None => return,
    };

    // Read file from disk (more reliable than parsing tool_response).
    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let symbols = crate::symbols::extract_symbols(&file_path, &content);
    if symbols.is_empty() {
        return;
    }

    // Make path relative to cwd for storage.
    let rel_path = if file_path.starts_with(&cwd) {
        file_path[cwd.len()..].trim_start_matches('/').to_string()
    } else {
        file_path.clone()
    };

    let _ = db.upsert_file_symbols(&cwd, &rel_path, &symbols);
}

/// POST /claude_hook/stop
/// Reads the transcript JSONL, sums usage across the latest assistant turn,
/// and overwrites the prompt row with real Anthropic-API token counts.
async fn stop_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: StopInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] stop parse error: {}", e);
            return (
                StatusCode::OK,
                Json(GenericResponse {
                    status: "ok".to_string(),
                }),
            );
        }
    };
    println!("[CLAUDE_HOOK] stop - session: {}", input.session_id);

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

    if let Some(u) = usage.as_ref() {
        match state.db.update_latest_agent_hook_with_usage(
            BACKEND,
            &input.session_id,
            PROMPT_ENDPOINT,
            u,
            None,
        ) {
            Ok(true) => {}
            Ok(false) => {
                println!(
                    "[CLAUDE_HOOK] stop - no open prompt row found for session: {} (was the UserPromptSubmit hook installed?)",
                    input.session_id
                );
            }
            Err(e) => {
                println!(
                    "[CLAUDE_HOOK] stop - DB error updating session {}: {}",
                    input.session_id, e
                );
            }
        }
    } else {
        println!(
            "[CLAUDE_HOOK] stop - no usage available for session: {}",
            input.session_id
        );
    }

    (
        StatusCode::OK,
        Json(GenericResponse {
            status: "ok".to_string(),
        }),
    )
}

/// POST /claude_hook/session_start
/// Standalone log row capturing session metadata.
async fn session_start_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: SessionStartInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] session_start parse error: {}", e);
            return (
                StatusCode::OK,
                Json(GenericResponse {
                    status: "ok".to_string(),
                }),
            );
        }
    };
    println!(
        "[CLAUDE_HOOK] session_start - session: {}, source: {:?}",
        input.session_id, input.source
    );

    // Clear ctx_read cache so the new conversation gets fresh file reads
    if let Some(ref cache) = state.ctx_read_cache {
        if let Ok(mut c) = cache.lock() {
            let cleared = c.clear();
            if cleared > 0 {
                println!(
                    "[CLAUDE_HOOK] session_start - cleared {} ctx_read cache entries",
                    cleared
                );
            }
        }
    }

    let request_body_json = serde_json::to_string(&input).unwrap_or_default();
    // Use session_id + ":start" so the prompt-row keyed on `session_id` doesn't
    // collide with this metadata row in the upsert path.
    let correlation_id = format!("{}:start", input.session_id);
    let metadata = ClaudeHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: None,
        tool_use_id: None,
        file_path: None,
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

/// POST /claude_hook/session_end
async fn session_end_handler(
    State(state): State<ClaudeHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: SessionEndInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CLAUDE_HOOK] session_end parse error: {}", e);
            return (
                StatusCode::OK,
                Json(GenericResponse {
                    status: "ok".to_string(),
                }),
            );
        }
    };
    println!(
        "[CLAUDE_HOOK] session_end - session: {}, reason: {:?}",
        input.session_id, input.matcher_value
    );

    let request_body_json = serde_json::to_string(&input).unwrap_or_default();
    let correlation_id = format!("{}:end", input.session_id);
    let metadata = ClaudeHookMetadata {
        correlation_id: correlation_id.clone(),
        session_id: input.session_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        cwd: input.cwd.clone(),
        transcript_path: input.transcript_path.clone(),
        tool_name: None,
        tool_use_id: None,
        file_path: None,
        source: input.matcher_value.clone(),
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    let _ = state.db.log_agent_hook_request(
        BACKEND,
        &correlation_id,
        SESSION_ENDPOINT,
        "",
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

pub fn create_claude_hooks_router(
    db: Database,
    settings: CustomBackendSettings,
    ctx_read_cache: Option<Arc<Mutex<crate::ctx_read::cache::SessionCache>>>,
) -> Router {
    let state = ClaudeHooksState {
        db,
        settings: Arc::new(settings),
        ctx_read_cache,
    };

    Router::new()
        .route("/user_prompt_submit", post(user_prompt_submit_handler))
        .route("/pre_bash", post(pre_bash_handler))
        .route("/pre_read", post(pre_read_handler))
        .route("/pre_write", post(pre_write_handler))
        .route("/pre_mcp", post(pre_mcp_handler))
        .route("/post_tool", post(post_tool_handler))
        .route("/stop", post(stop_handler))
        .route("/session_start", post(session_start_handler))
        .route("/session_end", post(session_end_handler))
        .with_state(state)
}
