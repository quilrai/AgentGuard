use std::path::Path;

use super::cache::SessionCache;
use super::compressor;
use super::protocol;
use super::ReadResult;
use crate::shell_compression::tokens::count_tokens;

/// Read a file with the given mode. Manages session cache automatically.
///
/// Supported modes: "full", "diff", "lines:N-M,X-Y"
/// Unsupported modes fall back to "full".
///
/// Returns per-request token accounting in `ReadResult`.
pub fn handle(cache: &mut SessionCache, path: &str, mode: &str, fresh: bool) -> ReadResult {
    let file_ref = cache.get_file_ref(path);
    let short = protocol::shorten_path(path);
    let _ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if fresh {
        cache.invalidate(path);
    }

    if mode == "diff" {
        return handle_diff(cache, path, &file_ref);
    }

    // lines: mode — always re-read from disk to avoid staleness
    if mode.starts_with("lines:") {
        return handle_lines(cache, path, mode, &file_ref, &short);
    }

    if cache.get(path).is_some() {
        return handle_full_with_auto_delta(cache, path, &file_ref, &short);
    }

    // Not cached: read from disk and store
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let output = format!("ERROR: {e}");
            let sent = count_tokens(&output);
            return ReadResult {
                output,
                original_tokens: 0,
                sent_tokens: sent,
            };
        }
    };

    let (entry, _is_hit) = cache.store(path, content.clone());
    format_full_result(cache, &file_ref, &short, &content, &entry)
}

const AUTO_DELTA_THRESHOLD: f64 = 0.6;

/// Re-reads from disk; if content changed and delta is compact, sends auto-delta.
fn handle_full_with_auto_delta(
    cache: &mut SessionCache,
    path: &str,
    file_ref: &str,
    short: &str,
) -> ReadResult {
    let disk_content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            // Disk read failed — return cached stub
            cache.record_cache_hit(path);
            let existing = cache.get(path).unwrap();
            let original_tokens = existing.original_tokens;
            let output = format!(
                "{file_ref}={short} cached {}t {}L",
                existing.read_count, existing.line_count
            );
            let sent = count_tokens(&output);
            cache.record_sent_tokens(sent);
            return ReadResult {
                output,
                original_tokens,
                sent_tokens: sent,
            };
        }
    };

    let old_content = cache.get(path).unwrap().content.clone();
    let (entry, is_hit) = cache.store(path, disk_content.clone());
    let original_tokens = entry.original_tokens;

    if is_hit {
        let output = format!(
            "{file_ref}={short} cached {}t {}L",
            entry.read_count, entry.line_count
        );
        let sent = count_tokens(&output);
        cache.record_sent_tokens(sent);
        return ReadResult {
            output,
            original_tokens,
            sent_tokens: sent,
        };
    }

    // Content changed — try auto-delta
    let diff = compressor::diff_content(&old_content, &disk_content);
    let diff_tokens = count_tokens(&diff);
    let full_tokens = entry.original_tokens;

    if full_tokens > 0 && (diff_tokens as f64) < (full_tokens as f64 * AUTO_DELTA_THRESHOLD) {
        let savings = protocol::format_savings(full_tokens, diff_tokens);
        let output = format!(
            "{file_ref}={short} [auto-delta] ∆{}L\n{diff}\n{savings}",
            disk_content.lines().count()
        );
        let sent = count_tokens(&output);
        cache.record_sent_tokens(sent);
        return ReadResult {
            output,
            original_tokens,
            sent_tokens: sent,
        };
    }

    format_full_result(cache, file_ref, short, &disk_content, &entry)
}

fn format_full_result(
    cache: &mut SessionCache,
    file_ref: &str,
    short: &str,
    content: &str,
    entry: &super::cache::CacheEntry,
) -> ReadResult {
    let original_tokens = entry.original_tokens;
    let header = format!("{file_ref}={short} {}L", entry.line_count);
    let output = format!("{header}\n{content}");
    let sent = count_tokens(&output);
    let savings = protocol::format_savings(original_tokens, sent);
    let output = format!("{output}\n{savings}");
    let sent = count_tokens(&output);
    cache.record_sent_tokens(sent);
    ReadResult {
        output,
        original_tokens,
        sent_tokens: sent,
    }
}

fn handle_diff(cache: &mut SessionCache, path: &str, file_ref: &str) -> ReadResult {
    let short = protocol::shorten_path(path);
    let old_content = cache.get(path).map(|e| e.content.clone());

    let new_content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let output = format!("ERROR: {e}");
            let sent = count_tokens(&output);
            return ReadResult {
                output,
                original_tokens: 0,
                sent_tokens: sent,
            };
        }
    };

    let original_tokens = count_tokens(&new_content);

    let diff_output = if let Some(old) = &old_content {
        compressor::diff_content(old, &new_content)
    } else {
        format!("[first read]\n{new_content}")
    };

    cache.store(path, new_content);

    let sent_content = count_tokens(&diff_output);
    let savings = protocol::format_savings(original_tokens, sent_content);
    let output = format!("{file_ref}={short} [diff]\n{diff_output}\n{savings}");
    let sent = count_tokens(&output);
    cache.record_sent_tokens(sent);
    ReadResult {
        output,
        original_tokens,
        sent_tokens: sent,
    }
}

/// Handle lines: mode. Re-reads from disk and updates cache to avoid staleness.
fn handle_lines(
    cache: &mut SessionCache,
    path: &str,
    mode: &str,
    file_ref: &str,
    short: &str,
) -> ReadResult {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let output = format!("ERROR: {e}");
            let sent = count_tokens(&output);
            return ReadResult {
                output,
                original_tokens: 0,
                sent_tokens: sent,
            };
        }
    };

    let (entry, _is_hit) = cache.store(path, content.clone());
    let original_tokens = entry.original_tokens;

    let range_str = &mode[6..]; // strip "lines:"
    let extracted = extract_line_range(&content, range_str);
    let line_count = content.lines().count();
    let header = format!("{file_ref}={short} {line_count}L lines:{range_str}");
    let sent_content = count_tokens(&extracted);
    let savings = protocol::format_savings(original_tokens, sent_content);
    let output = format!("{header}\n{extracted}\n{savings}");
    let sent = count_tokens(&output);
    cache.record_sent_tokens(sent);
    ReadResult {
        output,
        original_tokens,
        sent_tokens: sent,
    }
}

fn extract_line_range(content: &str, range_str: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let mut selected = Vec::new();

    for part in range_str.split(',') {
        let part = part.trim();
        if let Some((start_s, end_s)) = part.split_once('-') {
            let start = start_s.trim().parse::<usize>().unwrap_or(1).max(1);
            let end = end_s.trim().parse::<usize>().unwrap_or(total).min(total);
            for i in start..=end {
                if i >= 1 && i <= total {
                    selected.push(format!("{i:>4}| {}", lines[i - 1]));
                }
            }
        } else if let Ok(n) = part.parse::<usize>() {
            if n >= 1 && n <= total {
                selected.push(format!("{n:>4}| {}", lines[n - 1]));
            }
        }
    }

    if selected.is_empty() {
        "No lines matched the range.".to_string()
    } else {
        selected.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_range() {
        let content = "a\nb\nc\nd\ne";
        let result = extract_line_range(content, "2-4");
        assert!(result.contains("b"));
        assert!(result.contains("c"));
        assert!(result.contains("d"));
        assert!(!result.contains("a"));
        assert!(!result.contains("e"));
    }

    #[test]
    fn extract_comma_separated() {
        let content = "a\nb\nc\nd\ne";
        let result = extract_line_range(content, "1-2,4-5");
        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(result.contains("d"));
        assert!(result.contains("e"));
    }

    #[test]
    fn extract_single_line() {
        let content = "a\nb\nc";
        let result = extract_line_range(content, "2");
        assert!(result.contains("b"));
        assert!(!result.contains("a"));
    }

    #[test]
    fn extract_out_of_range() {
        let content = "a\nb";
        let result = extract_line_range(content, "100-200");
        assert_eq!(result, "No lines matched the range.");
    }
}
