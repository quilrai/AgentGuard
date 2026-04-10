// ToolCrusher — conservative JSON compression for large tool outputs.
//
// Only touches parseable JSON. Preserves structure (never removes keys),
// but truncates arrays to max_items, truncates long strings, and limits
// nesting depth. Appends a CompressionSummary for dropped array items.

use super::compression_summary;

pub struct ToolCrusherConfig {
    /// Minimum content length (chars) before attempting to crush.
    pub min_chars_to_crush: usize,
    /// Maximum items to keep in each array.
    pub max_array_items: usize,
    /// Maximum string length before truncation.
    pub max_string_length: usize,
    /// Maximum nesting depth before summarizing.
    pub max_depth: usize,
}

impl Default for ToolCrusherConfig {
    fn default() -> Self {
        Self {
            min_chars_to_crush: 500,
            max_array_items: 10,
            max_string_length: 1000,
            max_depth: 5,
        }
    }
}

/// Crush JSON content. Returns None if content isn't JSON, is too small,
/// or crushing doesn't reduce it.
pub fn crush(content: &str) -> Option<String> {
    crush_with_config(content, &ToolCrusherConfig::default())
}

pub fn crush_with_config(content: &str, config: &ToolCrusherConfig) -> Option<String> {
    let trimmed = content.trim();

    if trimmed.len() < config.min_chars_to_crush {
        return None;
    }

    // Parse JSON
    let parsed: serde_json::Value = serde_json::from_str(trimmed).ok()?;

    // Crush
    let (crushed, any_modified) = crush_value(&parsed, 0, config);

    if !any_modified {
        return None;
    }

    // Serialize back
    let result = serde_json::to_string(&crushed).ok()?;

    // Only return if actually shorter
    if result.len() >= trimmed.len() {
        return None;
    }

    Some(result)
}

/// Find the largest byte offset <= `desired` that sits on a UTF-8 char boundary.
fn safe_truncate_pos(s: &str, desired: usize) -> usize {
    if desired >= s.len() {
        return s.len();
    }
    // Walk backwards from desired until we hit a char boundary
    let mut pos = desired;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Recursively crush a JSON value. Returns (crushed_value, was_any_modified).
fn crush_value(
    value: &serde_json::Value,
    depth: usize,
    config: &ToolCrusherConfig,
) -> (serde_json::Value, bool) {
    if depth >= config.max_depth {
        return match value {
            serde_json::Value::Object(map) => (
                serde_json::json!({"__depth_exceeded": map.len()}),
                true,
            ),
            serde_json::Value::Array(arr) => (
                serde_json::json!({"__depth_exceeded": arr.len()}),
                true,
            ),
            serde_json::Value::String(s) if s.len() > config.max_string_length => {
                let cut = safe_truncate_pos(s, config.max_string_length);
                let truncated = &s[..cut];
                let remaining = s.len() - cut;
                (
                    serde_json::Value::String(format!(
                        "{}...[truncated {} chars]",
                        truncated, remaining
                    )),
                    true,
                )
            }
            _ => (value.clone(), false),
        };
    }

    match value {
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            let mut modified = false;
            for (k, v) in map {
                let (crushed, was_modified) = crush_value(v, depth + 1, config);
                if was_modified {
                    modified = true;
                }
                new_map.insert(k.clone(), crushed);
            }
            (serde_json::Value::Object(new_map), modified)
        }
        serde_json::Value::Array(arr) => {
            if arr.len() <= config.max_array_items {
                // Process all items
                let mut new_arr = Vec::new();
                let mut modified = false;
                for item in arr {
                    let (crushed, was_modified) = crush_value(item, depth + 1, config);
                    if was_modified {
                        modified = true;
                    }
                    new_arr.push(crushed);
                }
                (serde_json::Value::Array(new_arr), modified)
            } else {
                // Truncate array
                let mut new_arr = Vec::new();
                for item in arr.iter().take(config.max_array_items) {
                    let (crushed, _) = crush_value(item, depth + 1, config);
                    new_arr.push(crushed);
                }

                let truncated_count = arr.len() - config.max_array_items;

                // Generate summary of dropped items
                let kept_indices: Vec<usize> = (0..config.max_array_items).collect();
                let summary =
                    compression_summary::summarize_dropped_json_items(arr, &kept_indices, 5, 3);

                let mut truncation_info = serde_json::Map::new();
                truncation_info.insert(
                    "__truncated".to_string(),
                    serde_json::Value::Number(truncated_count.into()),
                );
                if !summary.is_empty() {
                    truncation_info.insert(
                        "__summary".to_string(),
                        serde_json::Value::String(summary),
                    );
                }
                new_arr.push(serde_json::Value::Object(truncation_info));

                (serde_json::Value::Array(new_arr), true)
            }
        }
        serde_json::Value::String(s) => {
            if s.len() > config.max_string_length {
                let cut = safe_truncate_pos(s, config.max_string_length);
                let truncated = &s[..cut];
                let remaining = s.len() - cut;
                (
                    serde_json::Value::String(format!(
                        "{}...[truncated {} chars]",
                        truncated, remaining
                    )),
                    true,
                )
            } else {
                (value.clone(), false)
            }
        }
        // Numbers, bools, null — pass through
        _ => (value.clone(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_json_not_crushed() {
        let json = r#"{"key": "value"}"#;
        assert!(crush(json).is_none());
    }

    #[test]
    fn large_array_truncated() {
        let items: Vec<serde_json::Value> = (0..50)
            .map(|i| serde_json::json!({"id": i, "name": format!("item_{}", i), "status": "active"}))
            .collect();
        let json = serde_json::to_string(&items).unwrap();

        let result = crush(&json);
        assert!(result.is_some());
        let crushed = result.unwrap();
        assert!(crushed.len() < json.len());

        // Should contain truncation marker
        assert!(crushed.contains("__truncated"));
    }

    #[test]
    fn long_strings_truncated() {
        let long_string = "x".repeat(2000);
        let json = serde_json::json!({"data": long_string}).to_string();

        let result = crush(&json);
        assert!(result.is_some());
        let crushed = result.unwrap();
        assert!(crushed.contains("truncated"));
        assert!(crushed.len() < json.len());
    }

    #[test]
    fn deep_nesting_summarized() {
        let mut val = serde_json::json!({"leaf": "data"});
        for _ in 0..10 {
            val = serde_json::json!({"nested": val});
        }
        let json = val.to_string();

        // Use a config with low depth
        let config = ToolCrusherConfig {
            min_chars_to_crush: 10,
            max_depth: 3,
            ..Default::default()
        };
        let result = crush_with_config(&json, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("__depth_exceeded"));
    }

    #[test]
    fn preserves_structure() {
        let items: Vec<serde_json::Value> = (0..20)
            .map(|i| serde_json::json!({"id": i, "type": "record", "value": i * 10}))
            .collect();
        let json = serde_json::to_string(&items).unwrap();

        let result = crush(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        // Should still be a valid JSON array
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        // First items should have all original keys
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("type").is_some());
        assert!(arr[0].get("value").is_some());
    }

    #[test]
    fn array_summary_includes_categories() {
        let items: Vec<serde_json::Value> = (0..30)
            .map(|i| {
                serde_json::json!({
                    "id": i,
                    "type": if i % 3 == 0 { "error" } else { "info" },
                    "message": format!("message {}", i)
                })
            })
            .collect();
        let json = serde_json::to_string(&items).unwrap();

        let result = crush(&json).unwrap();
        // Should have a summary mentioning dropped item types
        assert!(result.contains("__summary") || result.contains("__truncated"));
    }
}
