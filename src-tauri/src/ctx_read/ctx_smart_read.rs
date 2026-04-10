use super::cache::{compute_md5, SessionCache};
use super::ReadResult;
use crate::shell_compression::tokens::count_tokens;

/// Auto-select the best read mode and delegate to ctx_read.
pub fn handle(cache: &mut SessionCache, path: &str) -> ReadResult {
    let mode = select_mode(cache, path);
    let mut result = super::ctx_read::handle(cache, path, &mode, false);
    result.output = format!("[auto:{mode}] {}", result.output);
    // Recount sent tokens since we prepended the prefix
    result.sent_tokens = count_tokens(&result.output);
    result
}

pub fn select_mode(cache: &SessionCache, path: &str) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return "full".to_string(),
    };

    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Cached and unchanged → stub via full
    if let Some(cached) = cache.get(path) {
        if cached.hash == compute_md5(&content) {
            return "full".to_string();
        }
        return "diff".to_string();
    }

    let token_count = count_tokens(&content);

    if token_count <= 200 {
        return "full".to_string();
    }

    if is_config_or_data(ext, path) {
        return "full".to_string();
    }

    // Without signatures/map/aggressive modes, heuristic always returns full.
    // The real savings come from cache hit stubs and auto-delta on re-reads.
    "full".to_string()
}

fn is_config_or_data(ext: &str, path: &str) -> bool {
    if matches!(
        ext,
        "json" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" | "env" | "lock"
    ) {
        return true;
    }
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "tsconfig.json"
            | "Makefile"
            | "Dockerfile"
            | "docker-compose.yml"
            | ".gitignore"
            | ".env"
            | "pyproject.toml"
            | "go.mod"
            | "build.gradle"
            | "pom.xml"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_detection() {
        assert!(is_config_or_data("json", "package.json"));
        assert!(is_config_or_data("toml", "Cargo.toml"));
        assert!(!is_config_or_data("rs", "main.rs"));
    }
}
