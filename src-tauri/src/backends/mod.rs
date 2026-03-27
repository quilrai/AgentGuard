// Backend trait and implementations

pub mod claude;
pub mod codex;
pub mod custom;

use axum::http::HeaderMap;
use crate::backends::custom::TokenSavingSettings;
use crate::requestresponsemetadata::{RequestMetadata, ResponseMetadata};

/// Trait for API backend implementations
/// Each backend (Claude, OpenAI, Gemini, etc.) implements this trait
pub trait Backend: Send + Sync {
    /// Returns the backend name (e.g., "claude", "openai", "gemini")
    fn name(&self) -> &str;

    /// Returns the base URL for this backend's API
    fn base_url(&self) -> &str;

    /// Parse request body to extract metadata
    fn parse_request_metadata(&self, body: &str) -> RequestMetadata;

    /// Parse response body to extract metadata
    fn parse_response_metadata(&self, body: &str, is_streaming: bool) -> ResponseMetadata;

    /// Determine if this request should be logged
    /// (e.g., only log Messages API calls, not token counting)
    fn should_log(&self, body: &str) -> bool;

    /// Extract backend-specific metadata as JSON string
    /// This is stored in the extra_metadata column for flexible, backend-specific data
    /// Default implementation returns None (no extra metadata)
    fn extract_extra_metadata(
        &self,
        _request_body: &str,
        _response_body: &str,
        _headers: &HeaderMap,
    ) -> Option<String> {
        None
    }

    /// Check if DLP is enabled for this backend
    /// Default implementation returns true (DLP enabled)
    fn is_dlp_enabled(&self) -> bool {
        true
    }

    /// Get rate limit settings (requests per window, window in minutes)
    /// Returns (0, 1) by default which means no rate limit
    fn get_rate_limit(&self) -> (u32, u32) {
        (0, 1)
    }

    /// Get max tokens limit settings (max_tokens, action)
    /// action is "block" or "notify"
    /// Returns (0, "block") by default which means no token limit
    fn get_max_tokens_limit(&self) -> (u32, String) {
        (0, "block".to_string())
    }

    /// Get token saving settings
    /// Returns default (all disabled) by default
    fn get_token_saving_settings(&self) -> TokenSavingSettings {
        TokenSavingSettings::default()
    }
}

// Re-export backends for convenience
pub use claude::ClaudeBackend;
pub use codex::CodexBackend;
pub use custom::CustomBackend;
