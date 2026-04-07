pub fn compress(output: &str) -> Option<String> {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() < 5 {
        return None;
    }

    let is_long = lines.iter().any(|l| {
        l.starts_with('-') || l.starts_with('d') || l.starts_with('l') || l.starts_with("total ")
    });

    if is_long {
        compress_long(output)
    } else {
        compress_short(output)
    }
}

fn compress_long(output: &str) -> Option<String> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for line in output.lines() {
        if line.starts_with("total ") || line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }

        let name = parts[8..].join(" ");

        if name == "." || name == ".." {
            continue;
        }

        if line.starts_with('d') {
            dirs.push(format!("{name}/"));
        } else {
            let size = format_size(parts[4]);
            files.push(format!("{name}  {size}"));
        }
    }

    if dirs.is_empty() && files.is_empty() {
        return None;
    }

    let mut result = String::new();
    for d in &dirs {
        result.push_str(d);
        result.push('\n');
    }
    for f in &files {
        result.push_str(f);
        result.push('\n');
    }

    result.push_str(&format!("\n{} files, {} dirs", files.len(), dirs.len()));

    Some(result)
}

fn compress_short(output: &str) -> Option<String> {
    let items: Vec<&str> = output
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .collect();

    if items.len() < 10 {
        return None;
    }

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for item in &items {
        if item.ends_with('/') {
            dirs.push(*item);
        } else {
            files.push(*item);
        }
    }

    let mut result = String::new();
    for d in &dirs {
        result.push_str(d);
        result.push('\n');
    }

    let mut line_buf = String::new();
    for f in &files {
        if line_buf.len() + f.len() + 2 > 70 {
            result.push_str(&line_buf);
            result.push('\n');
            line_buf.clear();
        }
        if !line_buf.is_empty() {
            line_buf.push_str("  ");
        }
        line_buf.push_str(f);
    }
    if !line_buf.is_empty() {
        result.push_str(&line_buf);
        result.push('\n');
    }

    result.push_str(&format!("\n{} items", dirs.len() + files.len()));

    Some(result)
}

fn format_size(size_str: &str) -> String {
    let bytes: u64 = size_str.parse().unwrap_or(0);
    if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}
