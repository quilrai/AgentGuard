// DLP (Data Loss Prevention) Detection Logic
//
// Compiles enabled DLP patterns from the database and checks text against
// them. Used by hook receivers (e.g. cursor_hooks) to decide whether to
// allow or block a request.

use crate::builtin_patterns::get_validator_by_name;
use crate::database::open_connection;
use crate::pattern_utils::{compile_pattern_set, count_unique_chars, is_match_excluded_by_context};
use regex::Regex;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct DlpDetection {
    pub pattern_name: String,
    pub pattern_type: String, // "keyword" or "regex"
    pub original_value: String,
    pub message_index: Option<i32>,
    /// 1-based line number within the scanned text (before applying file_line_offset)
    pub line_number: Option<usize>,
    /// 1-based column (character offset within the line)
    pub column: Option<usize>,
    /// Absolute 1-based line number in the original file (line_number + file_line_offset)
    pub absolute_line: Option<usize>,
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
    /// Optional post-match validator (e.g. Luhn checksum for credit cards)
    pub validator: Option<fn(&str) -> bool>,
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
                min_occurrences, min_unique_chars, validator_name
         FROM dlp_patterns WHERE enabled = 1",
    ) {
        Ok(s) => s,
        Err(_) => return patterns,
    };

    let db_patterns: Vec<(
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i32,
        i32,
        Option<String>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i32>(5)?,
                row.get::<_, i32>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })
        .ok()
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    for (
        name,
        pattern_type,
        patterns_json,
        negative_pattern_type,
        negative_patterns_json,
        min_occurrences,
        min_unique_chars,
        validator_name,
    ) in db_patterns
    {
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

        // Resolve validator function from name
        let validator = validator_name.as_deref().and_then(get_validator_by_name);

        if !compiled.regexes.is_empty() {
            patterns.push(CompiledDlpPattern {
                name,
                pattern_type,
                regexes: compiled.regexes,
                negative_regexes: compiled.negative_regexes,
                min_occurrences,
                min_unique_chars,
                validator,
            });
        }
    }

    patterns
}

/// Compute (1-based line number, 1-based column) for a byte offset within text.
fn position_of_byte_offset(text: &str, byte_offset: usize) -> (usize, usize) {
    let before = &text[..byte_offset];
    let line = before.matches('\n').count() + 1;
    let last_newline = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let col = byte_offset - last_newline + 1;
    (line, col)
}

/// Check text for DLP patterns (detection only).
/// `file_line_offset` is the 0-based line offset of the scanned text within
/// the original file (i.e. the agent's `offset` parameter). When set,
/// `absolute_line` in each detection = relative line + file_line_offset.
pub fn check_dlp_patterns(text: &str) -> Vec<DlpDetection> {
    check_dlp_patterns_with_offset(text, 0)
}

/// Like `check_dlp_patterns` but with an explicit file line offset so that
/// detections report absolute line numbers within the original file.
pub fn check_dlp_patterns_with_offset(text: &str, file_line_offset: usize) -> Vec<DlpDetection> {
    let patterns = get_enabled_dlp_patterns();

    if patterns.is_empty() {
        return Vec::new();
    }

    let mut detections: Vec<DlpDetection> = Vec::new();
    let mut seen_values: HashSet<String> = HashSet::new();

    for pattern in patterns {
        // Collect all matches with positions, filtering by context-aware negative patterns
        let mut valid_matches: Vec<(String, usize, usize)> = Vec::new(); // (value, line, col)

        for regex in &pattern.regexes {
            for m in regex.find_iter(text) {
                let matched = m.as_str().to_string();

                // Skip duplicates (across all patterns)
                if seen_values.contains(&matched) {
                    continue;
                }

                // Check if this match should be excluded based on its context
                // Context = 30 chars before + match + 30 chars after
                if is_match_excluded_by_context(text, m.start(), m.end(), &pattern.negative_regexes)
                {
                    continue;
                }

                // Validate min_unique_chars
                if pattern.min_unique_chars > 0 {
                    let unique_count = count_unique_chars(&matched);
                    if (unique_count as i32) < pattern.min_unique_chars {
                        continue;
                    }
                }

                // Run post-match validator (e.g. Luhn/Verhoeff checksum)
                if let Some(validate) = pattern.validator {
                    if !validate(&matched) {
                        continue;
                    }
                }

                let (line, col) = position_of_byte_offset(text, m.start());
                valid_matches.push((matched, line, col));
            }
        }

        // Check min_occurrences threshold
        if (valid_matches.len() as i32) < pattern.min_occurrences {
            continue;
        }

        for (matched, line, col) in valid_matches {
            seen_values.insert(matched.clone());

            let absolute_line = line + file_line_offset;

            detections.push(DlpDetection {
                pattern_name: pattern.name.clone(),
                pattern_type: pattern.pattern_type.clone(),
                original_value: matched,
                message_index: None,
                line_number: Some(line),
                column: Some(col),
                absolute_line: Some(absolute_line),
            });
        }
    }

    detections
}
