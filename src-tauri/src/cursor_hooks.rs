// Cursor Hooks API Handlers
//
// Implements endpoints for Cursor IDE hooks to enable DLP blocking.
// Hooks: beforeSubmitPrompt, beforeReadFile, beforeTabFileRead,
//        beforeShellExecution, beforeMCPExecution,
//        afterAgentResponse, afterAgentThought, afterTabFileEdit

use crate::database::{Database, DLP_ACTION_BLOCKED, DLP_ACTION_PASSED, DLP_ACTION_RATELIMITED};
use crate::dlp::{check_dlp_patterns, DlpDetection};
use crate::predefined_backend_settings::CustomBackendSettings;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

// ============================================================================
// Common Input Fields (present in all hooks)
// Fields are required for JSON deserialization but not all are actively used
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CommonHookFields {
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
}

// ============================================================================
// Hook-specific Input Structures
// Some fields are only used for JSON deserialization, not actively read
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct BeforeSubmitPromptInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub prompt: String,
    pub attachments: Vec<Attachment>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct Attachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub file_path: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct BeforeReadFileInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub file_path: String,
    pub content: Option<String>,
    pub attachments: Option<Vec<Attachment>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct BeforeTabFileReadInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub file_path: String,
    pub content: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct AfterAgentResponseInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub text: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct AfterTabFileEditInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub file_path: String,
    pub edits: Vec<TabEdit>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TabEdit {
    pub old_string: String,
    pub new_string: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<TabEditRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_line: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TabEditRange {
    pub start_line_number: i32,
    pub start_column: i32,
    pub end_line_number: i32,
    pub end_column: i32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct AfterAgentThoughtInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub text: String,
    pub duration_ms: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct BeforeShellExecutionInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub command: String,
    pub cwd: Option<String>,
    pub sandbox: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct BeforeMCPExecutionInput {
    // Common fields
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    pub hook_event_name: String,
    pub cursor_version: String,
    pub workspace_roots: Vec<String>,
    pub user_email: Option<String>,
    // Hook-specific
    pub server_name: String,
    pub tool_name: String,
    pub arguments: Option<serde_json::Value>,
}

// ============================================================================
// Response Structures
// ============================================================================

#[derive(Debug, Serialize)]
pub struct BeforeSubmitPromptResponse {
    #[serde(rename = "continue")]
    pub should_continue: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BeforeReadFileResponse {
    pub permission: String, // "allow" or "deny"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BeforeTabFileReadResponse {
    pub permission: String, // "allow" or "deny"
}

#[derive(Debug, Serialize)]
pub struct BeforeShellExecutionResponse {
    pub permission: String, // "allow" or "deny"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BeforeMCPExecutionResponse {
    pub permission: String, // "allow" or "deny"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GenericResponse {
    pub status: String,
}

// ============================================================================
// Extra Metadata for DB Storage
// ============================================================================

#[derive(Debug, Serialize)]
struct CursorHookMetadata {
    /// Stable per-turn ID used to thread before/after hooks together at the
    /// DB layer (matched on by `log_agent_hook_request` /
    /// `update_agent_hook_output`). For Cursor this is the engine's
    /// `generation_id`; the field is named `correlation_id` so the same DB
    /// path can serve Claude/Codex hooks too.
    correlation_id: String,
    conversation_id: String,
    generation_id: String,
    hook_event_name: String,
    user_email: Option<String>,
    cursor_version: String,
    workspace_roots: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_word_count: Option<i32>,
}

// ============================================================================
// State
// ============================================================================

#[derive(Clone)]
pub struct CursorHooksState {
    pub db: Database,
    pub settings: Arc<CustomBackendSettings>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Count words in a string (simple whitespace split)
fn count_words(text: &str) -> i32 {
    text.split_whitespace().count() as i32
}

/// Estimate token count from text (words * 1.5)
fn estimate_tokens(text: &str) -> i32 {
    (count_words(text) as f32 * 1.5) as i32
}

/// Format detected entities for user message
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

/// Check token limit for cursor hooks
/// Returns (is_allowed, error_message)
fn check_cursor_token_limit(
    token_count: i32,
    settings: &CustomBackendSettings,
) -> (bool, Option<String>) {
    let max_tokens = settings.max_tokens_in_a_request;

    if max_tokens == 0 || token_count <= max_tokens as i32 {
        return (true, None);
    }

    // Cursor only supports block mode
    (
        false,
        Some(format!(
            "Token limit exceeded: {} tokens (limit: {})",
            token_count, max_tokens
        )),
    )
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /cursor_hook/before_submit_prompt
/// Checks prompt and attached files for sensitive data, blocks if found
async fn before_submit_prompt_handler(
    State(state): State<CursorHooksState>,
    Json(raw_json): Json<Value>,
) -> impl IntoResponse {
    let input: BeforeSubmitPromptInput = match serde_json::from_value(raw_json) {
        Ok(v) => v,
        Err(e) => {
            println!("[CURSOR_HOOK] Failed to parse input: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(BeforeSubmitPromptResponse {
                    should_continue: true,
                    user_message: Some(format!("Parse error: {}", e)),
                }),
            );
        }
    };

    println!(
        "[CURSOR_HOOK] before_submit_prompt - generation_id: {}, attachments: {}",
        input.generation_id,
        input.attachments.len()
    );

    // Calculate total token count first
    let mut total_token_count = estimate_tokens(&input.prompt);
    for attachment in &input.attachments {
        if let (Some(file_path), Some(att_type)) = (&attachment.file_path, &attachment.attachment_type) {
            if att_type == "file" {
                if let Ok(content) = std::fs::read_to_string(file_path) {
                    total_token_count += estimate_tokens(&content);
                }
            }
        }
    }

    // Serialize full input for request_body (before moving fields)
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Build extra metadata
    let metadata = CursorHookMetadata {
        correlation_id: input.generation_id.clone(),
        conversation_id: input.conversation_id.clone(),
        generation_id: input.generation_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        user_email: input.user_email.clone(),
        cursor_version: input.cursor_version.clone(),
        workspace_roots: input.workspace_roots.clone(),
        file_path: None,
        thinking_word_count: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Check token limit
    let (token_allowed, token_error) = check_cursor_token_limit(total_token_count, &state.settings);
    if !token_allowed {
        println!(
            "[CURSOR_HOOK] Token limit exceeded for generation_id: {}, tokens: {}",
            input.generation_id, total_token_count
        );
        let response = BeforeSubmitPromptResponse {
            should_continue: false,
            user_message: token_error,
        };
        let response_body_json = serde_json::to_string(&response).unwrap_or_default();

        // Log the token-limited request
        let _ = state.db.log_agent_hook_request(
            "cursor-hooks",
            &input.generation_id,
            "CursorChat",
            &input.model,
            total_token_count,
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

    // Check DLP patterns (only if DLP is enabled)
    let mut all_detections: Vec<DlpDetection> = Vec::new();
    if state.settings.dlp_enabled {
        all_detections = check_dlp_patterns(&input.prompt);

        // Also check attached files
        for attachment in &input.attachments {
            if let (Some(file_path), Some(att_type)) = (&attachment.file_path, &attachment.attachment_type) {
                if att_type == "file" {
                    match std::fs::read_to_string(file_path) {
                        Ok(content) => {
                            let file_detections = check_dlp_patterns(&content);
                            if !file_detections.is_empty() {
                                println!(
                                    "[CURSOR_HOOK] DLP detected in attached file: {}",
                                    file_path
                                );
                                all_detections.extend(file_detections);
                            }
                        }
                        Err(e) => {
                            println!(
                                "[CURSOR_HOOK] Error reading attached file {}: {}",
                                file_path, e
                            );
                        }
                    }
                }
            }
        }
    }

    let is_blocked = !all_detections.is_empty();

    // Create or update request entry
    let response_status = if is_blocked { 403 } else { 200 };
    let user_message = if is_blocked {
        Some(format_detection_message(&all_detections))
    } else {
        None
    };

    // Build response
    let response = BeforeSubmitPromptResponse {
        should_continue: !is_blocked,
        user_message: user_message.clone(),
    };
    let response_body_json = serde_json::to_string(&response).unwrap_or_default();

    // Log to database
    let dlp_action = if is_blocked { DLP_ACTION_BLOCKED } else { DLP_ACTION_PASSED };
    match state.db.log_agent_hook_request(
        "cursor-hooks",
        &input.generation_id,
        "CursorChat",
        &input.model,
        total_token_count,
        0, // output_tokens will be updated later
        &request_body_json,
        &response_body_json,
        response_status,
        metadata_json.as_deref(),
        None, // request_headers (not applicable for cursor hooks)
        None, // response_headers (not applicable for cursor hooks)
        dlp_action,
    ) {
        Ok(request_id) => {
            println!("[CURSOR_HOOK] before_submit_prompt - logged request_id: {}", request_id);
            // Log DLP detections if any
            if !all_detections.is_empty() {
                let _ = state.db.log_dlp_detections(request_id, &all_detections);
            }
        }
        Err(e) => {
            println!("[CURSOR_HOOK] before_submit_prompt - FAILED to log: {}", e);
        }
    }

    (StatusCode::OK, Json(response))
}

/// POST /cursor_hook/before_read_file
/// Checks file content for sensitive data, blocks if found
async fn before_read_file_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<BeforeReadFileInput>,
) -> impl IntoResponse {
    println!(
        "[CURSOR_HOOK] before_read_file - generation_id: {}, file: {}",
        input.generation_id, input.file_path
    );

    // Serialize full input for request_body (before moving any fields)
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Get content: prefer provided content, fallback to reading file
    let content = match &input.content {
        Some(c) => c.clone(),
        None => {
            // Read file from disk
            match std::fs::read_to_string(&input.file_path) {
                Ok(c) => c,
                Err(e) => {
                    println!(
                        "[CURSOR_HOOK] Failed to read file {}: {}",
                        input.file_path, e
                    );
                    // Allow if we can't read (file might not exist or be binary)
                    return (
                        StatusCode::OK,
                        Json(BeforeReadFileResponse {
                            permission: "allow".to_string(),
                            user_message: None,
                            agent_message: None,
                        }),
                    );
                }
            }
        }
    };

    let token_count = estimate_tokens(&content);

    // Build extra metadata
    let metadata = CursorHookMetadata {
        correlation_id: input.generation_id.clone(),
        conversation_id: input.conversation_id.clone(),
        generation_id: input.generation_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        user_email: input.user_email.clone(),
        cursor_version: input.cursor_version.clone(),
        workspace_roots: input.workspace_roots.clone(),
        file_path: Some(input.file_path.clone()),
        thinking_word_count: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Check token limit
    let (token_allowed, token_error) = check_cursor_token_limit(token_count, &state.settings);
    if !token_allowed {
        println!(
            "[CURSOR_HOOK] Token limit exceeded for generation_id: {}, tokens: {}",
            input.generation_id, token_count
        );
        let response = BeforeReadFileResponse {
            permission: "deny".to_string(),
            user_message: token_error,
            agent_message: Some("Token limit exceeded.".to_string()),
        };
        let response_body_json = serde_json::to_string(&response).unwrap_or_default();

        // Log the token-limited request
        let _ = state.db.log_agent_hook_request(
            "cursor-hooks",
            &input.generation_id,
            "CursorChat",
            &input.model,
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

    // Check DLP patterns (only if DLP is enabled)
    let mut all_detections: Vec<DlpDetection> = Vec::new();
    if state.settings.dlp_enabled {
        all_detections = check_dlp_patterns(&content);

        // Also check attached files if present
        if let Some(attachments) = &input.attachments {
            for attachment in attachments {
                if let (Some(file_path), Some(att_type)) = (&attachment.file_path, &attachment.attachment_type) {
                    if att_type == "file" {
                        match std::fs::read_to_string(file_path) {
                            Ok(att_content) => {
                                let file_detections = check_dlp_patterns(&att_content);
                                if !file_detections.is_empty() {
                                    println!(
                                        "[CURSOR_HOOK] DLP detected in attached file: {}",
                                        file_path
                                    );
                                    all_detections.extend(file_detections);
                                }
                            }
                            Err(e) => {
                                println!(
                                    "[CURSOR_HOOK] Error reading attached file {}: {}",
                                    file_path, e
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let is_blocked = !all_detections.is_empty();

    let (permission, user_message, agent_message) = if is_blocked {
        let msg = format_detection_message(&all_detections);
        (
            "deny".to_string(),
            Some(msg.clone()),
            Some(format!(
                "Access to file {} was blocked due to sensitive data detection.",
                input.file_path
            )),
        )
    } else {
        ("allow".to_string(), None, None)
    };

    // Build response
    let response = BeforeReadFileResponse {
        permission: permission.clone(),
        user_message: user_message.clone(),
        agent_message: agent_message.clone(),
    };
    let response_body_json = serde_json::to_string(&response).unwrap_or_default();

    // Log to database
    let response_status = if is_blocked { 403 } else { 200 };
    let dlp_action = if is_blocked { DLP_ACTION_BLOCKED } else { DLP_ACTION_PASSED };

    if let Ok(request_id) = state.db.log_agent_hook_request(
        "cursor-hooks",
        &input.generation_id,
        "CursorChat",
        &input.model,
        token_count,
        0,
        &request_body_json,
        &response_body_json,
        response_status,
        metadata_json.as_deref(),
        None, // request_headers (not applicable for cursor hooks)
        None, // response_headers (not applicable for cursor hooks)
        dlp_action,
    ) {
        if !all_detections.is_empty() {
            let _ = state.db.log_dlp_detections(request_id, &all_detections);
        }
    }

    (StatusCode::OK, Json(response))
}

/// POST /cursor_hook/before_tab_file_read
/// Checks file content for Tab completions, blocks if sensitive data found
/// NOTE: Only DLP is applied on this endpoint
async fn before_tab_file_read_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<BeforeTabFileReadInput>,
) -> impl IntoResponse {
    println!(
        "[CURSOR_HOOK] before_tab_file_read - generation_id: {}, file: {}",
        input.generation_id, input.file_path
    );

    // Serialize full input for request_body (before moving any fields)
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Get content: prefer provided content, fallback to reading file
    let content = match input.content {
        Some(c) => c,
        None => {
            match std::fs::read_to_string(&input.file_path) {
                Ok(c) => c,
                Err(e) => {
                    println!(
                        "[CURSOR_HOOK] Failed to read file {}: {}",
                        input.file_path, e
                    );
                    // Allow if we can't read
                    return (
                        StatusCode::OK,
                        Json(BeforeTabFileReadResponse {
                            permission: "allow".to_string(),
                        }),
                    );
                }
            }
        }
    };

    // Check DLP patterns (only if DLP is enabled)
    // NOTE: before_tab_file_read only applies DLP
    let detections = if state.settings.dlp_enabled {
        check_dlp_patterns(&content)
    } else {
        Vec::new()
    };
    let is_blocked = !detections.is_empty();

    // Build extra metadata
    let metadata = CursorHookMetadata {
        correlation_id: input.generation_id.clone(),
        conversation_id: input.conversation_id,
        generation_id: input.generation_id.clone(),
        hook_event_name: input.hook_event_name,
        user_email: input.user_email,
        cursor_version: input.cursor_version,
        workspace_roots: input.workspace_roots,
        file_path: Some(input.file_path.clone()),
        thinking_word_count: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Build response
    let response = BeforeTabFileReadResponse {
        permission: if is_blocked { "deny" } else { "allow" }.to_string(),
    };
    let response_body_json = serde_json::to_string(&response).unwrap_or_default();

    // Log to database
    let token_count = estimate_tokens(&content);
    let response_status = if is_blocked { 403 } else { 200 };
    let dlp_action = if is_blocked { DLP_ACTION_BLOCKED } else { DLP_ACTION_PASSED };

    if let Ok(request_id) = state.db.log_agent_hook_request(
        "cursor-hooks",
        &input.generation_id,
        "CursorTab",
        &input.model,
        token_count,
        0,
        &request_body_json,
        &response_body_json,
        response_status,
        metadata_json.as_deref(),
        None, // request_headers (not applicable for cursor hooks)
        None, // response_headers (not applicable for cursor hooks)
        dlp_action,
    ) {
        if !detections.is_empty() {
            let _ = state.db.log_dlp_detections(request_id, &detections);
        }
    }

    // ---- Symbol extraction (best-effort) ----
    if crate::symbols::is_supported_extension(&input.file_path) {
        let cwd = metadata.workspace_roots.first().cloned().unwrap_or_default();
        if !cwd.is_empty() {
            let file_content = std::fs::read_to_string(&input.file_path).unwrap_or_default();
            if !file_content.is_empty() {
                let symbols = crate::symbols::extract_symbols(&input.file_path, &file_content);
                if !symbols.is_empty() {
                    let rel = if input.file_path.starts_with(&cwd) {
                        input.file_path[cwd.len()..].trim_start_matches('/').to_string()
                    } else {
                        input.file_path.clone()
                    };
                    let _ = state.db.upsert_file_symbols(&cwd, &rel, &symbols);
                }
            }
        }
    }

    (StatusCode::OK, Json(response))
}

/// POST /cursor_hook/after_agent_response
/// Logs agent response for monitoring (word count as output_tokens)
async fn after_agent_response_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<AfterAgentResponseInput>,
) -> impl IntoResponse {
    println!(
        "[CURSOR_HOOK] after_agent_response - generation_id: {}",
        input.generation_id
    );

    let token_count = estimate_tokens(&input.text);

    // Update existing request entry with output tokens
    match state.db.update_agent_hook_output(
        "cursor-hooks",
        &input.generation_id,
        token_count,
        Some(&input.text),
        None,
    ) {
        Ok(false) => {
            println!(
                "[CURSOR_HOOK] WARNING: No entry found for generation_id: {} in after_agent_response",
                input.generation_id
            );
        }
        Err(e) => {
            println!(
                "[CURSOR_HOOK] ERROR: Failed to update output for generation_id {}: {}",
                input.generation_id, e
            );
        }
        _ => {}
    }

    (StatusCode::OK, Json(GenericResponse { status: "ok".to_string() }))
}

/// POST /cursor_hook/after_agent_thought
/// Logs agent thinking for monitoring (word count added to output_tokens)
async fn after_agent_thought_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<AfterAgentThoughtInput>,
) -> impl IntoResponse {
    println!(
        "[CURSOR_HOOK] after_agent_thought - generation_id: {}, duration_ms: {:?}",
        input.generation_id, input.duration_ms
    );

    let token_count = estimate_tokens(&input.text);

    // Add thinking token count to output tokens
    match state.db.add_agent_hook_thinking_tokens(
        "cursor-hooks",
        &input.generation_id,
        token_count,
    ) {
        Ok(false) => {
            println!(
                "[CURSOR_HOOK] WARNING: No entry found for generation_id: {} in after_agent_thought",
                input.generation_id
            );
        }
        Err(e) => {
            println!(
                "[CURSOR_HOOK] ERROR: Failed to add thinking tokens for generation_id {}: {}",
                input.generation_id, e
            );
        }
        _ => {}
    }

    (StatusCode::OK, Json(GenericResponse { status: "ok".to_string() }))
}

/// POST /cursor_hook/after_tab_file_edit
/// Logs Tab edits for monitoring - updates existing entry from beforeTabFileRead
async fn after_tab_file_edit_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<AfterTabFileEditInput>,
) -> impl IntoResponse {
    println!(
        "[CURSOR_HOOK] after_tab_file_edit - generation_id: {}, file: {}, edits: {}",
        input.generation_id, input.file_path, input.edits.len()
    );

    // Calculate token count from new_string in all edits (represents output/generated code)
    let output_token_count: i32 = input
        .edits
        .iter()
        .map(|edit| estimate_tokens(&edit.new_string))
        .sum();

    // Serialize edits for response body
    let edits_json = serde_json::to_string(&input.edits).unwrap_or_default();
    let response_body = format!("Tab edit: {}\nEdits: {}", input.file_path, edits_json);

    // Update existing entry from beforeTabFileRead with output tokens
    match state.db.update_agent_hook_output(
        "cursor-hooks",
        &input.generation_id,
        output_token_count,
        Some(&response_body),
        None,
    ) {
        Ok(false) => {
            println!(
                "[CURSOR_HOOK] WARNING: No entry found for generation_id: {} in after_tab_file_edit",
                input.generation_id
            );
        }
        Err(e) => {
            println!(
                "[CURSOR_HOOK] ERROR: Failed to update output for generation_id {}: {}",
                input.generation_id, e
            );
        }
        _ => {}
    }

    (StatusCode::OK, Json(GenericResponse { status: "ok".to_string() }))
}

/// POST /cursor_hook/before_shell_execution
/// Checks shell command for sensitive data, blocks if found, logs tool call
async fn before_shell_execution_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<BeforeShellExecutionInput>,
) -> impl IntoResponse {
    println!("============================================================");
    println!("[CURSOR_HOOK] before_shell_execution CALLED");
    println!("  generation_id: {}", input.generation_id);
    println!("  command: {}", input.command);
    println!("  cwd: {:?}", input.cwd);
    println!("  sandbox: {:?}", input.sandbox);
    println!("  user_email: {:?}", input.user_email);
    println!("============================================================");

    // Serialize full input for request_body
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Build extra metadata
    let metadata = CursorHookMetadata {
        correlation_id: input.generation_id.clone(),
        conversation_id: input.conversation_id.clone(),
        generation_id: input.generation_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        user_email: input.user_email.clone(),
        cursor_version: input.cursor_version.clone(),
        workspace_roots: input.workspace_roots.clone(),
        file_path: None,
        thinking_word_count: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    let token_count = estimate_tokens(&input.command);

    // Check DLP patterns on command (only if DLP is enabled)
    let detections = if state.settings.dlp_enabled {
        check_dlp_patterns(&input.command)
    } else {
        Vec::new()
    };
    let is_blocked = !detections.is_empty();

    let (permission, user_message, agent_message) = if is_blocked {
        let msg = format_detection_message(&detections);
        (
            "deny".to_string(),
            Some(msg.clone()),
            Some(format!(
                "Shell command was blocked due to sensitive data detection."
            )),
        )
    } else {
        ("allow".to_string(), None, None)
    };

    // Build response
    let response = BeforeShellExecutionResponse {
        permission: permission.clone(),
        user_message: user_message.clone(),
        agent_message: agent_message.clone(),
    };
    let response_body_json = serde_json::to_string(&response).unwrap_or_default();

    // Log to database
    let response_status = if is_blocked { 403 } else { 200 };
    let dlp_action = if is_blocked { DLP_ACTION_BLOCKED } else { DLP_ACTION_PASSED };

    if let Ok(request_id) = state.db.log_agent_hook_request(
        "cursor-hooks",
        &input.generation_id,
        "CursorChat",
        &input.model,
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
        println!("[CURSOR_HOOK] before_shell_execution - logged request_id: {}", request_id);

        // Log DLP detections if any
        if !detections.is_empty() {
            let _ = state.db.log_dlp_detections(request_id, &detections);
            println!("[CURSOR_HOOK] before_shell_execution - logged {} DLP detections", detections.len());
        }

        // Log as tool call (shell execution)
        let tool_call = crate::requestresponsemetadata::ToolCall {
            id: format!("shell-{}", input.generation_id),
            name: "shell".to_string(),
            input: serde_json::json!({
                "command": input.command,
                "cwd": input.cwd,
                "sandbox": input.sandbox,
            }),
        };
        let _ = state.db.log_tool_calls(request_id, &[tool_call]);
        println!("[CURSOR_HOOK] before_shell_execution - logged tool call");
    } else {
        println!("[CURSOR_HOOK] before_shell_execution - FAILED to log request");
    }

    println!("[CURSOR_HOOK] before_shell_execution - returning response: {:?}", response);
    (StatusCode::OK, Json(response))
}

/// POST /cursor_hook/before_mcp_execution
/// Checks MCP tool arguments for sensitive data, blocks if found, logs tool call
async fn before_mcp_execution_handler(
    State(state): State<CursorHooksState>,
    Json(input): Json<BeforeMCPExecutionInput>,
) -> impl IntoResponse {
    println!("============================================================");
    println!("[CURSOR_HOOK] before_mcp_execution CALLED");
    println!("  generation_id: {}", input.generation_id);
    println!("  server_name: {}", input.server_name);
    println!("  tool_name: {}", input.tool_name);
    println!("  arguments: {:?}", input.arguments);
    println!("  user_email: {:?}", input.user_email);
    println!("============================================================");

    // Serialize full input for request_body
    let request_body_json = serde_json::to_string(&input).unwrap_or_default();

    // Build extra metadata
    let metadata = CursorHookMetadata {
        correlation_id: input.generation_id.clone(),
        conversation_id: input.conversation_id.clone(),
        generation_id: input.generation_id.clone(),
        hook_event_name: input.hook_event_name.clone(),
        user_email: input.user_email.clone(),
        cursor_version: input.cursor_version.clone(),
        workspace_roots: input.workspace_roots.clone(),
        file_path: None,
        thinking_word_count: None,
    };
    let metadata_json = serde_json::to_string(&metadata).ok();

    // Convert arguments to string for token counting and DLP check
    let args_str = input
        .arguments
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();
    let token_count = estimate_tokens(&args_str) + estimate_tokens(&input.tool_name);

    // Check DLP patterns on arguments (only if DLP is enabled)
    let detections = if state.settings.dlp_enabled {
        check_dlp_patterns(&args_str)
    } else {
        Vec::new()
    };
    let is_blocked = !detections.is_empty();

    let (permission, user_message, agent_message) = if is_blocked {
        let msg = format_detection_message(&detections);
        (
            "deny".to_string(),
            Some(msg.clone()),
            Some(format!(
                "MCP tool '{}' on server '{}' was blocked due to sensitive data detection.",
                input.tool_name, input.server_name
            )),
        )
    } else {
        ("allow".to_string(), None, None)
    };

    // Build response
    let response = BeforeMCPExecutionResponse {
        permission: permission.clone(),
        user_message: user_message.clone(),
        agent_message: agent_message.clone(),
    };
    let response_body_json = serde_json::to_string(&response).unwrap_or_default();

    // Log to database
    let response_status = if is_blocked { 403 } else { 200 };
    let dlp_action = if is_blocked { DLP_ACTION_BLOCKED } else { DLP_ACTION_PASSED };

    if let Ok(request_id) = state.db.log_agent_hook_request(
        "cursor-hooks",
        &input.generation_id,
        "CursorChat",
        &input.model,
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
        println!("[CURSOR_HOOK] before_mcp_execution - logged request_id: {}", request_id);

        // Log DLP detections if any
        if !detections.is_empty() {
            let _ = state.db.log_dlp_detections(request_id, &detections);
            println!("[CURSOR_HOOK] before_mcp_execution - logged {} DLP detections", detections.len());
        }

        // Log as tool call (MCP execution)
        let tool_call = crate::requestresponsemetadata::ToolCall {
            id: format!("mcp-{}-{}", input.server_name, input.generation_id),
            name: format!("mcp:{}:{}", input.server_name, input.tool_name),
            input: input.arguments.clone().unwrap_or(serde_json::json!({})),
        };
        let _ = state.db.log_tool_calls(request_id, &[tool_call]);
        println!("[CURSOR_HOOK] before_mcp_execution - logged tool call");
    } else {
        println!("[CURSOR_HOOK] before_mcp_execution - FAILED to log request");
    }

    println!("[CURSOR_HOOK] before_mcp_execution - returning response: {:?}", response);
    (StatusCode::OK, Json(response))
}

// ============================================================================
// Router
// ============================================================================

pub fn create_cursor_hooks_router(
    db: Database,
    settings: CustomBackendSettings,
) -> Router {
    let state = CursorHooksState {
        db,
        settings: Arc::new(settings),
    };

    Router::new()
        .route("/before_submit_prompt", post(before_submit_prompt_handler))
        .route("/before_read_file", post(before_read_file_handler))
        .route("/before_tab_file_read", post(before_tab_file_read_handler))
        .route("/before_shell_execution", post(before_shell_execution_handler))
        .route("/before_mcp_execution", post(before_mcp_execution_handler))
        .route("/after_agent_response", post(after_agent_response_handler))
        .route("/after_agent_thought", post(after_agent_thought_handler))
        .route("/after_tab_file_edit", post(after_tab_file_edit_handler))
        .with_state(state)
}
