use regex::Regex;
use std::sync::OnceLock;

static TSC_ERROR_RE: OnceLock<Regex> = OnceLock::new();
static ERROR_COUNT_RE: OnceLock<Regex> = OnceLock::new();

fn tsc_error_re() -> &'static Regex {
    TSC_ERROR_RE.get_or_init(|| Regex::new(r"(\S+)\((\d+),\d+\): error (TS\d+): (.+)").unwrap())
}
fn error_count_re() -> &'static Regex {
    ERROR_COUNT_RE.get_or_init(|| Regex::new(r"Found (\d+) error").unwrap())
}

fn shorten_path(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).to_string()
}

pub fn compress(output: &str) -> Option<String> {
    let mut errors = Vec::new();
    let mut file_count = std::collections::HashSet::new();
    let mut total_errors = 0u32;

    for line in output.lines() {
        if let Some(caps) = tsc_error_re().captures(line) {
            let file = shorten_path(&caps[1]);
            let line_no = &caps[2];
            let code = &caps[3];
            let msg = caps[4].trim();
            let short_msg = if msg.len() > 40 {
                let truncated: String = msg.chars().take(40).collect();
                format!("{truncated}...")
            } else {
                msg.to_string()
            };
            errors.push(format!("{file}:{line_no} {code} {short_msg}"));
            file_count.insert(caps[1].to_string());
        }
        if let Some(caps) = error_count_re().captures(line) {
            total_errors = caps[1].parse().unwrap_or(0);
        }
    }

    if errors.is_empty() {
        return None;
    }

    let header = format!("{total_errors} errors in {} files:", file_count.len());
    let mut result = vec![header];
    result.extend(errors);
    Some(result.join("\n"))
}
