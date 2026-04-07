use regex::Regex;
use std::sync::OnceLock;

static COMPILING_RE: OnceLock<Regex> = OnceLock::new();
static ERROR_RE: OnceLock<Regex> = OnceLock::new();
static WARNING_RE: OnceLock<Regex> = OnceLock::new();
static TEST_RESULT_RE: OnceLock<Regex> = OnceLock::new();
static FINISHED_RE: OnceLock<Regex> = OnceLock::new();

fn compiling_re() -> &'static Regex {
    COMPILING_RE.get_or_init(|| Regex::new(r"Compiling (\S+) v(\S+)").unwrap())
}
fn error_re() -> &'static Regex {
    ERROR_RE.get_or_init(|| Regex::new(r"error\[E(\d+)\]: (.+)").unwrap())
}
fn warning_re() -> &'static Regex {
    WARNING_RE.get_or_init(|| Regex::new(r"warning: (.+)").unwrap())
}
fn test_result_re() -> &'static Regex {
    TEST_RESULT_RE.get_or_init(|| {
        Regex::new(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored").unwrap()
    })
}
fn finished_re() -> &'static Regex {
    FINISHED_RE.get_or_init(|| Regex::new(r"Finished .+ in (\d+\.?\d*s)").unwrap())
}

pub fn compress(command: &str, output: &str) -> Option<String> {
    if command.contains("build") || command.contains("check") {
        return Some(compress_build(output));
    }
    if command.contains("test") {
        return Some(compress_test(output));
    }
    if command.contains("clippy") {
        return Some(compress_clippy(output));
    }
    None
}

fn compress_build(output: &str) -> String {
    let mut crate_count = 0u32;
    let mut errors = Vec::new();
    let mut warnings = 0u32;
    let mut time = String::new();

    for line in output.lines() {
        if compiling_re().is_match(line) {
            crate_count += 1;
        }
        if let Some(caps) = error_re().captures(line) {
            errors.push(format!("E{}: {}", &caps[1], &caps[2]));
        }
        if warning_re().is_match(line) && !line.contains("generated") {
            warnings += 1;
        }
        if let Some(caps) = finished_re().captures(line) {
            time = caps[1].to_string();
        }
    }

    let mut parts = Vec::new();
    if crate_count > 0 {
        parts.push(format!("compiled {crate_count} crates"));
    }
    if !errors.is_empty() {
        parts.push(format!("{} errors:", errors.len()));
        for e in &errors {
            parts.push(format!("  {e}"));
        }
    }
    if warnings > 0 {
        parts.push(format!("{warnings} warnings"));
    }
    if !time.is_empty() {
        parts.push(format!("({time})"));
    }

    if parts.is_empty() {
        return "ok".to_string();
    }
    parts.join("\n")
}

fn compress_test(output: &str) -> String {
    let mut results = Vec::new();
    let mut failed_tests = Vec::new();
    let mut time = String::new();

    for line in output.lines() {
        if let Some(caps) = test_result_re().captures(line) {
            results.push(format!(
                "{}: {} pass, {} fail, {} skip",
                &caps[1], &caps[2], &caps[3], &caps[4]
            ));
        }
        if line.contains("FAILED") && line.contains("---") {
            let name = line.split_whitespace().nth(1).unwrap_or("?");
            failed_tests.push(name.to_string());
        }
        if let Some(caps) = finished_re().captures(line) {
            time = caps[1].to_string();
        }
    }

    let mut parts = Vec::new();
    if !results.is_empty() {
        parts.extend(results);
    }
    if !failed_tests.is_empty() {
        parts.push(format!("failed: {}", failed_tests.join(", ")));
    }
    if !time.is_empty() {
        parts.push(format!("({time})"));
    }

    if parts.is_empty() {
        return "ok".to_string();
    }
    parts.join("\n")
}

fn compress_clippy(output: &str) -> String {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for line in output.lines() {
        if let Some(caps) = error_re().captures(line) {
            errors.push(caps[2].to_string());
        } else if let Some(caps) = warning_re().captures(line) {
            let msg = &caps[1];
            if !msg.contains("generated") && !msg.starts_with('`') {
                warnings.push(msg.to_string());
            }
        }
    }

    let mut parts = Vec::new();
    if !errors.is_empty() {
        parts.push(format!("{} errors: {}", errors.len(), errors.join("; ")));
    }
    if !warnings.is_empty() {
        parts.push(format!("{} warnings", warnings.len()));
    }

    if parts.is_empty() {
        return "clean".to_string();
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_build_success() {
        let output = "   Compiling lean-ctx v2.1.1\n    Finished release profile [optimized] target(s) in 30.5s";
        let result = compress("cargo build", output).unwrap();
        assert!(result.contains("compiled"), "should mention compilation");
        assert!(result.contains("30.5s"), "should include build time");
    }

    #[test]
    fn cargo_build_with_errors() {
        let output = "   Compiling lean-ctx v2.1.1\nerror[E0308]: mismatched types\n --> src/main.rs:10:5\n  |\n10|     1 + \"hello\"\n  |         ^^^^^^^ expected integer, found &str";
        let result = compress("cargo build", output).unwrap();
        assert!(result.contains("E0308"), "should contain error code");
    }

    #[test]
    fn cargo_test_success() {
        let output = "running 5 tests\ntest test_one ... ok\ntest test_two ... ok\ntest test_three ... ok\ntest test_four ... ok\ntest test_five ... ok\n\ntest result: ok. 5 passed; 0 failed; 0 ignored";
        let result = compress("cargo test", output).unwrap();
        assert!(result.contains("5 pass"), "should show passed count");
    }

    #[test]
    fn cargo_test_failure() {
        let output = "running 3 tests\ntest test_ok ... ok\ntest test_fail ... FAILED\ntest test_ok2 ... ok\n\ntest result: FAILED. 2 passed; 1 failed; 0 ignored";
        let result = compress("cargo test", output).unwrap();
        assert!(result.contains("FAIL"), "should indicate failure");
    }

    #[test]
    fn cargo_clippy_clean() {
        let output = "    Checking lean-ctx v2.1.1\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.2s";
        let result = compress("cargo clippy", output).unwrap();
        assert!(result.contains("clean"), "clean clippy should say clean");
    }

    #[test]
    fn cargo_check_routes_to_build() {
        let output = "    Checking lean-ctx v2.1.1\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.1s";
        let result = compress("cargo check", output);
        assert!(
            result.is_some(),
            "cargo check should route to build compressor"
        );
    }
}
