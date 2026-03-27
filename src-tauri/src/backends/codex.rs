// Codex (OpenAI GPT-5) Backend Implementation

use axum::http::HeaderMap;
use serde_json::json;

use crate::backends::custom::{CustomBackendSettings, TokenSavingSettings};
use crate::backends::Backend;
use crate::requestresponsemetadata::{RequestMetadata, ResponseMetadata, ToolCall};
use std::collections::HashMap;

pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

pub struct CodexBackend {
    settings: CustomBackendSettings,
}

impl CodexBackend {
    pub fn new() -> Self {
        Self {
            settings: CustomBackendSettings::default(),
        }
    }

    pub fn with_settings(settings_json: &str) -> Self {
        let settings: CustomBackendSettings = serde_json::from_str(settings_json)
            .unwrap_or_default();
        Self { settings }
    }
}

impl Default for CodexBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for CodexBackend {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn base_url(&self) -> &'static str {
        CODEX_BASE_URL
    }

    fn parse_request_metadata(&self, body: &str) -> RequestMetadata {
        let mut meta = RequestMetadata::default();

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            // Extract model
            if let Some(model) = json.get("model").and_then(|v| v.as_str()) {
                meta.model = Some(model.to_string());
            }

            // Codex uses "instructions" field instead of "system"
            meta.has_system_prompt = json.get("instructions").is_some();

            // Check for tools
            meta.has_tools = json.get("tools").is_some();

            // Count messages in the "input" array
            // Codex input format: [{"type": "message", "role": "user", ...}, {"type": "reasoning", ...}, ...]
            if let Some(input) = json.get("input").and_then(|v| v.as_array()) {
                for item in input {
                    // Only count items with type "message"
                    if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                        if let Some(role) = item.get("role").and_then(|v| v.as_str()) {
                            match role {
                                "user" => meta.user_message_count += 1,
                                "assistant" => meta.assistant_message_count += 1,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        meta
    }

    fn parse_response_metadata(&self, body: &str, is_streaming: bool) -> ResponseMetadata {
        let mut meta = ResponseMetadata::default();

        if is_streaming {
            // Check for reasoning in the streamed response
            meta.has_thinking = body.contains("\"type\":\"reasoning\"");

            // Track function calls by item_id: (call_id, name, accumulated_arguments)
            let mut function_calls_map: HashMap<String, (String, String, String)> = HashMap::new();

            // Parse SSE stream
            for line in body.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                let json_str = &line[6..];

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match event_type {
                        "response.output_item.added" => {
                            // Check if this is a function_call item
                            if let Some(item) = json.get("item") {
                                if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                                    // item_id is used to match delta events, call_id is the external ID we store
                                    let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    println!("[CODEX] output_item.added: item_id={}, call_id={}, name={}", item_id, call_id, name);
                                    function_calls_map.insert(item_id, (call_id, name, String::new()));
                                }
                            }
                        }
                        "response.function_call_arguments.delta" => {
                            // Delta events use item_id to identify which function call
                            if let Some(item_id) = json.get("item_id").and_then(|v| v.as_str()) {
                                if let Some(delta) = json.get("delta").and_then(|v| v.as_str()) {
                                    if let Some(entry) = function_calls_map.get_mut(item_id) {
                                        entry.2.push_str(delta);
                                    }
                                }
                            }
                        }
                        "response.completed" => {
                            // Extract final usage and status
                            if let Some(response) = json.get("response") {
                                if let Some(status) = response.get("status").and_then(|v| v.as_str()) {
                                    meta.stop_reason = Some(status.to_string());
                                }

                                if let Some(usage) = response.get("usage") {
                                    meta.input_tokens = usage
                                        .get("input_tokens")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0) as i32;
                                    meta.output_tokens = usage
                                        .get("output_tokens")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0) as i32;

                                    if let Some(details) = usage.get("input_tokens_details") {
                                        meta.cache_read_tokens = details
                                            .get("cached_tokens")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or(0) as i32;
                                    }
                                }

                                // Also extract function calls from the completed response output
                                if let Some(output) = response.get("output").and_then(|v| v.as_array()) {
                                    for item in output {
                                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                            let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let arguments = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            // Only add if not already tracked via streaming
                                            if !function_calls_map.contains_key(&item_id) {
                                                function_calls_map.insert(item_id, (call_id, name, arguments));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Convert accumulated function calls to ToolCall structs
            println!("[CODEX] Final function_calls_map: {:?}", function_calls_map);
            meta.tool_calls = function_calls_map
                .into_iter()
                .map(|(_item_id, (call_id, name, arguments))| {
                    let input = serde_json::from_str(&arguments).unwrap_or(serde_json::Value::Null);
                    println!("[CODEX] ToolCall: id={}, name={}, args_len={}, input={}", call_id, name, arguments.len(), input);
                    ToolCall { id: call_id, name, input }
                })
                .collect();

        } else {
            // Non-streaming response (full JSON object)
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                // Check for reasoning in output
                if let Some(output) = json.get("output").and_then(|v| v.as_array()) {
                    meta.has_thinking = output
                        .iter()
                        .any(|item| item.get("type").and_then(|t| t.as_str()) == Some("reasoning"));

                    // Extract function calls from output
                    for item in output {
                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let arguments = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                            let input = serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
                            meta.tool_calls.push(ToolCall { id: call_id, name, input });
                        }
                    }
                }

                // Get status as stop_reason
                if let Some(status) = json.get("status").and_then(|v| v.as_str()) {
                    meta.stop_reason = Some(status.to_string());
                }

                // Get usage
                if let Some(usage) = json.get("usage") {
                    meta.input_tokens = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    meta.output_tokens = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;

                    if let Some(details) = usage.get("input_tokens_details") {
                        meta.cache_read_tokens = details
                            .get("cached_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0) as i32;
                    }
                }
            }
        }

        meta
    }

    fn should_log(&self, body: &str) -> bool {
        // Log if request has "model" and "input" fields (completion request)
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            let has_input = json.get("input").is_some();
            let has_model = json.get("model").and_then(|v| v.as_str()).is_some();
            has_input && has_model
        } else {
            false
        }
    }

    fn extract_extra_metadata(
        &self,
        request_body: &str,
        response_body: &str,
        headers: &HeaderMap,
    ) -> Option<String> {
        let mut extra = serde_json::Map::new();

        // Extract conversation_id and session_id from headers
        if let Some(conv_id) = headers.get("conversation_id").and_then(|v| v.to_str().ok()) {
            extra.insert("conversation_id".to_string(), json!(conv_id));
        }
        if let Some(sess_id) = headers.get("session_id").and_then(|v| v.to_str().ok()) {
            extra.insert("session_id".to_string(), json!(sess_id));
        }

        // Count function calls and reasoning blocks from request
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(request_body) {
            if let Some(input) = json.get("input").and_then(|v| v.as_array()) {
                let function_call_count = input
                    .iter()
                    .filter(|item| item.get("type").and_then(|t| t.as_str()) == Some("function_call"))
                    .count();
                if function_call_count > 0 {
                    extra.insert("function_call_count".to_string(), json!(function_call_count));
                }

                // Check for reasoning in input (previous turns)
                let has_reasoning_input = input
                    .iter()
                    .any(|item| item.get("type").and_then(|t| t.as_str()) == Some("reasoning"));
                if has_reasoning_input {
                    extra.insert("has_reasoning_input".to_string(), json!(true));
                }
            }

            // Extract prompt_cache_key if present
            if let Some(cache_key) = json.get("prompt_cache_key").and_then(|v| v.as_str()) {
                extra.insert("prompt_cache_key".to_string(), json!(cache_key));
            }
        }

        // Extract reasoning summaries from response
        let mut reasoning_summaries: Vec<String> = Vec::new();
        for line in response_body.lines() {
            if line.starts_with("data: ") && line.contains("reasoning_summary_text.done") {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line[6..]) {
                    if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                        reasoning_summaries.push(text.to_string());
                    }
                }
            }
        }
        if !reasoning_summaries.is_empty() {
            extra.insert("reasoning_summaries".to_string(), json!(reasoning_summaries));
        }

        if extra.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&extra).unwrap_or_default())
        }
    }

    fn is_dlp_enabled(&self) -> bool {
        self.settings.dlp_enabled
    }

    fn get_rate_limit(&self) -> (u32, u32) {
        (self.settings.rate_limit_requests, self.settings.rate_limit_minutes.max(1))
    }

    fn get_max_tokens_limit(&self) -> (u32, String) {
        (self.settings.max_tokens_in_a_request, self.settings.action_for_max_tokens_in_a_request.clone())
    }

    fn get_token_saving_settings(&self) -> TokenSavingSettings {
        self.settings.token_saving.clone()
    }
}
