// DiffCompressor — unified diff compression.
//
// Parses git diff / diff -u output into files and hunks, preserves
// all +/- change lines and hunk/file headers, trims context lines,
// limits hunks per file, and summarizes omissions.
//
// Sits alongside the existing shell_compression/patterns/git.rs
// compress_diff() as a higher-quality alternative that preserves
// actual change content rather than just counting +/-.

use regex::Regex;
use std::sync::OnceLock;

// ── Types ──────────────────────────────────────────────────────────

struct DiffHunk {
    header: String,
    lines: Vec<String>,
    additions: usize,
    deletions: usize,
    context_lines: usize,
    score: f64,
}

impl DiffHunk {
    fn change_count(&self) -> usize {
        self.additions + self.deletions
    }
}

struct DiffFile {
    header: String,
    old_file: String,
    new_file: String,
    hunks: Vec<DiffHunk>,
    is_binary: bool,
    is_new_file: bool,
    is_deleted_file: bool,
    is_renamed: bool,
}

impl DiffFile {
    fn total_additions(&self) -> usize {
        self.hunks.iter().map(|h| h.additions).sum()
    }
    fn total_deletions(&self) -> usize {
        self.hunks.iter().map(|h| h.deletions).sum()
    }
}

pub struct DiffCompressorConfig {
    pub max_context_lines: usize,
    pub max_hunks_per_file: usize,
    pub max_files: usize,
    pub min_lines_to_compress: usize,
}

impl Default for DiffCompressorConfig {
    fn default() -> Self {
        Self {
            max_context_lines: 2,
            max_hunks_per_file: 10,
            max_files: 20,
            min_lines_to_compress: 50,
        }
    }
}

// ── Regex patterns ────────────────────────────────────────────────

static DIFF_GIT_RE: OnceLock<Regex> = OnceLock::new();
static HUNK_HEADER_RE: OnceLock<Regex> = OnceLock::new();
static OLD_FILE_RE: OnceLock<Regex> = OnceLock::new();
static NEW_FILE_RE: OnceLock<Regex> = OnceLock::new();
static BINARY_RE: OnceLock<Regex> = OnceLock::new();
static NEW_FILE_MODE_RE: OnceLock<Regex> = OnceLock::new();
static DELETED_FILE_MODE_RE: OnceLock<Regex> = OnceLock::new();
static RENAME_RE: OnceLock<Regex> = OnceLock::new();
static PRIORITY_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

fn diff_git_re() -> &'static Regex {
    DIFF_GIT_RE.get_or_init(|| Regex::new(r"^diff --git a/(.+) b/(.+)$").unwrap())
}
fn hunk_header_re() -> &'static Regex {
    HUNK_HEADER_RE
        .get_or_init(|| Regex::new(r"^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$").unwrap())
}
fn old_file_re() -> &'static Regex {
    OLD_FILE_RE.get_or_init(|| Regex::new(r"^--- (a/.+|/dev/null)$").unwrap())
}
fn new_file_re() -> &'static Regex {
    NEW_FILE_RE.get_or_init(|| Regex::new(r"^\+\+\+ (b/.+|/dev/null)$").unwrap())
}
fn binary_re() -> &'static Regex {
    BINARY_RE.get_or_init(|| Regex::new(r"^Binary files .+ differ$").unwrap())
}
fn new_file_mode_re() -> &'static Regex {
    NEW_FILE_MODE_RE.get_or_init(|| Regex::new(r"^new file mode").unwrap())
}
fn deleted_file_mode_re() -> &'static Regex {
    DELETED_FILE_MODE_RE.get_or_init(|| Regex::new(r"^deleted file mode").unwrap())
}
fn rename_re() -> &'static Regex {
    RENAME_RE.get_or_init(|| Regex::new(r"^(rename|similarity|copy) ").unwrap())
}
fn priority_patterns() -> &'static Vec<Regex> {
    PRIORITY_PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\berror\b").unwrap(),
            Regex::new(r"(?i)\bfail(ed|ure)?\b").unwrap(),
            Regex::new(r"(?i)\btodo\b").unwrap(),
            Regex::new(r"(?i)\bfixme\b").unwrap(),
            Regex::new(r"(?i)\bunsafe\b").unwrap(),
        ]
    })
}

// ── Public API ────────────────────────────────────────────────────

/// Compress unified diff output. Returns None if content is too small
/// or doesn't parse as a diff.
pub fn compress(content: &str) -> Option<String> {
    compress_with_config(content, &DiffCompressorConfig::default())
}

pub fn compress_with_config(content: &str, config: &DiffCompressorConfig) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let original_line_count = lines.len();

    if original_line_count < config.min_lines_to_compress {
        return None;
    }

    let mut diff_files = parse_diff(&lines);
    if diff_files.is_empty() {
        return None;
    }

    // Score hunks
    score_hunks(&mut diff_files);

    // Compress files and hunks
    let (compressed_files, stats) = compress_files(&mut diff_files, config);

    // Format output
    let output = format_output(&compressed_files, &stats);
    let compressed_line_count = output.lines().count();

    // Only return if we actually saved something
    if compressed_line_count >= original_line_count {
        return None;
    }

    Some(output)
}

// ── Parser ────────────────────────────────────────────────────────

fn parse_diff(lines: &[&str]) -> Vec<DiffFile> {
    let mut diff_files: Vec<DiffFile> = Vec::new();
    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;

    for line in lines {
        // New file section
        if diff_git_re().is_match(line) {
            // Save previous hunk and file
            if let (Some(ref mut file), Some(hunk)) = (&mut current_file, current_hunk.take()) {
                file.hunks.push(hunk);
            }
            if let Some(file) = current_file.take() {
                diff_files.push(file);
            }

            current_file = Some(DiffFile {
                header: line.to_string(),
                old_file: String::new(),
                new_file: String::new(),
                hunks: Vec::new(),
                is_binary: false,
                is_new_file: false,
                is_deleted_file: false,
                is_renamed: false,
            });
            continue;
        }

        if let Some(ref mut file) = current_file {
            if new_file_mode_re().is_match(line) {
                file.is_new_file = true;
            } else if deleted_file_mode_re().is_match(line) {
                file.is_deleted_file = true;
            } else if rename_re().is_match(line) {
                file.is_renamed = true;
            } else if binary_re().is_match(line) {
                file.is_binary = true;
            }
        }

        if old_file_re().is_match(line) {
            if let Some(ref mut file) = current_file {
                file.old_file = line.to_string();
            }
            continue;
        }

        if new_file_re().is_match(line) {
            if let Some(ref mut file) = current_file {
                file.new_file = line.to_string();
            }
            continue;
        }

        // Hunk header
        if hunk_header_re().is_match(line) {
            if let (Some(ref mut file), Some(hunk)) = (&mut current_file, current_hunk.take()) {
                file.hunks.push(hunk);
            }
            current_hunk = Some(DiffHunk {
                header: line.to_string(),
                lines: Vec::new(),
                additions: 0,
                deletions: 0,
                context_lines: 0,
                score: 0.0,
            });
            continue;
        }

        // Hunk content
        if let Some(ref mut hunk) = current_hunk {
            if line.starts_with('+') && !line.starts_with("+++") {
                hunk.additions += 1;
                hunk.lines.push(line.to_string());
            } else if line.starts_with('-') && !line.starts_with("---") {
                hunk.deletions += 1;
                hunk.lines.push(line.to_string());
            } else if line.starts_with(' ') || line.is_empty() {
                hunk.context_lines += 1;
                hunk.lines.push(line.to_string());
            } else {
                // Other lines (e.g. "\ No newline at end of file")
                hunk.lines.push(line.to_string());
            }
        }
    }

    // Save final hunk and file
    if let (Some(ref mut file), Some(hunk)) = (&mut current_file, current_hunk.take()) {
        file.hunks.push(hunk);
    }
    if let Some(file) = current_file.take() {
        diff_files.push(file);
    }

    diff_files
}

fn score_hunks(diff_files: &mut Vec<DiffFile>) {
    for diff_file in diff_files.iter_mut() {
        for hunk in &mut diff_file.hunks {
            let mut score = 0.0f64;

            // Base score from change count
            score += (hunk.change_count() as f64 * 0.03).min(0.3);

            // Boost for priority patterns in change lines
            let hunk_content: String = hunk.lines.join("\n");
            for pattern in priority_patterns() {
                if pattern.is_match(&hunk_content) {
                    score += 0.3;
                    break;
                }
            }

            hunk.score = score.min(1.0);
        }
    }
}

fn compress_files(
    diff_files: &mut Vec<DiffFile>,
    config: &DiffCompressorConfig,
) -> (Vec<CompressedFile>, DiffStats) {
    let mut stats = DiffStats::default();

    // Collect total stats from ALL files before any truncation
    stats.files_affected = diff_files.len();
    stats.total_additions = diff_files.iter().map(|f| f.total_additions()).sum();
    stats.total_deletions = diff_files.iter().map(|f| f.total_deletions()).sum();

    // Limit files (after collecting totals)
    if diff_files.len() > config.max_files {
        diff_files.sort_by(|a, b| {
            let changes_a = a.total_additions() + a.total_deletions();
            let changes_b = b.total_additions() + b.total_deletions();
            changes_b.cmp(&changes_a)
        });
        stats.files_omitted = diff_files.len() - config.max_files;
        diff_files.truncate(config.max_files);
    }

    let mut compressed_files: Vec<CompressedFile> = Vec::new();

    for diff_file in diff_files.iter() {

        let (compressed_hunks, hunks_removed) = compress_hunks(&diff_file.hunks, config);
        stats.hunks_kept += compressed_hunks.len();
        stats.hunks_removed += hunks_removed;

        compressed_files.push(CompressedFile {
            header: diff_file.header.clone(),
            old_file: diff_file.old_file.clone(),
            new_file: diff_file.new_file.clone(),
            hunks: compressed_hunks,
            is_binary: diff_file.is_binary,
            is_new_file: diff_file.is_new_file,
            is_deleted_file: diff_file.is_deleted_file,
        });
    }

    (compressed_files, stats)
}

fn compress_hunks(hunks: &[DiffHunk], config: &DiffCompressorConfig) -> (Vec<CompressedHunk>, usize) {
    if hunks.is_empty() {
        return (Vec::new(), 0);
    }

    let mut selected_hunks: Vec<&DiffHunk> = Vec::new();
    let mut removed = 0usize;

    if hunks.len() <= config.max_hunks_per_file {
        // Keep all hunks
        selected_hunks = hunks.iter().collect();
    } else {
        // Always keep first and last
        selected_hunks.push(&hunks[0]);
        if hunks.len() > 1 {
            // Select middle hunks by score
            let mut middle: Vec<(usize, &DiffHunk)> = hunks[1..hunks.len() - 1]
                .iter()
                .enumerate()
                .map(|(i, h)| (i + 1, h))
                .collect();
            middle.sort_by(|a, b| {
                b.1.score
                    .partial_cmp(&a.1.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let remaining = config.max_hunks_per_file - 2;
            let mut keep_indices: Vec<usize> = vec![0];
            for (idx, _) in middle.iter().take(remaining) {
                keep_indices.push(*idx);
            }
            keep_indices.push(hunks.len() - 1);
            keep_indices.sort();

            selected_hunks = keep_indices.iter().map(|&i| &hunks[i]).collect();
        }
        removed = hunks.len() - selected_hunks.len();
    }

    // Reduce context in each selected hunk
    let compressed: Vec<CompressedHunk> = selected_hunks
        .iter()
        .map(|hunk| reduce_context(hunk, config.max_context_lines))
        .collect();

    (compressed, removed)
}

fn reduce_context(hunk: &DiffHunk, max_context: usize) -> CompressedHunk {
    // Find positions of change lines
    let change_positions: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with('+') || line.starts_with('-'))
        .map(|(i, _)| i)
        .collect();

    if change_positions.is_empty() {
        // No changes, keep minimal context
        let kept: Vec<String> = hunk.lines.iter().take(max_context).cloned().collect();
        return CompressedHunk {
            header: hunk.header.clone(),
            lines: kept,
        };
    }

    // Determine which lines to keep: changes + context around them
    let mut keep: Vec<bool> = vec![false; hunk.lines.len()];

    for &pos in &change_positions {
        keep[pos] = true;
        // Context before
        let start = pos.saturating_sub(max_context);
        for i in start..pos {
            keep[i] = true;
        }
        // Context after
        let end = (pos + max_context + 1).min(hunk.lines.len());
        for i in (pos + 1)..end {
            keep[i] = true;
        }
    }

    let kept_lines: Vec<String> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .map(|(_, line)| line.clone())
        .collect();

    CompressedHunk {
        header: hunk.header.clone(),
        lines: kept_lines,
    }
}

// ── Output types ──────────────────────────────────────────────────

struct CompressedHunk {
    header: String,
    lines: Vec<String>,
}

struct CompressedFile {
    header: String,
    old_file: String,
    new_file: String,
    hunks: Vec<CompressedHunk>,
    is_binary: bool,
    is_new_file: bool,
    is_deleted_file: bool,
}

#[derive(Default)]
struct DiffStats {
    files_affected: usize,
    files_omitted: usize,
    total_additions: usize,
    total_deletions: usize,
    hunks_kept: usize,
    hunks_removed: usize,
}

fn format_output(files: &[CompressedFile], stats: &DiffStats) -> String {
    let mut output: Vec<String> = Vec::new();

    for file in files {
        output.push(file.header.clone());

        if file.is_new_file {
            output.push("new file mode 100644".to_string());
        } else if file.is_deleted_file {
            output.push("deleted file mode 100644".to_string());
        }

        if file.is_binary {
            output.push("Binary files differ".to_string());
            continue;
        }

        if !file.old_file.is_empty() {
            output.push(file.old_file.clone());
        }
        if !file.new_file.is_empty() {
            output.push(file.new_file.clone());
        }

        for hunk in &file.hunks {
            output.push(hunk.header.clone());
            output.extend(hunk.lines.iter().cloned());
        }
    }

    // Summary line
    if stats.hunks_removed > 0 || stats.files_affected > 0 || stats.files_omitted > 0 {
        let mut parts = vec![
            format!("{} files changed", stats.files_affected),
            format!("+{} -{} lines", stats.total_additions, stats.total_deletions),
        ];
        if stats.files_omitted > 0 {
            parts.push(format!("{} files omitted", stats.files_omitted));
        }
        if stats.hunks_removed > 0 {
            parts.push(format!("{} hunks omitted", stats.hunks_removed));
        }
        output.push(format!("[{}]", parts.join(", ")));
    }

    output.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_diff() -> String {
        let mut lines = Vec::new();
        lines.push("diff --git a/src/main.rs b/src/main.rs");
        lines.push("--- a/src/main.rs");
        lines.push("+++ b/src/main.rs");
        lines.push("@@ -1,10 +1,12 @@");
        lines.push(" fn main() {");
        lines.push("-    let x = 1;");
        lines.push("+    let x = 2;");
        lines.push("+    let y = 3;");
        // Add lots of context to make it compressible
        for _ in 0..50 {
            lines.push("     // context line");
        }
        lines.push("diff --git a/src/lib.rs b/src/lib.rs");
        lines.push("--- a/src/lib.rs");
        lines.push("+++ b/src/lib.rs");
        lines.push("@@ -1,5 +1,6 @@");
        lines.push(" pub mod foo;");
        lines.push("+pub mod bar;");
        lines.join("\n")
    }

    #[test]
    fn parses_diff() {
        let diff = sample_diff();
        let lines: Vec<&str> = diff.lines().collect();
        let files = parse_diff(&lines);
        assert_eq!(files.len(), 2);
        assert!(files[0].hunks[0].additions >= 2);
        assert_eq!(files[0].hunks[0].deletions, 1);
    }

    #[test]
    fn compresses_large_diff() {
        let diff = sample_diff();
        let result = compress(&diff);
        assert!(result.is_some());
        let compressed = result.unwrap();
        // Should still have the actual changes
        assert!(compressed.contains("+    let x = 2;"));
        assert!(compressed.contains("-    let x = 1;"));
        // But should be shorter
        assert!(compressed.len() < diff.len());
    }

    #[test]
    fn small_diff_not_compressed() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,4 @@\n fn f() {\n+    x();\n }\n";
        assert!(compress(diff).is_none());
    }
}
