mod compress;
mod executor;
pub mod patterns;
pub mod tokens;

use std::collections::HashMap;

pub struct ShellCompressionResult {
    pub output: String,
    pub exit_code: i32,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub duration_ms: u64,
}

/// Execute a shell command, compress its output, and return the result.
/// `tokens_saved` is intentionally not stored here — the proxy mutates the
/// output (e.g. appending `[exit: N]`) before sending it to the agent, so it
/// recomputes the saved count from `original_tokens`/`compressed_tokens` after
/// any mutation.
pub fn compress_command(
    command: &str,
    cwd: Option<&str>,
    shell: Option<&str>,
    env: Option<&HashMap<String, String>>,
    flags: &crate::compression::AdvancedCompressionFlags,
) -> ShellCompressionResult {
    let cmd_result = executor::run_command(command, cwd, shell, env);

    let comp =
        compress::compress_and_measure(command, &cmd_result.stdout, &cmd_result.stderr, flags);

    ShellCompressionResult {
        output: comp.output,
        exit_code: cmd_result.exit_code,
        original_tokens: comp.original_tokens,
        compressed_tokens: comp.compressed_tokens,
        duration_ms: cmd_result.duration_ms,
    }
}

/// Compress output that was already produced by an agent shell tool.
///
/// Unlike `compress_command`, this does not execute anything. It is used by
/// post-tool hook surfaces such as Codex `PostToolUse`, where the command has
/// already completed and the hook can only replace what the model sees next.
pub fn compress_captured_output(
    command: &str,
    stdout: &str,
    stderr: &str,
    flags: &crate::compression::AdvancedCompressionFlags,
) -> ShellCompressionResult {
    let comp = compress::compress_and_measure(command, stdout, stderr, flags);

    ShellCompressionResult {
        output: comp.output,
        exit_code: 0,
        original_tokens: comp.original_tokens,
        compressed_tokens: comp.compressed_tokens,
        duration_ms: 0,
    }
}

/// Execute a shell command without compression, return raw output.
pub fn run_command_raw(
    command: &str,
    cwd: Option<&str>,
    shell: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> ShellCompressionResult {
    let cmd_result = executor::run_command(command, cwd, shell, env);

    let mut output = cmd_result.stdout;
    if !cmd_result.stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&cmd_result.stderr);
    }

    let token_count = tokens::count_tokens(&output);

    ShellCompressionResult {
        output,
        exit_code: cmd_result.exit_code,
        original_tokens: token_count,
        compressed_tokens: token_count,
        duration_ms: cmd_result.duration_ms,
    }
}
