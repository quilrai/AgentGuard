// Advanced compression transforms for tool outputs.
//
// These sit alongside the existing shell_compression patterns as a
// higher-quality tier. The pipeline in compress.rs tries existing
// domain-specific patterns first, then falls through to these:
//
//   1. CompressionCache — SQLite-backed content-hash cache
//   2. SearchCompressor — structured grep/rg output compression
//   3. DiffCompressor   — unified diff compression
//   4. ToolCrusher      — conservative JSON truncation
//   5. CompressionSummary — describes what was dropped

pub mod compression_cache;
pub mod compression_summary;
pub mod diff_compressor;
pub mod search_compressor;
pub mod tool_crusher;

use crate::predefined_backend_settings::TokenSavingSettings;
use crate::shell_compression::tokens::count_tokens;

/// Result of running the advanced compression pipeline on a piece of text.
pub struct AdvancedCompressionResult {
    pub output: String,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    /// Which compressor was used, if any.
    pub compressor: Option<&'static str>,
}

/// Feature flags controlling which advanced compressors are active.
/// Built from TokenSavingSettings by the caller.
pub struct AdvancedCompressionFlags {
    pub search_compressor: bool,
    pub diff_compressor: bool,
    pub tool_crusher: bool,
    pub compression_cache: bool,
}

impl From<&TokenSavingSettings> for AdvancedCompressionFlags {
    fn from(s: &TokenSavingSettings) -> Self {
        Self {
            search_compressor: s.search_compressor,
            diff_compressor: s.diff_compressor,
            tool_crusher: s.tool_crusher,
            // Cache is always on when shell compression is active
            compression_cache: true,
        }
    }
}

impl AdvancedCompressionFlags {
    fn any_compressor_enabled(&self) -> bool {
        self.search_compressor || self.diff_compressor || self.tool_crusher
    }
}

/// Run the advanced compression pipeline on arbitrary text content.
///
/// Tries compressors in order:
///   1. Check CompressionCache for a cached result
///   2. Classify content and route to SearchCompressor / DiffCompressor / ToolCrusher
///   3. Store result in cache if compression was applied
///
/// Returns `None` if no compressor matched or compression wasn't beneficial.
pub fn try_advanced_compress(
    content: &str,
    command_hint: Option<&str>,
    db_path: &str,
    flags: &AdvancedCompressionFlags,
) -> Option<AdvancedCompressionResult> {
    // Bail early if nothing is enabled
    if !flags.compression_cache && !flags.any_compressor_enabled() {
        return None;
    }

    let original_tokens = count_tokens(content);

    // Don't bother with small outputs
    if original_tokens < 80 {
        return None;
    }

    // 1. Check cache (only if cache is enabled)
    let content_hash = compression_cache::content_hash(content);
    if flags.compression_cache {
        if let Some(cached) = compression_cache::get_compressed(db_path, &content_hash) {
            let compressed_tokens = count_tokens(&cached);
            if compressed_tokens < original_tokens {
                return Some(AdvancedCompressionResult {
                    output: cached,
                    original_tokens,
                    compressed_tokens,
                    compressor: Some("cache_hit"),
                });
            }
        }
    }

    // 2. Classify and compress
    let result = classify_and_compress(content, command_hint, flags);

    if let Some((compressed, compressor_name)) = result {
        let compressed_tokens = count_tokens(&compressed);
        if compressed_tokens < original_tokens {
            // Store in cache (only if cache is enabled)
            if flags.compression_cache {
                let tokens_saved = (original_tokens - compressed_tokens) as i64;
                compression_cache::store_compressed(
                    db_path,
                    &content_hash,
                    &compressed,
                    tokens_saved,
                );
            }

            return Some(AdvancedCompressionResult {
                output: compressed,
                original_tokens,
                compressed_tokens,
                compressor: Some(compressor_name),
            });
        }
    }

    None
}

/// Classify content and route to the appropriate compressor.
fn classify_and_compress(
    content: &str,
    command_hint: Option<&str>,
    flags: &AdvancedCompressionFlags,
) -> Option<(String, &'static str)> {
    // Check for unified diff format
    if flags.diff_compressor && is_unified_diff(content) {
        let result = diff_compressor::compress(content);
        if let Some(compressed) = result {
            return Some((compressed, "diff_compressor"));
        }
    }

    // Check for grep/rg search output
    if flags.search_compressor && is_search_output(content, command_hint) {
        let result = search_compressor::compress(content);
        if let Some(compressed) = result {
            return Some((compressed, "search_compressor"));
        }
    }

    // Check for JSON that could be crushed
    if flags.tool_crusher {
        let trimmed = content.trim();
        if (trimmed.starts_with('{') || trimmed.starts_with('['))
            && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
        {
            let result = tool_crusher::crush(content);
            if let Some(compressed) = result {
                return Some((compressed, "tool_crusher"));
            }
        }
    }

    None
}

fn is_unified_diff(content: &str) -> bool {
    let mut lines = content.lines();
    // Look for diff --git or --- / +++ headers in first 10 lines
    for line in lines.by_ref().take(10) {
        if line.starts_with("diff --git ")
            || line.starts_with("--- a/")
            || line.starts_with("--- /dev/null")
        {
            return true;
        }
    }
    false
}

fn is_search_output(content: &str, command_hint: Option<&str>) -> bool {
    // Command hint takes priority
    if let Some(cmd) = command_hint {
        let cl = cmd.to_ascii_lowercase();
        if cl.starts_with("grep ")
            || cl.starts_with("rg ")
            || cl.starts_with("ag ")
            || cl.starts_with("ack ")
        {
            return true;
        }
    }

    // Heuristic: count lines matching file:line:content pattern
    let mut grep_lines = 0;
    let mut total_lines = 0;
    for line in content.lines().take(20) {
        total_lines += 1;
        if search_compressor::is_grep_line(line) {
            grep_lines += 1;
        }
    }

    total_lines >= 3 && grep_lines as f64 / total_lines as f64 > 0.5
}
