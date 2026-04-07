// Custom Backend Implementation for OpenAI-compatible endpoints

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::backends::Backend;
use crate::requestresponsemetadata::{RequestMetadata, ResponseMetadata};

/// Token saving settings with sub-category features
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenSavingSettings {
    /// Shell compression: compress shell command output before it reaches the agent
    #[serde(default)]
    pub shell_compression: bool,
    // Future features can be added here as new bool fields
}

impl TokenSavingSettings {
    /// Returns true if any token saving feature is enabled
    pub fn any_enabled(&self) -> bool {
        self.shell_compression
    }
}

/// Settings for a custom backend
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomBackendSettings {
    /// Whether DLP is enabled for this backend (default: true)
    #[serde(default = "default_true")]
    pub dlp_enabled: bool,
    /// Rate limit: number of requests allowed (0 = no limit)
    #[serde(default)]
    pub rate_limit_requests: u32,
    /// Rate limit: time window in minutes (default: 1)
    #[serde(default = "default_one")]
    pub rate_limit_minutes: u32,
    /// Maximum tokens allowed in a request (0 = no limit)
    #[serde(default)]
    pub max_tokens_in_a_request: u32,
    /// Action to take when max tokens is exceeded: "block" or "notify" (default: "block")
    #[serde(default = "default_block")]
    pub action_for_max_tokens_in_a_request: String,
    /// Token saving settings with sub-category features
    #[serde(default)]
    pub token_saving: TokenSavingSettings,
}

fn default_true() -> bool {
    true
}

fn default_one() -> u32 {
    1
}

fn default_block() -> String {
    "block".to_string()
}

/// A custom backend that proxies to user-defined OpenAI-compatible endpoints
pub struct CustomBackend {
    name: String,
    base_url: String,
    settings: CustomBackendSettings,
}

impl CustomBackend {
    pub fn new(name: String, base_url: String, settings_json: &str) -> Self {
        // Remove trailing slash from base_url if present
        let base_url = base_url.trim_end_matches('/').to_string();

        // Parse settings from JSON, use defaults if parsing fails
        let settings: CustomBackendSettings = serde_json::from_str(settings_json)
            .unwrap_or_default();

        Self { name, base_url, settings }
    }
}

impl Backend for CustomBackend {
    fn name(&self) -> &str {
        &self.name
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn parse_request_metadata(&self, body: &str) -> RequestMetadata {
        let mut meta = RequestMetadata::default();

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            // Extract model (OpenAI format)
            if let Some(model) = json.get("model").and_then(|v| v.as_str()) {
                meta.model = Some(model.to_string());
            }

            // Check for system message in messages array (OpenAI format)
            // or system field (some providers)
            if json.get("system").is_some() {
                meta.has_system_prompt = true;
            }

            // Check for tools/functions (OpenAI format)
            meta.has_tools = json.get("tools").is_some() || json.get("functions").is_some();

            // Count messages in OpenAI format: {"messages": [{"role": "user", "content": "..."}]}
            if let Some(messages) = json.get("messages").and_then(|v| v.as_array()) {
                for msg in messages {
                    if let Some(role) = msg.get("role").and_then(|v| v.as_str()) {
                        match role {
                            "user" => meta.user_message_count += 1,
                            "assistant" => meta.assistant_message_count += 1,
                            "system" => meta.has_system_prompt = true,
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
            // Parse SSE stream for OpenAI format
            // Look for [DONE] or final chunk with usage
            for line in body.lines() {
                if line.starts_with("data: ") && !line.contains("[DONE]") {
                    let data = &line[6..];
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                        // Check for finish_reason in choices
                        if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
                            for choice in choices {
                                if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                    meta.stop_reason = Some(finish_reason.to_string());
                                }
                            }
                        }

                        // Some providers include usage in the final streaming chunk
                        if let Some(usage) = json.get("usage") {
                            meta.input_tokens = usage
                                .get("prompt_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0) as i32;
                            meta.output_tokens = usage
                                .get("completion_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0) as i32;
                        }
                    }
                }
            }
        } else {
            // Non-streaming response (full JSON object)
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                // Get finish_reason from choices (OpenAI format)
                if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
                    if let Some(first_choice) = choices.first() {
                        if let Some(finish_reason) = first_choice.get("finish_reason").and_then(|v| v.as_str()) {
                            meta.stop_reason = Some(finish_reason.to_string());
                        }
                    }
                }

                // Get usage (OpenAI format)
                if let Some(usage) = json.get("usage") {
                    meta.input_tokens = usage
                        .get("prompt_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    meta.output_tokens = usage
                        .get("completion_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;

                    // Some providers include cached tokens
                    if let Some(prompt_details) = usage.get("prompt_tokens_details") {
                        meta.cache_read_tokens = prompt_details
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
        // Log if request has "model" and "messages" fields (chat completion request)
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            let has_messages = json.get("messages").is_some();
            let has_model = json.get("model").and_then(|v| v.as_str()).is_some();
            has_messages && has_model
        } else {
            false
        }
    }

    fn extract_extra_metadata(
        &self,
        _request_body: &str,
        response_body: &str,
        _headers: &HeaderMap,
    ) -> Option<String> {
        let mut extra = serde_json::Map::new();

        // Extract response id if present
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(response_body) {
            if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                extra.insert("response_id".to_string(), serde_json::json!(id));
            }
            if let Some(created) = json.get("created").and_then(|v| v.as_i64()) {
                extra.insert("created".to_string(), serde_json::json!(created));
            }
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
