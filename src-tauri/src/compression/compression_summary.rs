// CompressionSummary — describes what was dropped so lossy compression
// stays usable.
//
// Works on two kinds of inputs:
//   1. JSON arrays of objects — categorizes by type/status/kind fields,
//      highlights notable items (errors, failures, warnings).
//   2. Text-based omission maps — groups omitted counts by file/source,
//      e.g. "42 matches omitted: 15 in utils.py, 12 in models.py".

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

static NOTABLE_RE: OnceLock<Regex> = OnceLock::new();

fn notable_re() -> &'static Regex {
    NOTABLE_RE.get_or_init(|| {
        Regex::new(
            r"(?i)error|fail|critical|warning|exception|crash|timeout|denied|rejected|invalid",
        )
        .unwrap()
    })
}

// Category fields to look for in JSON objects (ordered by priority)
const CATEGORY_FIELDS: &[&str] = &[
    "type",
    "status",
    "kind",
    "category",
    "level",
    "severity",
    "state",
    "phase",
    "action",
    "event_type",
    "log_level",
    "result",
    "outcome",
];

// ── JSON-based summaries ──────────────────────────────────────────

/// Generate a categorical summary of dropped JSON items.
///
/// `all_items`: the original full array.
/// `kept_indices`: indices of items that were kept.
/// `max_categories`: how many category buckets to show.
/// `max_notable`: how many notable items (errors/failures) to call out.
pub fn summarize_dropped_json_items(
    all_items: &[serde_json::Value],
    kept_indices: &[usize],
    max_categories: usize,
    max_notable: usize,
) -> String {
    if all_items.is_empty() || kept_indices.len() >= all_items.len() {
        return String::new();
    }

    let kept_set: std::collections::HashSet<usize> = kept_indices.iter().copied().collect();
    let dropped: Vec<&serde_json::Value> = all_items
        .iter()
        .enumerate()
        .filter(|(i, _)| !kept_set.contains(i))
        .map(|(_, v)| v)
        .collect();

    if dropped.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();

    // Categorize by type/status/kind fields
    let categories = categorize_by_fields(&dropped);
    if !categories.is_empty() {
        let mut sorted: Vec<(&String, &usize)> = categories.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        let cat_strs: Vec<String> = sorted
            .iter()
            .take(max_categories)
            .map(|(field_val, count)| format!("{} {}", count, field_val))
            .collect();
        parts.push(cat_strs.join(", "));
    }

    // Find notable items
    let notable = find_notable_items(&dropped, max_notable);
    if !notable.is_empty() {
        parts.push(format!("notable: {}", notable.join("; ")));
    }

    // Fallback: describe data shape
    if parts.is_empty() {
        let keys = common_keys(&dropped);
        if !keys.is_empty() {
            parts.push(format!("fields: {}", keys.join(", ")));
        }
    }

    parts.join("; ")
}

fn categorize_by_fields(items: &[&serde_json::Value]) -> HashMap<String, usize> {
    let mut categories: HashMap<String, usize> = HashMap::new();

    for item in items {
        if let Some(obj) = item.as_object() {
            let mut categorized = false;
            for &field in CATEGORY_FIELDS {
                if let Some(val) = obj.get(field).and_then(|v| v.as_str()) {
                    if !val.is_empty() && val.len() < 50 {
                        *categories.entry(val.to_string()).or_insert(0) += 1;
                        categorized = true;
                        break;
                    }
                }
            }
            if !categorized {
                // Try first short string field
                for (key, val) in obj {
                    if let Some(s) = val.as_str() {
                        if s.len() > 2
                            && s.len() < 30
                            && !matches!(
                                key.as_str(),
                                "id" | "name" | "path" | "url" | "href" | "email"
                            )
                            && !s.starts_with("http")
                            && !s.starts_with('/')
                        {
                            *categories.entry(format!("{}={}", key, s)).or_insert(0) += 1;
                            break;
                        }
                    }
                }
            }
        }
    }

    categories
}

fn find_notable_items(items: &[&serde_json::Value], max_notable: usize) -> Vec<String> {
    let mut notable = Vec::new();
    let re = notable_re();

    for item in items {
        let item_str = item.to_string();
        if item_str.len() > 500 {
            // Only check first 500 chars
            if let Some(m) = re.find(&item_str[..500]) {
                let name = item
                    .get("name")
                    .or_else(|| item.get("id"))
                    .or_else(|| item.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !name.is_empty() {
                    notable.push(format!("{} ({})", name, m.as_str()));
                } else {
                    notable.push(m.as_str().to_string());
                }
            }
        } else if let Some(m) = re.find(&item_str) {
            let name = item
                .get("name")
                .or_else(|| item.get("id"))
                .or_else(|| item.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !name.is_empty() {
                notable.push(format!("{} ({})", name, m.as_str()));
            } else {
                notable.push(m.as_str().to_string());
            }
        }
        if notable.len() >= max_notable {
            break;
        }
    }

    notable
}

fn common_keys(items: &[&serde_json::Value]) -> Vec<String> {
    let mut key_counts: HashMap<&str, usize> = HashMap::new();
    for item in items.iter().take(50) {
        if let Some(obj) = item.as_object() {
            for key in obj.keys() {
                *key_counts.entry(key.as_str()).or_insert(0) += 1;
            }
        }
    }
    let mut sorted: Vec<(&&str, &usize)> = key_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    sorted.iter().take(8).map(|(k, _)| k.to_string()).collect()
}

// ── Text-based summaries (for search/diff omissions) ──────────────

/// Summarize omissions from a search compressor's omission map.
/// Input: file -> count of omitted matches.
/// Output: "15 in utils.py, 12 in models.py, 8 in 3 other files"
pub fn summarize_search_omissions(omission_map: &HashMap<String, usize>) -> String {
    if omission_map.is_empty() {
        return String::new();
    }

    let mut sorted: Vec<(&String, &usize)> = omission_map.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    let mut parts: Vec<String> = Vec::new();
    let max_named = 3;

    for (file, count) in sorted.iter().take(max_named) {
        let short = shorten_path(file);
        parts.push(format!("{} in {}", count, short));
    }

    if sorted.len() > max_named {
        let remaining_files = sorted.len() - max_named;
        let remaining_count: usize = sorted.iter().skip(max_named).map(|(_, c)| **c).sum();
        parts.push(format!(
            "{} in {} other files",
            remaining_count, remaining_files
        ));
    }

    parts.join(", ")
}

/// Summarize omissions from a diff compressor.
/// E.g. "3 hunks omitted across 2 files"
#[cfg(test)]
pub fn summarize_diff_omissions(hunks_removed: usize, files_affected: usize) -> String {
    if hunks_removed == 0 {
        return String::new();
    }
    if files_affected <= 1 {
        format!("{} hunks omitted", hunks_removed)
    } else {
        format!(
            "{} hunks omitted across {} files",
            hunks_removed, files_affected
        )
    }
}

fn shorten_path(path: &str) -> &str {
    // Strip leading ./ and any long prefix directories
    let path = path.strip_prefix("./").unwrap_or(path);
    // If path has many segments, show last 2
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 3 {
        // Find the start position of the last 2 segments
        let last_two = &segments[segments.len() - 2..];
        let joined = last_two.join("/");
        // Find this suffix in the original path
        if let Some(pos) = path.rfind(&joined) {
            return &path[pos..];
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_omission_summary() {
        let mut map = HashMap::new();
        map.insert("src/utils.py".to_string(), 15);
        map.insert("src/models.py".to_string(), 12);
        map.insert("src/views.py".to_string(), 8);
        map.insert("src/other.py".to_string(), 3);
        map.insert("src/misc.py".to_string(), 2);

        let summary = summarize_search_omissions(&map);
        assert!(summary.contains("15 in"));
        assert!(summary.contains("12 in"));
        assert!(summary.contains("other files"));
    }

    #[test]
    fn json_summary_categorizes() {
        let items: Vec<serde_json::Value> = (0..10)
            .map(|i| {
                serde_json::json!({
                    "type": if i < 5 { "log" } else { "error" },
                    "message": format!("msg {}", i),
                })
            })
            .collect();

        let kept = vec![0, 1]; // Keep first 2
        let summary = summarize_dropped_json_items(&items, &kept, 5, 3);
        assert!(!summary.is_empty());
        // Should mention "log" and "error" categories
        assert!(summary.contains("log") || summary.contains("error"));
    }

    #[test]
    fn json_summary_finds_notable() {
        let items: Vec<serde_json::Value> = vec![
            serde_json::json!({"name": "test1", "status": "ok"}),
            serde_json::json!({"name": "test2", "status": "error", "message": "connection failed"}),
            serde_json::json!({"name": "test3", "status": "ok"}),
        ];

        let kept = vec![0]; // Only keep first
        let summary = summarize_dropped_json_items(&items, &kept, 5, 3);
        assert!(summary.contains("error") || summary.contains("fail"));
    }

    #[test]
    fn empty_omissions() {
        let map: HashMap<String, usize> = HashMap::new();
        assert!(summarize_search_omissions(&map).is_empty());
    }

    #[test]
    fn diff_omission_summary() {
        assert_eq!(summarize_diff_omissions(3, 1), "3 hunks omitted");
        assert_eq!(
            summarize_diff_omissions(5, 3),
            "5 hunks omitted across 3 files"
        );
        assert!(summarize_diff_omissions(0, 0).is_empty());
    }
}
