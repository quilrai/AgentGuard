// Settings shapes shared by predefined-backend rows in the database.
//
// These structs deserialize the JSON stored in the `predefined_backend_settings`
// table. The cursor-hooks, claude-hooks, and codex-hooks routers all consume
// them at startup to pick up DLP / token-limit settings.

use serde::{Deserialize, Serialize};

/// Token saving settings with sub-category features
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenSavingSettings {
    /// Shell compression: compress shell command output before it reaches the agent
    #[serde(default)]
    pub shell_compression: bool,
    /// File read caching: cache file reads and return compact stubs for unchanged re-reads
    #[serde(default)]
    pub ctx_read: bool,
    /// Advanced: structured grep/rg output compression
    #[serde(default)]
    pub search_compressor: bool,
    /// Advanced: unified diff compression (keeps all +/- lines, trims context)
    #[serde(default)]
    pub diff_compressor: bool,
    /// Advanced: conservative JSON truncation (arrays, strings, depth)
    #[serde(default)]
    pub tool_crusher: bool,
    /// Advanced: content-hash cache for compressed outputs
    #[serde(default)]
    pub compression_cache: bool,
}

impl TokenSavingSettings {
    /// Returns true if any token saving feature is enabled
    #[allow(dead_code)]
    pub fn any_enabled(&self) -> bool {
        self.shell_compression
            || self.ctx_read
            || self.search_compressor
            || self.diff_compressor
            || self.tool_crusher
            || self.compression_cache
    }

    /// Returns true if any advanced compressor is enabled
    pub fn any_advanced_enabled(&self) -> bool {
        self.search_compressor || self.diff_compressor || self.tool_crusher
    }
}

/// Dependency protection settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DependencyProtectionSettings {
    /// Inform the agent when a newer version of a package is available
    #[serde(default)]
    pub inform_updated_packages: bool,
    /// Block packages with known vulnerabilities (checked via OSV API)
    #[serde(default)]
    pub block_malicious_packages: bool,
}

/// Settings persisted per predefined backend in the database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomBackendSettings {
    /// Whether DLP is enabled for this backend (default: true)
    #[serde(default = "default_true")]
    pub dlp_enabled: bool,
    /// Maximum tokens allowed in a request (0 = no limit)
    #[serde(default)]
    pub max_tokens_in_a_request: u32,
    /// Action to take when max tokens is exceeded: "block" or "notify" (default: "block")
    #[serde(default = "default_block")]
    pub action_for_max_tokens_in_a_request: String,
    /// Token saving settings with sub-category features
    #[serde(default)]
    pub token_saving: TokenSavingSettings,
    /// Dependency protection settings
    #[serde(default)]
    pub dependency_protection: DependencyProtectionSettings,
}

fn default_true() -> bool {
    true
}

fn default_block() -> String {
    "block".to_string()
}
