use regex::Regex;
use std::sync::OnceLock;

static PIP_INSTALLED_RE: OnceLock<Regex> = OnceLock::new();
static PIP_OUTDATED_RE: OnceLock<Regex> = OnceLock::new();

fn pip_installed_re() -> &'static Regex {
    PIP_INSTALLED_RE.get_or_init(|| Regex::new(r"Successfully installed\s+(.+)").unwrap())
}
fn pip_outdated_re() -> &'static Regex {
    PIP_OUTDATED_RE.get_or_init(|| Regex::new(r"^(\S+)\s+(\S+)\s+(\S+)\s+(\S+)").unwrap())
}

pub fn compress(command: &str, output: &str) -> Option<String> {
    if command.contains("install") {
        return Some(compress_install(output));
    }
    if command.contains("list") || command.contains("freeze") {
        if command.contains("outdated") || command.contains("--outdated") {
            return Some(compress_outdated(output));
        }
        return Some(compress_list(output));
    }
    if command.contains("uninstall") {
        return Some(compress_uninstall(output));
    }
    if command.contains("show") {
        return Some(compress_show(output));
    }
    if command.contains("check") {
        return Some(compress_check(output));
    }
    None
}

fn compress_install(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }

    if let Some(caps) = pip_installed_re().captures(trimmed) {
        let packages: Vec<&str> = caps[1].split_whitespace().collect();
        return format!("ok (+{} packages): {}", packages.len(), packages.join(", "));
    }

    if trimmed.contains("already satisfied") {
        return "ok (already satisfied)".to_string();
    }

    compact_output(trimmed, 5)
}

fn compress_list(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 2 {
        return output.to_string();
    }

    let skip = if lines[0].starts_with("Package") || lines[0].starts_with("---") {
        2
    } else {
        0
    };
    let packages: Vec<String> = lines[skip..]
        .iter()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() >= 2 {
                Some(format!("{}=={}", parts[0], parts[1]))
            } else {
                None
            }
        })
        .collect();

    if packages.is_empty() {
        return output.to_string();
    }
    format!("{} packages:\n{}", packages.len(), packages.join("\n"))
}

fn compress_outdated(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let skip = if lines
        .first()
        .map(|l| l.starts_with("Package"))
        .unwrap_or(false)
    {
        2
    } else {
        0
    };

    let mut outdated = Vec::new();
    for line in lines.iter().skip(skip) {
        if let Some(caps) = pip_outdated_re().captures(line) {
            let name = &caps[1];
            let current = &caps[2];
            let latest = &caps[3];
            outdated.push(format!("{name}: {current} → {latest}"));
        }
    }

    if outdated.is_empty() {
        return "all up-to-date".to_string();
    }
    format!("{} outdated:\n{}", outdated.len(), outdated.join("\n"))
}

fn compress_uninstall(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }

    let removed: Vec<&str> = trimmed
        .lines()
        .filter(|l| l.contains("Successfully uninstalled"))
        .collect();

    if removed.is_empty() {
        return compact_output(trimmed, 3);
    }
    format!("ok (removed {} packages)", removed.len())
}

fn compress_show(output: &str) -> String {
    compact_output(output, 10)
}

fn compress_check(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed.contains("No broken requirements") {
        return "ok (no broken dependencies)".to_string();
    }

    let broken: Vec<&str> = trimmed
        .lines()
        .filter(|l| l.contains("requires") || l.contains("has requirement"))
        .collect();

    if broken.is_empty() {
        return compact_output(trimmed, 5);
    }
    format!(
        "{} broken dependencies:\n{}",
        broken.len(),
        broken.join("\n")
    )
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
