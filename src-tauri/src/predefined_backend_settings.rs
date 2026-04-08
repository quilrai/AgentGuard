// Settings shapes shared by predefined-backend rows in the database.
//
// These structs deserialize the JSON stored in the `predefined_backend_settings`
// table. The cursor-hooks, claude-hooks, and codex-hooks routers all consume
// them at startup to pick up DLP / rate-limit / token-limit settings.

use serde::{Deserialize, Serialize};

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
    #[allow(dead_code)]
    pub fn any_enabled(&self) -> bool {
        self.shell_compression
    }
}

/// Settings persisted per predefined backend in the database.
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
