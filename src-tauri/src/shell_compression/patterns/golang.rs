use regex::Regex;
use std::sync::OnceLock;

static GO_TEST_RESULT_RE: OnceLock<Regex> = OnceLock::new();
static GO_BENCH_RE: OnceLock<Regex> = OnceLock::new();
static GOLINT_RE: OnceLock<Regex> = OnceLock::new();
static GO_BUILD_ERROR_RE: OnceLock<Regex> = OnceLock::new();

fn go_test_result_re() -> &'static Regex {
    GO_TEST_RESULT_RE.get_or_init(|| Regex::new(r"^(ok|FAIL)\s+(\S+)\s+(\S+)").unwrap())
}
fn go_bench_re() -> &'static Regex {
    GO_BENCH_RE.get_or_init(|| {
        Regex::new(r"^Benchmark(\S+)\s+(\d+)\s+(\d+\.?\d*)\s*(ns|µs|ms)/op").unwrap()
    })
}
fn golint_re() -> &'static Regex {
    GOLINT_RE.get_or_init(|| Regex::new(r"^(.+?):(\d+):(\d+):\s+(.+?)\s+\((.+?)\)$").unwrap())
}
fn go_build_error_re() -> &'static Regex {
    GO_BUILD_ERROR_RE.get_or_init(|| Regex::new(r"^(.+?):(\d+):(\d+):\s+(.+)$").unwrap())
}

pub fn compress(command: &str, output: &str) -> Option<String> {
    if command.contains("golangci-lint") || command.contains("golint") {
        return Some(compress_golint(output));
    }
    if command.contains("test") {
        if command.contains("-bench") || command.contains("bench") {
            return Some(compress_bench(output));
        }
        return Some(compress_test(output));
    }
    if command.contains("build") {
        return Some(compress_build(output));
    }
    if command.contains("vet") {
        return Some(compress_vet(output));
    }
    if command.contains("mod") {
        return Some(compress_mod(output));
    }
    if command.contains("fmt") {
        return Some(compress_fmt(output));
    }
    None
}

fn compress_test(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }

    let mut results = Vec::new();
    let mut failed_tests = Vec::new();

    for line in trimmed.lines() {
        if let Some(caps) = go_test_result_re().captures(line) {
            let status = &caps[1];
            let pkg = &caps[2];
            let duration = &caps[3];
            results.push(format!("{status} {pkg} ({duration})"));
        }
        if line.contains("--- FAIL:") {
            let name = line.replace("--- FAIL:", "").trim().to_string();
            failed_tests.push(name);
        }
    }

    if results.is_empty() {
        return compact_output(trimmed, 10);
    }

    let mut parts = results;
    if !failed_tests.is_empty() {
        parts.push(format!("failed: {}", failed_tests.join(", ")));
    }
    parts.join("\n")
}

fn compress_bench(output: &str) -> String {
    let trimmed = output.trim();
    let mut benchmarks = Vec::new();

    for line in trimmed.lines() {
        if let Some(caps) = go_bench_re().captures(line) {
            let name = &caps[1];
            let ops = &caps[2];
            let ns = &caps[3];
            let unit = &caps[4];
            benchmarks.push(format!("{name}: {ops} ops @ {ns} {unit}/op"));
        }
    }

    if benchmarks.is_empty() {
        return compact_output(trimmed, 10);
    }
    format!(
        "{} benchmarks:\n{}",
        benchmarks.len(),
        benchmarks.join("\n")
    )
}

fn compress_build(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }

    let mut errors = Vec::new();
    for line in trimmed.lines() {
        if let Some(caps) = go_build_error_re().captures(line) {
            errors.push(format!("{}:{}: {}", &caps[1], &caps[2], &caps[4]));
        }
    }

    if errors.is_empty() {
        return compact_output(trimmed, 5);
    }
    format!("{} errors:\n{}", errors.len(), errors.join("\n"))
}

fn compress_golint(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "clean".to_string();
    }

    let mut by_linter: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut files: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in trimmed.lines() {
        if let Some(caps) = golint_re().captures(line) {
            files.insert(caps[1].to_string());
            let linter = caps[5].to_string();
            *by_linter.entry(linter).or_insert(0) += 1;
        }
    }

    if by_linter.is_empty() {
        return compact_output(trimmed, 10);
    }

    let total: u32 = by_linter.values().sum();
    let mut linters: Vec<(String, u32)> = by_linter.into_iter().collect();
    linters.sort_by(|a, b| b.1.cmp(&a.1));

    let mut parts = Vec::new();
    parts.push(format!("{total} issues in {} files", files.len()));
    for (linter, count) in linters.iter().take(8) {
        parts.push(format!("  {linter}: {count}"));
    }
    if linters.len() > 8 {
        parts.push(format!("  ... +{} more linters", linters.len() - 8));
    }

    parts.join("\n")
}

fn compress_vet(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok (vet clean)".to_string();
    }
    compact_output(trimmed, 10)
}

fn compress_mod(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }
    compact_output(trimmed, 10)
}

fn compress_fmt(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok (formatted)".to_string();
    }

    let files: Vec<&str> = trimmed.lines().filter(|l| !l.trim().is_empty()).collect();
    format!("{} files reformatted:\n{}", files.len(), files.join("\n"))
}

fn compact_output(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= max {
        return lines.join("\n");
    }
    format!(
        "{}\n... ({} more lines)",
        lines[..max].join("\n"),
        lines.len() - max
    )
}
