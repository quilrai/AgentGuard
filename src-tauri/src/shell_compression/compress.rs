use super::patterns;
use super::tokens::count_tokens;

pub struct CompressionResult {
    pub output: String,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
}

pub fn compress_and_measure(command: &str, stdout: &str, stderr: &str) -> CompressionResult {
    let compressed_stdout = compress_if_beneficial(command, stdout);
    let compressed_stderr = compress_if_beneficial(command, stderr);

    let mut result = String::new();
    if !compressed_stdout.is_empty() {
        result.push_str(&compressed_stdout);
    }
    if !compressed_stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&compressed_stderr);
    }

    let original_stdout_tokens = count_tokens(stdout);
    let original_stderr_tokens = count_tokens(stderr);
    let original_tokens = original_stdout_tokens + original_stderr_tokens;
    let compressed_tokens = count_tokens(&result);

    CompressionResult {
        output: result,
        original_tokens,
        compressed_tokens,
    }
}

fn compress_if_beneficial(command: &str, output: &str) -> String {
    if output.trim().is_empty() {
        return String::new();
    }

    let original_tokens = count_tokens(output);

    if original_tokens < 50 {
        return output.to_string();
    }

    if let Some(compressed) = patterns::compress_output(command, output) {
        if !compressed.trim().is_empty() {
            let compressed_tokens = count_tokens(&compressed);
            if compressed_tokens < original_tokens {
                let saved = original_tokens - compressed_tokens;
                let pct = (saved as f64 / original_tokens as f64 * 100.0).round() as usize;
                return format!(
                    "{compressed}\n[compressed: {original_tokens}\u{2192}{compressed_tokens} tok, -{pct}%]"
                );
            }
        }
    }

    let lines: Vec<&str> = output.lines().collect();
    if lines.len() > 30 {
        let first = &lines[..5];
        let last = &lines[lines.len() - 5..];
        let omitted = lines.len() - 10;
        let compressed = format!(
            "{}\n... ({omitted} lines omitted) ...\n{}",
            first.join("\n"),
            last.join("\n")
        );
        let compressed_tokens = count_tokens(&compressed);
        if compressed_tokens < original_tokens {
            let saved = original_tokens - compressed_tokens;
            let pct = (saved as f64 / original_tokens as f64 * 100.0).round() as usize;
            return format!(
                "{compressed}\n[compressed: {original_tokens}\u{2192}{compressed_tokens} tok, -{pct}%]"
            );
        }
    }

    output.to_string()
}
