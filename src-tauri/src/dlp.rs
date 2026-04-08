// DLP (Data Loss Prevention) Detection Logic
//
// Compiles enabled DLP patterns from the database and checks text against
// them. Used by hook receivers (e.g. cursor_hooks) to decide whether to
// allow or block a request.

use crate::database::open_connection;
use crate::pattern_utils::{
    compile_pattern_set, count_unique_chars, is_match_excluded_by_context,
};
use regex::Regex;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct DlpDetection {
    pub pattern_name: String,
    pub pattern_type: String, // "keyword" or "regex"
    pub original_value: String,
    pub message_index: Option<i32>,
}

/// Compiled DLP pattern with all validation rules
#[derive(Clone)]
pub struct CompiledDlpPattern {
    pub name: String,
    pub pattern_type: String,
    pub regexes: Vec<Regex>,
    pub negative_regexes: Vec<Regex>,
    pub min_occurrences: i32,
    pub min_unique_chars: i32,
}

/// Get all enabled DLP patterns from database
pub fn get_enabled_dlp_patterns() -> Vec<CompiledDlpPattern> {
    let mut patterns: Vec<CompiledDlpPattern> = Vec::new();

    let conn = match open_connection() {
        Ok(c) => c,
        Err(_) => return patterns,
    };

    let mut stmt = match conn.prepare(
        "SELECT name, pattern_type, patterns, negative_pattern_type, negative_patterns,
                min_occurrences, min_unique_chars
         FROM dlp_patterns WHERE enabled = 1",
    ) {
        Ok(s) => s,
        Err(_) => return patterns,
    };

    let db_patterns: Vec<(String, String, String, Option<String>, Option<String>, i32, i32)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i32>(5)?,
                row.get::<_, i32>(6)?,
            ))
        })
        .ok()
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    for (name, pattern_type, patterns_json, negative_pattern_type, negative_patterns_json, min_occurrences, min_unique_chars) in db_patterns {
        let pattern_list: Vec<String> = serde_json::from_str(&patterns_json).unwrap_or_default();

        // Parse negative patterns if present
        let neg_pattern_list: Option<Vec<String>> = negative_patterns_json
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok());

        // Compile patterns using shared utility
        let compiled = match compile_pattern_set(
            &pattern_list,
            &pattern_type,
            neg_pattern_list.as_ref(),
            negative_pattern_type.as_deref(),
        ) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[DLP] Error compiling pattern '{}': {}", name, e);
                continue;
            }
        };

        if !compiled.regexes.is_empty() {
            patterns.push(CompiledDlpPattern {
                name,
                pattern_type,
                regexes: compiled.regexes,
                negative_regexes: compiled.negative_regexes,
                min_occurrences,
                min_unique_chars,
            });
        }
    }

    patterns
}

/// Check text for DLP patterns (detection only).
/// Used by hook receivers to decide whether to allow or block a request.
pub fn check_dlp_patterns(text: &str) -> Vec<DlpDetection> {
    let patterns = get_enabled_dlp_patterns();

    if patterns.is_empty() {
        return Vec::new();
    }

    let mut detections: Vec<DlpDetection> = Vec::new();
    let mut seen_values: HashSet<String> = HashSet::new();

    for pattern in patterns {
        // Collect all matches, filtering by context-aware negative patterns
        let mut valid_matches: Vec<String> = Vec::new();

        for regex in &pattern.regexes {
            for m in regex.find_iter(text) {
                let matched = m.as_str().to_string();

                // Skip duplicates (across all patterns)
                if seen_values.contains(&matched) {
                    continue;
                }

                // Check if this match should be excluded based on its context
                // Context = 30 chars before + match + 30 chars after
                if is_match_excluded_by_context(text, m.start(), m.end(), &pattern.negative_regexes) {
                    continue;
                }

                // Validate min_unique_chars
                if pattern.min_unique_chars > 0 {
                    let unique_count = count_unique_chars(&matched);
                    if (unique_count as i32) < pattern.min_unique_chars {
                        continue;
                    }
                }

                valid_matches.push(matched);
            }
        }

        // Check min_occurrences threshold
        if (valid_matches.len() as i32) < pattern.min_occurrences {
            continue;
        }

        for matched in valid_matches {
            seen_values.insert(matched.clone());

            detections.push(DlpDetection {
                pattern_name: pattern.name.clone(),
                pattern_type: pattern.pattern_type.clone(),
                original_value: matched,
                message_index: None,
            });
        }
    }

    detections
}
