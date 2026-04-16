// Shared Pattern Utilities for DLP
//
// This module provides common pattern compilation and matching utilities
// used by both the DLP redaction engine (dlp.rs) and the test command (commands/dlp.rs).

use regex::Regex;
use std::collections::HashSet;

/// Context window size (characters before and after a match) for negative pattern checking
pub const NEGATIVE_CONTEXT_WINDOW: usize = 30;
/// Minimum token length for considering a surrounding blob as encoded/random payload.
pub const ENCODED_TOKEN_MIN_LEN: usize = 48;
/// Minimum amount of extra encoded-looking content around a match before we suppress it.
pub const ENCODED_TOKEN_MIN_EXTRA_CHARS: usize = 12;

/// Result of compiling patterns - includes both positive and negative regexes
#[derive(Clone)]
pub struct CompiledPatterns {
    pub regexes: Vec<Regex>,
    pub negative_regexes: Vec<Regex>,
}

/// Compile a list of patterns into regexes
/// - For "keyword" type: patterns are escaped and made case-insensitive
/// - For "regex" type: patterns are used as-is
/// Returns an error if any pattern is invalid
pub fn compile_patterns(patterns: &[String], pattern_type: &str) -> Result<Vec<Regex>, String> {
    let mut regexes = Vec::new();

    for p in patterns {
        if p.trim().is_empty() {
            continue;
        }

        let regex_pattern = if pattern_type == "keyword" {
            format!(r"(?i){}", regex::escape(p))
        } else {
            p.clone()
        };

        match Regex::new(&regex_pattern) {
            Ok(re) => regexes.push(re),
            Err(e) => return Err(format!("Invalid pattern '{}': {}", p, e)),
        }
    }

    Ok(regexes)
}

/// Compile both positive and negative patterns
/// Returns a CompiledPatterns struct with all compiled regexes
pub fn compile_pattern_set(
    patterns: &[String],
    pattern_type: &str,
    negative_patterns: Option<&Vec<String>>,
    negative_pattern_type: Option<&str>,
) -> Result<CompiledPatterns, String> {
    let regexes = compile_patterns(patterns, pattern_type)?;

    let negative_regexes = match negative_patterns {
        Some(neg_patterns) => {
            let neg_type = negative_pattern_type.unwrap_or("regex");
            compile_patterns(neg_patterns, neg_type)?
        }
        None => Vec::new(),
    };

    Ok(CompiledPatterns {
        regexes,
        negative_regexes,
    })
}

/// Extract context around a match position in text
/// Returns: [up to 30 chars before] + [match] + [up to 30 chars after]
pub fn get_match_context(text: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();

    // Convert byte positions to char positions
    let char_start = text[..start].chars().count();
    let char_end = text[..end].chars().count();

    // Calculate context boundaries
    let context_start = char_start.saturating_sub(NEGATIVE_CONTEXT_WINDOW);
    let context_end = (char_end + NEGATIVE_CONTEXT_WINDOW).min(text_len);

    // Extract context
    chars[context_start..context_end].iter().collect()
}

/// Check if a specific match should be excluded based on its surrounding context
/// Extracts context window around the match and checks if any negative pattern matches
pub fn is_match_excluded_by_context(
    text: &str,
    match_start: usize,
    match_end: usize,
    negative_regexes: &[Regex],
) -> bool {
    if negative_regexes.is_empty() {
        return false;
    }

    let context = get_match_context(text, match_start, match_end);

    for neg_re in negative_regexes {
        if neg_re.is_match(&context) {
            return true;
        }
    }
    false
}

fn is_encoded_blob_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-' | b'%')
}

fn encoded_blob_bounds(text: &str, match_start: usize, match_end: usize) -> (usize, usize) {
    let bytes = text.as_bytes();

    let mut start = match_start;
    while start > 0 && is_encoded_blob_byte(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = match_end;
    while end < bytes.len() && is_encoded_blob_byte(bytes[end]) {
        end += 1;
    }

    (start, end)
}

fn looks_like_hex_blob(token: &str) -> bool {
    token.len() >= 64 && token.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

fn count_url_encoded_triplets(token: &str) -> usize {
    let bytes = token.as_bytes();
    let mut i = 0usize;
    let mut count = 0usize;

    while i + 2 < bytes.len() {
        if bytes[i] == b'%' && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit()
        {
            count += 1;
            i += 3;
        } else {
            i += 1;
        }
    }

    count
}

fn looks_like_url_encoded_blob(token: &str) -> bool {
    if token.len() < ENCODED_TOKEN_MIN_LEN {
        return false;
    }

    let triplets = count_url_encoded_triplets(token);
    triplets >= 8 && triplets * 3 >= token.len() / 3
}

fn looks_like_base64ish_blob(token: &str) -> bool {
    if token.len() < ENCODED_TOKEN_MIN_LEN {
        return false;
    }

    if !token
        .as_bytes()
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-'))
    {
        return false;
    }

    let has_signal_char = token
        .as_bytes()
        .iter()
        .any(|b| matches!(b, b'+' | b'/' | b'=' | b'_' | b'-'));
    let has_upper = token.as_bytes().iter().any(|b| b.is_ascii_uppercase());
    let has_lower = token.as_bytes().iter().any(|b| b.is_ascii_lowercase());
    let has_digit = token.as_bytes().iter().any(|b| b.is_ascii_digit());
    let len_multiple_of_four = token.len() % 4 == 0;

    (has_signal_char || len_multiple_of_four) && has_upper && has_lower && has_digit
}

/// Ignore substring matches that appear inside larger encoded/random-looking blobs.
/// This suppresses accidental hits inside base64/base64url/hex/url-encoded payloads
/// without suppressing detections where the matched value is itself the whole token.
pub fn is_match_excluded_by_encoding(text: &str, match_start: usize, match_end: usize) -> bool {
    let (blob_start, blob_end) = encoded_blob_bounds(text, match_start, match_end);
    let blob = &text[blob_start..blob_end];

    if blob.len() < ENCODED_TOKEN_MIN_LEN {
        return false;
    }

    let extra_chars = (match_start - blob_start) + (blob_end - match_end);
    if extra_chars < ENCODED_TOKEN_MIN_EXTRA_CHARS {
        return false;
    }

    looks_like_hex_blob(blob)
        || looks_like_url_encoded_blob(blob)
        || looks_like_base64ish_blob(blob)
}

/// Shared exclusion logic for individual matches.
pub fn is_match_excluded(
    text: &str,
    match_start: usize,
    match_end: usize,
    negative_regexes: &[Regex],
) -> bool {
    is_match_excluded_by_context(text, match_start, match_end, negative_regexes)
        || is_match_excluded_by_encoding(text, match_start, match_end)
}

/// Count unique characters in a string
pub fn count_unique_chars(s: &str) -> usize {
    s.chars().collect::<HashSet<_>>().len()
}

/// Match result containing all unique matches
pub struct MatchResult {
    pub matches: Vec<String>,
}

/// Collect all matches from regexes with context-aware negative pattern filtering
/// - First finds all positive matches
/// - For each match, checks if any negative pattern matches within its context window
/// - Applies min_unique_chars filter to individual matches
/// - Runs optional validator function (e.g. Luhn/Verhoeff checksum)
/// - Returns unique matches (deduplicated)
pub fn collect_matches_with_negative_context(
    text: &str,
    regexes: &[Regex],
    negative_regexes: &[Regex],
    min_unique_chars: i32,
    validator: Option<fn(&str) -> bool>,
) -> MatchResult {
    let mut all_matches: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for regex in regexes {
        for m in regex.find_iter(text) {
            let matched = m.as_str().to_string();

            if seen.contains(&matched) {
                continue;
            }

            // Skip matches that are excluded by negative context or are merely
            // substrings inside larger encoded/random-looking blobs.
            if is_match_excluded(text, m.start(), m.end(), negative_regexes) {
                continue;
            }

            // Validate min_unique_chars
            if min_unique_chars > 0 {
                let unique_count = count_unique_chars(&matched);
                if (unique_count as i32) < min_unique_chars {
                    continue;
                }
            }

            // Run post-match validator (e.g. Luhn/Verhoeff checksum)
            if let Some(validate) = validator {
                if !validate(&matched) {
                    continue;
                }
            }

            seen.insert(matched.clone());
            all_matches.push(matched);
        }
    }

    MatchResult {
        matches: all_matches,
    }
}

/// Filter matches by min_occurrences threshold
/// Uses the collected match count
pub fn filter_by_min_occurrences(match_result: MatchResult, min_occurrences: i32) -> Vec<String> {
    if (match_result.matches.len() as i32) < min_occurrences {
        Vec::new()
    } else {
        match_result.matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_keyword_patterns() {
        let patterns = vec!["secret".to_string(), "password".to_string()];
        let result = compile_patterns(&patterns, "keyword").unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].is_match("SECRET"));
        assert!(result[0].is_match("secret"));
        assert!(result[1].is_match("PASSWORD"));
    }

    #[test]
    fn test_compile_regex_patterns() {
        let patterns = vec![r"sk-[a-zA-Z0-9]+".to_string()];
        let result = compile_patterns(&patterns, "regex").unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].is_match("sk-abc123"));
        assert!(!result[0].is_match("SK-ABC123")); // case-sensitive
    }

    #[test]
    fn test_invalid_pattern() {
        let patterns = vec![r"[invalid".to_string()];
        let result = compile_patterns(&patterns, "regex");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_match_context() {
        let text = "prefix text before KEY123 text after suffix";
        // KEY123 starts at position 19, ends at 25
        let context = get_match_context(text, 19, 25);
        // Should include up to 30 chars before and after
        assert!(context.contains("KEY123"));
        assert!(context.contains("before"));
        assert!(context.contains("after"));
    }

    #[test]
    fn test_context_aware_negative_matching() {
        // Scenario: API key pattern with "test" as negative
        // "sk-test123" should be excluded (test in context)
        // "sk-prod456" should NOT be excluded (no test in context)
        // Note: Keys must be >60 chars apart so their context windows don't overlap
        let text = "testing key: sk-test123 and here is some padding text that ensures the keys are far apart so production key: sk-prod456 works";
        let pos_regexes = compile_patterns(&vec![r"sk-[a-z0-9]+".to_string()], "regex").unwrap();
        let neg_regexes = compile_patterns(&vec!["test".to_string()], "keyword").unwrap();

        let result =
            collect_matches_with_negative_context(text, &pos_regexes, &neg_regexes, 0, None);

        // Only sk-prod456 should remain (sk-test123 excluded due to "testing" in context)
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0], "sk-prod456");
    }

    #[test]
    fn test_context_window_boundary() {
        // Test that context window is limited to 30 chars
        let text = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaXXXXXXbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        // XXXXXX is at position 42-48 (0-indexed)
        // Context should be 30 chars before (positions 12-42) + match + 30 chars after
        let context = get_match_context(text, 42, 48);

        // Context should not include chars before position 12
        assert!(context.len() <= 30 + 6 + 30); // 30 before + match + 30 after
    }

    #[test]
    fn test_count_unique_chars() {
        assert_eq!(count_unique_chars("aaa"), 1);
        assert_eq!(count_unique_chars("abc"), 3);
        assert_eq!(count_unique_chars("aabbcc"), 3);
    }

    #[test]
    fn test_collect_matches() {
        let regexes = compile_patterns(&vec![r"\d+".to_string()], "regex").unwrap();
        let result = collect_matches_with_negative_context("123 456 123", &regexes, &[], 0, None);
        assert_eq!(result.matches.len(), 2); // unique: 123, 456
    }

    #[test]
    fn test_encoded_blob_suppresses_internal_match() {
        let text =
            "QmFzZTY0VVJMU2VnbWVudF9QcmVmaXhfc2stQUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVowMTIzNDU2Nzg5";
        let pos_regexes =
            compile_patterns(&vec![r"sk-[A-Za-z0-9]{20,}".to_string()], "regex").unwrap();

        let result = collect_matches_with_negative_context(text, &pos_regexes, &[], 0, None);
        assert!(
            result.matches.is_empty(),
            "unexpected matches inside encoded blob: {:?}",
            result.matches
        );
    }

    #[test]
    fn test_encoded_blob_does_not_suppress_full_token_match() {
        let text = "sk-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd";
        let pos_regexes =
            compile_patterns(&vec![r"sk-[A-Za-z0-9]{20,}".to_string()], "regex").unwrap();

        let result = collect_matches_with_negative_context(text, &pos_regexes, &[], 0, None);
        assert_eq!(result.matches, vec![text.to_string()]);
    }

    #[test]
    fn test_url_encoded_blob_suppresses_internal_match() {
        let text = "%51%57%78%68%5A%47%52%70%62%6B%39%77%5A%57%35%54%52%56%4E%42%54%55%56%66%63%32%74%74%78%6F%78%62%2D%31%32%33%34%35%36%37%38%39%30%41%42%43%44%45%46%47%48%49%4A";
        let pos_regexes =
            compile_patterns(&vec![r"xox[baprs]-[a-zA-Z0-9\-]{10,}".to_string()], "regex").unwrap();

        let result = collect_matches_with_negative_context(text, &pos_regexes, &[], 0, None);
        assert!(
            result.matches.is_empty(),
            "unexpected matches inside url-encoded blob: {:?}",
            result.matches
        );
    }
}
