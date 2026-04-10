// SearchCompressor — structured grep/ripgrep output compression.
//
// Parses file:line:content format, groups by file, scores matches
// by relevance, uses adaptive sizing to pick how many to keep,
// always preserves first/last per file, and summarizes omissions.
//
// Sits alongside the existing shell_compression/patterns/grep.rs
// as a higher-quality alternative for large search outputs.

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

use super::compression_summary;

// ── Types ──────────────────────────────────────────────────────────

struct SearchMatch {
    file: String,
    line_number: usize,
    content: String,
    score: f64,
}

struct FileMatches {
    file: String,
    matches: Vec<SearchMatch>,
}

pub struct SearchCompressorConfig {
    pub max_matches_per_file: usize,
    pub always_keep_first: bool,
    pub always_keep_last: bool,
    pub max_total_matches: usize,
    pub max_files: usize,
    pub boost_errors: bool,
}

impl Default for SearchCompressorConfig {
    fn default() -> Self {
        Self {
            max_matches_per_file: 5,
            always_keep_first: true,
            always_keep_last: true,
            max_total_matches: 30,
            max_files: 15,
            boost_errors: true,
        }
    }
}

// ── Regex patterns ────────────────────────────────────────────────

static GREP_PATTERN: OnceLock<Regex> = OnceLock::new();
static PRIORITY_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

fn grep_re() -> &'static Regex {
    GREP_PATTERN.get_or_init(|| Regex::new(r"^([^:]+):(\d+):(.*)$").unwrap())
}

fn priority_patterns() -> &'static Vec<Regex> {
    PRIORITY_PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\berror\b").unwrap(),
            Regex::new(r"(?i)\bfail(ed|ure|ing)?\b").unwrap(),
            Regex::new(r"(?i)\bwarn(ing)?\b").unwrap(),
            Regex::new(r"(?i)\bpanic\b").unwrap(),
            Regex::new(r"(?i)\bexception\b").unwrap(),
            Regex::new(r"(?i)\btimeout\b").unwrap(),
        ]
    })
}

// ── Public API ────────────────────────────────────────────────────

/// Check if a line looks like grep output (file:line:content).
pub fn is_grep_line(line: &str) -> bool {
    grep_re().is_match(line)
}

/// Compress search output. Returns None if content doesn't parse as
/// search results or compression isn't beneficial.
pub fn compress(content: &str) -> Option<String> {
    compress_with_config(content, &SearchCompressorConfig::default())
}

pub fn compress_with_config(content: &str, config: &SearchCompressorConfig) -> Option<String> {
    let file_matches = parse_search_results(content);
    if file_matches.is_empty() {
        return None;
    }

    let original_count: usize = file_matches.values().map(|fm| fm.matches.len()).sum();
    if original_count <= config.max_total_matches {
        return None; // Already small enough
    }

    // Score matches
    let mut file_matches = file_matches;
    score_matches(&mut file_matches, config);

    // Select top matches with adaptive sizing
    let (selected, omission_map) = select_matches(&file_matches, config);

    // Format output
    let compressed = format_output(&selected, &omission_map);

    // Append compression summary for omitted text lines
    let total_kept: usize = selected.values().map(|fm| fm.matches.len()).sum();
    let total_omitted = original_count - total_kept;

    if total_omitted > 0 {
        let summary = compression_summary::summarize_search_omissions(&omission_map);
        if summary.is_empty() {
            Some(format!(
                "{compressed}\n[{original_count} matches compressed to {total_kept}, {total_omitted} omitted]"
            ))
        } else {
            Some(format!(
                "{compressed}\n[{original_count} matches compressed to {total_kept}: {summary}]"
            ))
        }
    } else {
        Some(compressed)
    }
}

// ── Internals ─────────────────────────────────────────────────────

fn parse_search_results(content: &str) -> HashMap<String, FileMatches> {
    let mut file_matches: HashMap<String, FileMatches> = HashMap::new();
    let re = grep_re();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(caps) = re.captures(line) {
            let file = caps[1].to_string();
            let line_num: usize = caps[2].parse().unwrap_or(0);
            let match_content = caps[3].to_string();

            let fm = file_matches
                .entry(file.clone())
                .or_insert_with(|| FileMatches {
                    file: file.clone(),
                    matches: Vec::new(),
                });

            fm.matches.push(SearchMatch {
                file,
                line_number: line_num,
                content: match_content,
                score: 0.0,
            });
        }
    }

    file_matches
}

fn score_matches(file_matches: &mut HashMap<String, FileMatches>, config: &SearchCompressorConfig) {
    for fm in file_matches.values_mut() {
        for m in &mut fm.matches {
            let mut score = 0.0f64;
            let content_lower = m.content.to_lowercase();

            // Boost error/warning patterns
            if config.boost_errors {
                for (i, pattern) in priority_patterns().iter().enumerate() {
                    if pattern.is_match(&m.content) {
                        score += 0.5 - (i as f64 * 0.05);
                        break;
                    }
                }
            }

            // Boost for definition-like patterns (def, fn, class, struct, impl)
            if content_lower.contains("fn ")
                || content_lower.contains("def ")
                || content_lower.contains("class ")
                || content_lower.contains("struct ")
                || content_lower.contains("impl ")
                || content_lower.contains("function ")
            {
                score += 0.3;
            }

            // Small boost for non-empty content
            if !m.content.trim().is_empty() {
                score += 0.1;
            }

            m.score = score.min(1.0);
        }
    }
}

fn select_matches(
    file_matches: &HashMap<String, FileMatches>,
    config: &SearchCompressorConfig,
) -> (HashMap<String, FileMatches>, HashMap<String, usize>) {
    let mut selected: HashMap<String, FileMatches> = HashMap::new();
    let mut omission_map: HashMap<String, usize> = HashMap::new();

    // Sort files by total match score (highest first)
    let mut sorted_files: Vec<(&String, &FileMatches)> = file_matches.iter().collect();
    sorted_files.sort_by(|a, b| {
        let score_a: f64 = a.1.matches.iter().map(|m| m.score).sum();
        let score_b: f64 = b.1.matches.iter().map(|m| m.score).sum();
        score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Track matches from files that are beyond max_files (entirely dropped)
    if sorted_files.len() > config.max_files {
        for (file_path, fm) in sorted_files.iter().skip(config.max_files) {
            omission_map.insert(file_path.to_string(), fm.matches.len());
        }
        sorted_files.truncate(config.max_files);
    }

    // Compute adaptive total using same logic as Python's compute_optimal_k
    let all_count: usize = sorted_files.iter().map(|(_, fm)| fm.matches.len()).sum();
    let adaptive_total = compute_optimal_k(all_count, config.max_total_matches);

    let mut total_selected = 0usize;

    for (file_path, fm) in &sorted_files {
        if total_selected >= adaptive_total {
            omission_map.insert(file_path.to_string(), fm.matches.len());
            continue;
        }

        // Sort matches by score for selection
        let mut scored: Vec<(usize, &SearchMatch)> = fm.matches.iter().enumerate().collect();
        scored.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let remaining_slots = (adaptive_total - total_selected).min(config.max_matches_per_file);
        let mut keep_indices: Vec<usize> = Vec::new();

        // Always keep first (if budget allows)
        if config.always_keep_first && !fm.matches.is_empty() && keep_indices.len() < remaining_slots {
            keep_indices.push(0);
        }

        // Always keep last (if budget allows and different from first)
        if config.always_keep_last && fm.matches.len() > 1 && keep_indices.len() < remaining_slots {
            keep_indices.push(fm.matches.len() - 1);
        }

        // Fill remaining slots with highest-scoring
        for (idx, _) in &scored {
            if keep_indices.len() >= remaining_slots {
                break;
            }
            if !keep_indices.contains(idx) {
                keep_indices.push(*idx);
            }
        }

        // Sort by line number for output ordering
        keep_indices.sort();

        let kept: Vec<SearchMatch> = keep_indices
            .iter()
            .map(|&i| {
                let m = &fm.matches[i];
                SearchMatch {
                    file: m.file.clone(),
                    line_number: m.line_number,
                    content: m.content.clone(),
                    score: m.score,
                }
            })
            .collect();

        let omitted = fm.matches.len() - kept.len();
        if omitted > 0 {
            omission_map.insert(file_path.to_string(), omitted);
        }

        total_selected += kept.len();
        selected.insert(
            file_path.to_string(),
            FileMatches {
                file: fm.file.clone(),
                matches: kept,
            },
        );
    }

    (selected, omission_map)
}

/// Adaptive sizing: compute optimal number of items to keep.
/// Mirrors Headroom's compute_optimal_k logic — scales with input size
/// but caps at max_k.
fn compute_optimal_k(total_items: usize, max_k: usize) -> usize {
    if total_items <= max_k {
        return total_items;
    }

    // sqrt-based scaling: keep roughly sqrt(n) items, capped at max_k
    let k = (total_items as f64).sqrt().ceil() as usize;
    k.clamp(5, max_k)
}

fn format_output(
    selected: &HashMap<String, FileMatches>,
    omission_map: &HashMap<String, usize>,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Sort files alphabetically for stable output
    let mut sorted: Vec<(&String, &FileMatches)> = selected.iter().collect();
    sorted.sort_by_key(|(k, _)| k.to_string());

    for (file_path, fm) in sorted {
        for m in &fm.matches {
            lines.push(format!("{}:{}:{}", m.file, m.line_number, m.content));
        }

        if let Some(&omitted) = omission_map.get(file_path.as_str()) {
            if omitted > 0 {
                lines.push(format!("[... and {} more matches in {}]", omitted, file_path));
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_grep_output() {
        let input = "src/main.rs:10:fn main() {\nsrc/main.rs:20:    println!(\"hello\");\nsrc/lib.rs:5:pub mod foo;\n";
        let matches = parse_search_results(input);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches["src/main.rs"].matches.len(), 2);
        assert_eq!(matches["src/lib.rs"].matches.len(), 1);
    }

    #[test]
    fn small_output_not_compressed() {
        let input = "src/main.rs:10:fn main() {\nsrc/main.rs:20:println!(\"hello\");\n";
        assert!(compress(input).is_none());
    }

    #[test]
    fn large_output_compressed() {
        let mut lines = Vec::new();
        for i in 0..100 {
            lines.push(format!("src/big.rs:{}:let x_{} = {};", i, i, i));
        }
        let input = lines.join("\n");
        let result = compress(&input);
        assert!(result.is_some());
        let compressed = result.unwrap();
        assert!(compressed.len() < input.len());
        assert!(compressed.contains("compressed"));
    }

    #[test]
    fn is_grep_line_works() {
        assert!(is_grep_line("src/main.rs:10:fn main() {"));
        assert!(!is_grep_line("just some text"));
        assert!(!is_grep_line(""));
    }

    #[test]
    fn compute_optimal_k_scales() {
        assert_eq!(compute_optimal_k(5, 30), 5);
        assert_eq!(compute_optimal_k(100, 30), 10);
        assert_eq!(compute_optimal_k(1000, 30), 30); // capped
        assert_eq!(compute_optimal_k(10, 30), 10);
    }
}
