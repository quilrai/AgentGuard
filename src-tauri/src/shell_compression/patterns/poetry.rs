use regex::Regex;
use std::sync::OnceLock;

static UV_INSTALLED_LINE: OnceLock<Regex> = OnceLock::new();
static UV_RESOLVED: OnceLock<Regex> = OnceLock::new();
static POETRY_INSTALLING: OnceLock<Regex> = OnceLock::new();
static POETRY_UPDATING: OnceLock<Regex> = OnceLock::new();
static PIP_STYLE_SUCCESS: OnceLock<Regex> = OnceLock::new();
static PERCENT_BAR: OnceLock<Regex> = OnceLock::new();

fn uv_installed_line_re() -> &'static Regex {
    UV_INSTALLED_LINE.get_or_init(|| Regex::new(r"^\s*\+\s+(\S+)").unwrap())
}
fn uv_resolved_re() -> &'static Regex {
    UV_RESOLVED
        .get_or_init(|| Regex::new(r"(?i)^(Resolved|Prepared|Installed|Audited)\s+").unwrap())
}
fn poetry_installing_re() -> &'static Regex {
    POETRY_INSTALLING
        .get_or_init(|| Regex::new(r"(?i)^\s*-\s+Installing\s+(\S+)\s+\(([^)]+)\)").unwrap())
}
fn poetry_updating_re() -> &'static Regex {
    POETRY_UPDATING
        .get_or_init(|| Regex::new(r"(?i)^\s*-\s+Updating\s+(\S+)\s+\(([^)]+)\)").unwrap())
}
fn pip_style_success_re() -> &'static Regex {
    PIP_STYLE_SUCCESS.get_or_init(|| Regex::new(r"(?i)Successfully installed\s+(.+)").unwrap())
}
fn percent_bar_re() -> &'static Regex {
    PERCENT_BAR.get_or_init(|| Regex::new(r"\d+%\|").unwrap())
}

pub fn compress(command: &str, output: &str) -> Option<String> {
    let cl = command.trim().to_ascii_lowercase();
    if cl.starts_with("poetry ") {
        let sub = cl.split_whitespace().nth(1).unwrap_or("");
        return match sub {
            "install" => Some(compress_poetry(output, false)),
            "update" => Some(compress_poetry(output, true)),
            "add" => Some(compress_poetry(output, false)),
            _ => None,
        };
    }
    let uv_parts: Vec<&str> = cl.split_whitespace().collect();
    if uv_parts.len() >= 2 && uv_parts[0] == "uv" && uv_parts[1] == "sync" {
        return Some(compress_uv(output));
    }
    if uv_parts.len() >= 3
        && uv_parts[0] == "uv"
        && uv_parts[1] == "pip"
        && uv_parts[2] == "install"
    {
        return Some(compress_uv(output));
    }
    None
}

fn is_download_noise(line: &str) -> bool {
    let t = line.trim();
    let tl = t.to_ascii_lowercase();
    if tl.contains("downloading ")
        || tl.starts_with("downloading [")
        || tl.contains("kiB/s")
        || tl.contains("kib/s")
        || tl.contains("mib/s")
        || tl.contains("%") && (tl.contains("eta") || tl.contains('|') || tl.contains("of "))
    {
        return true;
    }
    if tl.starts_with("progress ") && tl.contains('/') {
        return true;
    }
    if percent_bar_re().is_match(t) {
        return true;
    }
    false
}

fn compress_poetry(output: &str, prefer_update: bool) -> String {
    let mut packages = Vec::new();
    let mut errors = Vec::new();

    for line in output.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() || is_download_noise(t) {
            continue;
        }
        let trim = t.trim();
        let tl = trim.to_ascii_lowercase();

        if prefer_update {
            if let Some(caps) = poetry_updating_re().captures(trim) {
                packages.push(format!("{} {}", &caps[1], &caps[2]));
                continue;
            }
        }
        if let Some(caps) = poetry_installing_re().captures(trim) {
            packages.push(format!("{} {}", &caps[1], &caps[2]));
            continue;
        }
        if !prefer_update {
            if let Some(caps) = poetry_updating_re().captures(trim) {
                packages.push(format!("{} {}", &caps[1], &caps[2]));
                continue;
            }
        }

        if tl.contains("error")
            && (tl.contains("because") || tl.contains("could not") || tl.contains("failed"))
        {
            errors.push(trim.to_string());
        }
        if tl.starts_with("solverproblemerror") || tl.contains("version solving failed") {
            errors.push(trim.to_string());
        }
    }

    let mut parts = Vec::new();
    if !packages.is_empty() {
        parts.push(format!("{} package(s):", packages.len()));
        parts.extend(packages.into_iter().map(|p| format!("  {p}")));
    }
    if !errors.is_empty() {
        parts.push(format!("{} error line(s):", errors.len()));
        parts.extend(errors.into_iter().take(15).map(|e| format!("  {e}")));
    }

    if parts.is_empty() {
        fallback_compact(output)
    } else {
        parts.join("\n")
    }
}

fn compress_uv(output: &str) -> String {
    let mut summary = Vec::new();
    let mut installed = Vec::new();
    let mut errors = Vec::new();

    for line in output.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() || is_download_noise(t) {
            continue;
        }
        let trim = t.trim();
        let tl = trim.to_ascii_lowercase();

        if uv_resolved_re().is_match(trim) {
            summary.push(trim.to_string());
            continue;
        }
        if let Some(caps) = uv_installed_line_re().captures(trim) {
            installed.push(caps[1].to_string());
            continue;
        }
        if let Some(caps) = pip_style_success_re().captures(trim) {
            let pkgs: Vec<&str> = caps[1].split_whitespace().collect();
            summary.push(format!("Successfully installed {} packages", pkgs.len()));
            for p in pkgs.into_iter().take(30) {
                installed.push(p.to_string());
            }
            continue;
        }

        if tl.contains("error:")
            || tl.starts_with("error:")
            || tl.contains("failed to")
            || tl.contains("resolution failed")
        {
            errors.push(trim.to_string());
        }
    }

    let mut parts = Vec::new();
    parts.extend(summary);
    if !installed.is_empty() {
        parts.push(format!("+ {} package(s):", installed.len()));
        for p in installed.into_iter().take(40) {
            parts.push(format!("  {p}"));
        }
    }
    if !errors.is_empty() {
        parts.push(format!("{} error line(s):", errors.len()));
        parts.extend(errors.into_iter().take(15).map(|e| format!("  {e}")));
    }

    if parts.is_empty() {
        fallback_compact(output)
    } else {
        parts.join("\n")
    }
}

fn fallback_compact(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !is_download_noise(l))
        .collect();
    if lines.is_empty() {
        return "ok".to_string();
    }
    let max = 12usize;
    if lines.len() <= max {
        return lines.join("\n");
    }
    format!(
        "{}\n... ({} more lines)",
        lines[..max].join("\n"),
        lines.len() - max
    )
}
