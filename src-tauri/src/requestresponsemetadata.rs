// Common Request and Response Metadata Structures
// These are the normalized formats that all backends produce

use serde::{Deserialize, Serialize};

/// Metadata extracted from API requests
#[derive(Default, Clone)]
pub struct RequestMetadata {
    pub model: Option<String>,
    pub has_system_prompt: bool,
    pub has_tools: bool,
    pub user_message_count: i32,
    pub assistant_message_count: i32,
}

/// Represents a single tool call made by the LLM
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Metadata extracted from API responses
#[derive(Default, Clone)]
pub struct ResponseMetadata {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: i32,
    pub cache_creation_tokens: i32,
    pub stop_reason: Option<String>,
    pub has_thinking: bool,
    /// Populated by passthrough backends (currently none) and by the
    /// tool-call backfill in database.rs. Not consumed by the slim server,
    /// but kept on the struct so log_request callers stay uniform.
    #[allow(dead_code)]
    pub tool_calls: Vec<ToolCall>,
}
