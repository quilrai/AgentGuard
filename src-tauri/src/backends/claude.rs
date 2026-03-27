// Claude (Anthropic) Backend Implementation

use crate::backends::custom::{CustomBackendSettings, TokenSavingSettings};
use crate::backends::Backend;
use crate::requestresponsemetadata::{RequestMetadata, ResponseMetadata, ToolCall};
use std::collections::HashMap;

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

pub struct ClaudeBackend {
    settings: CustomBackendSettings,
}

impl ClaudeBackend {
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

impl Default for ClaudeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for ClaudeBackend {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn base_url(&self) -> &'static str {
        ANTHROPIC_BASE_URL
    }

    fn parse_request_metadata(&self, body: &str) -> RequestMetadata {
        let mut meta = RequestMetadata::default();

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            if let Some(model) = json.get("model").and_then(|v| v.as_str()) {
                meta.model = Some(model.to_string());
            }
            meta.has_system_prompt = json.get("system").is_some();
            meta.has_tools = json.get("tools").is_some();

            if let Some(messages) = json.get("messages").and_then(|v| v.as_array()) {
                for msg in messages {
                    if let Some(role) = msg.get("role").and_then(|v| v.as_str()) {
                        match role {
                            "user" => meta.user_message_count += 1,
                            "assistant" => meta.assistant_message_count += 1,
                            _ => {}
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
            // Check for thinking content blocks
            meta.has_thinking = body.contains("\"type\":\"thinking\"");

            // Track tool calls by index: (id, name, accumulated_input_json)
            let mut tool_calls_map: HashMap<i64, (String, String, String)> = HashMap::new();

            for line in body.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                let json_str = &line[6..];

                // Parse the SSE event
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match event_type {
                        "content_block_start" => {
                            // Check if this is a tool_use block
                            if let Some(content_block) = json.get("content_block") {
                                if content_block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                                    let index = json.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let id = content_block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let name = content_block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    tool_calls_map.insert(index, (id, name, String::new()));
                                }
                            }
                        }
                        "content_block_delta" => {
                            // Check if this is an input_json_delta for a tool
                            if let Some(delta) = json.get("delta") {
                                if delta.get("type").and_then(|v| v.as_str()) == Some("input_json_delta") {
                                    let index = json.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if let Some(partial_json) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                        if let Some(entry) = tool_calls_map.get_mut(&index) {
                                            entry.2.push_str(partial_json);
                                        }
                                    }
                                }
                            }
                        }
                        "message_delta" => {
                            // Get stop_reason
                            if let Some(delta) = json.get("delta") {
                                if let Some(reason) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                    meta.stop_reason = Some(reason.to_string());
                                }
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
                                meta.cache_read_tokens = usage
                                    .get("cache_read_input_tokens")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0) as i32;
                                meta.cache_creation_tokens = usage
                                    .get("cache_creation_input_tokens")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0) as i32;
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Convert accumulated tool calls to ToolCall structs
            let mut tool_calls: Vec<(i64, ToolCall)> = tool_calls_map
                .into_iter()
                .map(|(index, (id, name, input_str))| {
                    let input = serde_json::from_str(&input_str).unwrap_or(serde_json::Value::Null);
                    (index, ToolCall { id, name, input })
                })
                .collect();
            // Sort by index to maintain order
            tool_calls.sort_by_key(|(index, _)| *index);
            meta.tool_calls = tool_calls.into_iter().map(|(_, tc)| tc).collect();

        } else {
            // Non-streaming response
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                // Get stop_reason
                if let Some(reason) = json.get("stop_reason").and_then(|v| v.as_str()) {
                    meta.stop_reason = Some(reason.to_string());
                }

                // Check for thinking and tool_use in content
                if let Some(content) = json.get("content").and_then(|v| v.as_array()) {
                    meta.has_thinking = content
                        .iter()
                        .any(|c| c.get("type").and_then(|t| t.as_str()) == Some("thinking"));

                    // Extract tool calls
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                            meta.tool_calls.push(ToolCall { id, name, input });
                        }
                    }
                }

                if let Some(usage) = json.get("usage") {
                    meta.input_tokens = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    meta.output_tokens = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    meta.cache_read_tokens = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    meta.cache_creation_tokens = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                }
            }
        }

        meta
    }

    fn should_log(&self, body: &str) -> bool {
        // Check if request body looks like a Messages API call
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            // Must have "messages" array and "model" field
            let has_messages = json.get("messages").and_then(|v| v.as_array()).is_some();
            let has_model = json.get("model").and_then(|v| v.as_str()).is_some();
            has_messages && has_model
        } else {
            false
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
