// Token Saving Module
//
// Applies optional token-saving transformations to request bodies before forwarding.
// Each feature is independently toggleable via per-backend TokenSavingSettings.
//
// Note: shell_compression is handled separately by the /cli_compression endpoint,
// not through this module. This module handles request-body transformations only.
//
// Currently dormant — the passthrough proxy that called into this module was
// removed. Kept as a stub for future request-body transforms once hook
// receivers are wired up.

#![allow(dead_code)]

use crate::predefined_backend_settings::TokenSavingSettings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of applying token saving transformations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSavingResult {
    /// The (possibly transformed) request body
    pub body: String,
    /// Total tokens saved across all features
    pub total_tokens_saved: i32,
    /// Per-feature breakdown: feature_name -> tokens saved
    pub feature_savings: HashMap<String, i32>,
}

impl TokenSavingResult {
    pub fn none(body: String) -> Self {
        Self {
            body,
            total_tokens_saved: 0,
            feature_savings: HashMap::new(),
        }
    }

    /// Serialize the per-feature savings as a JSON string for storage
    pub fn meta_json(&self) -> Option<String> {
        if self.total_tokens_saved == 0 {
            return None;
        }
        serde_json::to_string(&self.feature_savings).ok()
    }
}

/// Apply all enabled token-saving transformations to a request body.
/// Returns the transformed body along with savings metadata.
///
/// Note: shell_compression is not handled here — it uses a separate endpoint.
/// Future request-body features will be added here.
pub fn apply_token_saving(body: &str, _settings: &TokenSavingSettings) -> TokenSavingResult {
    // Currently no request-body transformations are implemented.
    // shell_compression is handled by the /cli_compression endpoint.
    // Future features (e.g., context trimming) would be applied here.
    TokenSavingResult::none(body.to_string())
}
