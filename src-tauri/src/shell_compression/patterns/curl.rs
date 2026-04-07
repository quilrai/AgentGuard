pub fn compress(output: &str) -> Option<String> {
    let trimmed = output.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return compress_json(trimmed);
    }

    if trimmed.starts_with("<!") || trimmed.starts_with("<html") {
        return compress_html(trimmed);
    }

    if trimmed.starts_with("HTTP/") {
        return compress_headers(trimmed);
    }

    None
}

fn compress_json(output: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(output).ok()?;
    let schema = extract_schema(&val, 0);
    let size = output.len();

    Some(format!("JSON ({} bytes):\n{schema}", size))
}

fn extract_schema(val: &serde_json::Value, depth: usize) -> String {
    if depth > 3 {
        return "  ".repeat(depth) + "...";
    }

    let indent = "  ".repeat(depth);

    match val {
        serde_json::Value::Object(map) => {
            let mut lines = Vec::new();
            for (key, value) in map.iter().take(15) {
                let type_str = match value {
                    serde_json::Value::Null => "null".to_string(),
                    serde_json::Value::Bool(_) => "bool".to_string(),
                    serde_json::Value::Number(_) => "number".to_string(),
                    serde_json::Value::String(s) => {
                        if s.len() > 50 {
                            format!("string({})", s.len())
                        } else {
                            format!("\"{}\"", s)
                        }
                    }
                    serde_json::Value::Array(arr) => {
                        if arr.is_empty() {
                            "[]".to_string()
                        } else {
                            let inner = value_type(&arr[0]);
                            format!("[{inner}; {}]", arr.len())
                        }
                    }
                    serde_json::Value::Object(inner) => {
                        if inner.len() <= 3 {
                            let keys: Vec<&String> = inner.keys().collect();
                            format!(
                                "{{{}}}",
                                keys.iter()
                                    .map(|k| k.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        } else {
                            format!("{{{}K}}", inner.len())
                        }
                    }
                };
                lines.push(format!("{indent}  {key}: {type_str}"));
            }
            if map.len() > 15 {
                lines.push(format!("{indent}  ... +{} more keys", map.len() - 15));
            }
            format!("{indent}{{\n{}\n{indent}}}", lines.join("\n"))
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                format!("{indent}[]")
            } else {
                let inner = value_type(&arr[0]);
                format!("{indent}[{inner}; {}]", arr.len())
            }
        }
        _ => format!("{indent}{}", value_type(val)),
    }
}

fn value_type(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(_) => "bool".to_string(),
        serde_json::Value::Number(_) => "number".to_string(),
        serde_json::Value::String(_) => "string".to_string(),
        serde_json::Value::Array(_) => "array".to_string(),
        serde_json::Value::Object(m) => format!("object({}K)", m.len()),
    }
}

fn compress_html(output: &str) -> Option<String> {
    let lines = output.lines().count();
    let size = output.len();

    let title = output
        .find("<title>")
        .and_then(|start| {
            let after = &output[start + 7..];
            after.find("</title>").map(|end| &after[..end])
        })
        .unwrap_or("(no title)");

    Some(format!("HTML: \"{title}\" ({size} bytes, {lines} lines)"))
}

fn compress_headers(output: &str) -> Option<String> {
    let mut status = String::new();
    let mut content_type = String::new();
    let mut content_length = String::new();

    for line in output.lines().take(20) {
        if line.starts_with("HTTP/") {
            status = line.to_string();
        } else if line.to_lowercase().starts_with("content-type:") {
            content_type = line.split(':').nth(1).unwrap_or("").trim().to_string();
        } else if line.to_lowercase().starts_with("content-length:") {
            content_length = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }

    if status.is_empty() {
        return None;
    }

    let mut result = status;
    if !content_type.is_empty() {
        result.push_str(&format!(" | {content_type}"));
    }
    if !content_length.is_empty() {
        result.push_str(&format!(" | {content_length}B"));
    }

    Some(result)
}
