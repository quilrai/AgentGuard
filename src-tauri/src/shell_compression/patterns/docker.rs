use regex::Regex;
use std::sync::OnceLock;

static LOG_TIMESTAMP_RE: OnceLock<Regex> = OnceLock::new();

fn log_timestamp_re() -> &'static Regex {
    LOG_TIMESTAMP_RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}").unwrap())
}

pub fn compress(command: &str, output: &str) -> Option<String> {
    if command.contains("build") {
        return Some(compress_build(output));
    }
    if command.contains("compose") && command.contains("ps") {
        return Some(compress_compose_ps(output));
    }
    if command.contains("compose")
        && (command.contains("up")
            || command.contains("down")
            || command.contains("start")
            || command.contains("stop"))
    {
        return Some(compress_compose_action(output));
    }
    if command.contains("ps") {
        return Some(compress_ps(output));
    }
    if command.contains("images") {
        return Some(compress_images(output));
    }
    if command.contains("logs") {
        return Some(compress_logs(output));
    }
    if command.contains("network") {
        return Some(compress_network(output));
    }
    if command.contains("volume") {
        return Some(compress_volume(output));
    }
    if command.contains("inspect") {
        return Some(compress_inspect(output));
    }
    if command.contains("exec") || command.contains("run") {
        return Some(compress_exec(output));
    }
    None
}

fn compress_build(output: &str) -> String {
    let mut steps = 0u32;
    let mut last_step = String::new();
    let mut errors = Vec::new();

    for line in output.lines() {
        if line.starts_with("Step ") || (line.starts_with('#') && line.contains('[')) {
            steps += 1;
            last_step = line.trim().to_string();
        }
        if line.contains("ERROR") || line.contains("error:") {
            errors.push(line.trim().to_string());
        }
    }

    if !errors.is_empty() {
        return format!(
            "{steps} steps, {} errors:\n{}",
            errors.len(),
            errors.join("\n")
        );
    }

    if steps > 0 {
        format!("{steps} steps, last: {last_step}")
    } else {
        "built".to_string()
    }
}

fn compress_ps(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 1 {
        return "no containers".to_string();
    }

    let mut containers = Vec::new();
    for line in &lines[1..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 7 {
            let name = parts.last().unwrap_or(&"?");
            let status = parts.get(4).unwrap_or(&"?");
            containers.push(format!("{name}: {status}"));
        }
    }

    if containers.is_empty() {
        return "no containers".to_string();
    }
    containers.join("\n")
}

fn compress_images(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 1 {
        return "no images".to_string();
    }

    let mut images = Vec::new();
    for line in &lines[1..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            let repo = parts[0];
            let tag = parts[1];
            let size = parts.last().unwrap_or(&"?");
            if repo == "<none>" {
                continue;
            }
            images.push(format!("{repo}:{tag} ({size})"));
        }
    }

    if images.is_empty() {
        return "no images".to_string();
    }
    format!("{} images:\n{}", images.len(), images.join("\n"))
}

fn compress_logs(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 10 {
        return output.to_string();
    }

    let mut deduped: Vec<(String, u32)> = Vec::new();
    for line in &lines {
        let normalized = log_timestamp_re().replace(line, "[T]").to_string();
        let stripped = normalized.trim().to_string();
        if stripped.is_empty() {
            continue;
        }

        if let Some(last) = deduped.last_mut() {
            if last.0 == stripped {
                last.1 += 1;
                continue;
            }
        }
        deduped.push((stripped, 1));
    }

    let result: Vec<String> = deduped
        .iter()
        .map(|(line, count)| {
            if *count > 1 {
                format!("{line} (x{count})")
            } else {
                line.clone()
            }
        })
        .collect();

    if result.len() > 30 {
        let last_lines = &result[result.len() - 15..];
        format!(
            "... ({} lines total)\n{}",
            lines.len(),
            last_lines.join("\n")
        )
    } else {
        result.join("\n")
    }
}

fn compress_compose_ps(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 1 {
        return "no services".to_string();
    }

    let mut services = Vec::new();
    for line in &lines[1..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let name = parts[0];
            let status_parts: Vec<&str> = parts[1..].to_vec();
            let status = status_parts.join(" ");
            services.push(format!("{name}: {status}"));
        }
    }

    if services.is_empty() {
        return "no services".to_string();
    }
    format!("{} services:\n{}", services.len(), services.join("\n"))
}

fn compress_compose_action(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }

    let mut created = 0u32;
    let mut started = 0u32;
    let mut stopped = 0u32;
    let mut removed = 0u32;

    for line in trimmed.lines() {
        let l = line.to_lowercase();
        if l.contains("created") || l.contains("creating") {
            created += 1;
        }
        if l.contains("started") || l.contains("starting") {
            started += 1;
        }
        if l.contains("stopped") || l.contains("stopping") {
            stopped += 1;
        }
        if l.contains("removed") || l.contains("removing") {
            removed += 1;
        }
    }

    let mut parts = Vec::new();
    if created > 0 {
        parts.push(format!("{created} created"));
    }
    if started > 0 {
        parts.push(format!("{started} started"));
    }
    if stopped > 0 {
        parts.push(format!("{stopped} stopped"));
    }
    if removed > 0 {
        parts.push(format!("{removed} removed"));
    }

    if parts.is_empty() {
        return "ok".to_string();
    }
    format!("ok ({})", parts.join(", "))
}

fn compress_network(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 1 {
        return output.trim().to_string();
    }

    let mut networks = Vec::new();
    for line in &lines[1..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let name = parts[1];
            let driver = parts[2];
            networks.push(format!("{name} ({driver})"));
        }
    }

    if networks.is_empty() {
        return "no networks".to_string();
    }
    networks.join(", ")
}

fn compress_volume(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 1 {
        return output.trim().to_string();
    }

    let volumes: Vec<&str> = lines[1..]
        .iter()
        .filter_map(|l| l.split_whitespace().nth(1))
        .collect();

    if volumes.is_empty() {
        return "no volumes".to_string();
    }
    format!("{} volumes: {}", volumes.len(), volumes.join(", "))
}

fn compress_inspect(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return compress_json_value(&val, 0);
        }
    }
    if trimmed.lines().count() > 20 {
        let lines: Vec<&str> = trimmed.lines().collect();
        return format!(
            "{}\n... ({} more lines)",
            lines[..10].join("\n"),
            lines.len() - 10
        );
    }
    trimmed.to_string()
}

fn compress_exec(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "ok".to_string();
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() > 30 {
        let last = &lines[lines.len() - 10..];
        return format!("... ({} lines)\n{}", lines.len(), last.join("\n"));
    }
    trimmed.to_string()
}

fn compress_json_value(val: &serde_json::Value, depth: usize) -> String {
    if depth > 2 {
        return "...".to_string();
    }
    match val {
        serde_json::Value::Object(map) => {
            let keys: Vec<String> = map.keys().take(15).map(|k| k.to_string()).collect();
            let total = map.len();
            if total > 15 {
                format!("{{{} ... +{} keys}}", keys.join(", "), total - 15)
            } else {
                format!("{{{}}}", keys.join(", "))
            }
        }
        serde_json::Value::Array(arr) => format!("[...{}]", arr.len()),
        other => format!("{other}"),
    }
}
