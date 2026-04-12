// Dependency Protection Module
//
// Intercepts package install commands and dependency file writes, then:
//   1. Checks the OSV API (https://api.osv.dev/v1/query) for known vulnerabilities
//   2. Checks package registries for newer versions
//
// Two independent settings control behaviour:
//   - block_malicious_packages  → hard deny if OSV returns vulnerabilities
//   - inform_updated_packages   → allow with advisory reason if a newer version exists

use serde_json::Value;
use std::path::{Path, PathBuf};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecosystem {
    PyPI,
    Npm,
    CratesIo,
    Maven,
    Go,
    RubyGems,
    NuGet,
    Packagist,
}

impl Ecosystem {
    /// Name used in the OSV API `package.ecosystem` field.
    pub fn osv_name(&self) -> &'static str {
        match self {
            Self::PyPI => "PyPI",
            Self::Npm => "npm",
            Self::CratesIo => "crates.io",
            Self::Maven => "Maven",
            Self::Go => "Go",
            Self::RubyGems => "RubyGems",
            Self::NuGet => "NuGet",
            Self::Packagist => "Packagist",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedPackage {
    pub name: String,
    pub version: Option<String>,
    pub ecosystem: Ecosystem,
    pub version_is_exact: bool,
}

#[derive(Debug, Clone)]
pub struct VulnInfo {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Default)]
pub struct DepCheckResult {
    pub should_block: bool,
    pub block_reason: Option<String>,
    pub info_message: Option<String>,
}

fn extracted_package(
    name: String,
    version: Option<String>,
    ecosystem: Ecosystem,
    version_is_exact: bool,
) -> ExtractedPackage {
    ExtractedPackage {
        name,
        version,
        ecosystem,
        version_is_exact,
    }
}

fn looks_like_exact_version(spec: &str) -> bool {
    let trimmed = spec.trim();
    !trimmed.is_empty()
        && trimmed.chars().any(|c| c.is_ascii_digit())
        && !trimmed.eq_ignore_ascii_case("latest")
        && !trimmed.contains([
            '^', '~', '>', '<', '*', 'x', 'X', '|', ',', '[', ']', '(', ')',
        ])
}

// ============================================================================
// Package extraction — bash commands
// ============================================================================

/// Extract package names (and optional versions) from a shell command string.
/// Handles compound commands separated by `&&`, `||`, `;`.
pub fn extract_packages_from_command(command: &str) -> Vec<ExtractedPackage> {
    let mut results = Vec::new();

    // Split compound commands
    for segment in split_compound_command(command) {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        extract_from_tokens(&tokens, &mut results);
    }
    results
}

/// Extract package specs from a command, optionally resolving dependency files in
/// the current working directory for manifest-driven installs like `npm install`
/// or `pip install -r requirements.txt`.
pub fn extract_packages_from_command_with_context(
    command: &str,
    cwd: Option<&str>,
) -> Vec<ExtractedPackage> {
    let mut results = extract_packages_from_command(command);

    for segment in split_compound_command(command) {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        let mut explicit = Vec::new();
        extract_from_tokens(&tokens, &mut explicit);
        let explicit_added = !explicit.is_empty();
        extract_from_install_context(&tokens, cwd, explicit_added, &mut results);
    }

    results
}

/// Split a command on `&&`, `||`, `;`, and `|` boundaries.
fn split_compound_command(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = cmd.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        let c = chars[i];
        if c == ';' || c == '|' || c == '&' {
            // Check for && or ||
            if i + 1 < len
                && ((c == '&' && chars[i + 1] == '&') || (c == '|' && chars[i + 1] == '|'))
            {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                i += 2;
                continue;
            }
            // Single ; or |
            if c == ';' || c == '|' {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                i += 1;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn extract_from_install_context(
    tokens: &[&str],
    cwd: Option<&str>,
    explicit_added: bool,
    out: &mut Vec<ExtractedPackage>,
) {
    if tokens.is_empty() {
        return;
    }

    let joined = tokens.join(" ");
    if matches_pip_install(&joined) {
        extract_pip_requirements_files(tokens, cwd, out);
        return;
    }

    if is_manifest_js_install(tokens) && !explicit_added {
        extend_from_dependency_file_in_cwd(cwd, "package.json", out);
        return;
    }

    if tokens.len() >= 2 && tokens[0] == "composer" && tokens[1] == "install" {
        extend_from_dependency_file_in_cwd(cwd, "composer.json", out);
    }
}

fn extract_pip_requirements_files(
    tokens: &[&str],
    cwd: Option<&str>,
    out: &mut Vec<ExtractedPackage>,
) {
    let Some(start) = find_pip_pkg_start(tokens) else {
        return;
    };

    let mut i = start;
    while i < tokens.len() {
        let t = tokens[i];
        if matches!(t, "-r" | "--requirement") && i + 1 < tokens.len() {
            extend_from_dependency_file(cwd, tokens[i + 1], out);
            i += 2;
            continue;
        }
        i += 1;
    }
}

fn is_manifest_js_install(tokens: &[&str]) -> bool {
    if tokens.is_empty() {
        return false;
    }

    match tokens[0] {
        "npm" => matches!(tokens.get(1).copied(), Some("install" | "i" | "ci")),
        "yarn" => matches!(tokens.get(1).copied(), Some("install")),
        "pnpm" => matches!(tokens.get(1).copied(), Some("install" | "i")),
        "bun" => matches!(tokens.get(1).copied(), Some("install" | "i")),
        _ => false,
    }
}

fn extend_from_dependency_file_in_cwd(
    cwd: Option<&str>,
    file_name: &str,
    out: &mut Vec<ExtractedPackage>,
) {
    extend_from_dependency_file(cwd, file_name, out);
}

fn extend_from_dependency_file(
    cwd: Option<&str>,
    file_name: &str,
    out: &mut Vec<ExtractedPackage>,
) {
    let Some(path) = resolve_dependency_path(cwd, file_name) else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let path_str = path.to_string_lossy();
    out.extend(extract_packages_from_file(&path_str, &content));
}

fn resolve_dependency_path(cwd: Option<&str>, file_name: &str) -> Option<PathBuf> {
    let path = Path::new(file_name);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    let cwd = cwd?;
    Some(Path::new(cwd).join(file_name))
}

fn extract_from_tokens(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    let joined = tokens.join(" ");

    // Python: pip install, pip3 install, python -m pip install
    if matches_pip_install(&joined) {
        let pkg_start = find_pip_pkg_start(tokens);
        if let Some(start) = pkg_start {
            extract_pip_packages(&tokens[start..], out);
        }
        return;
    }

    // JS: npm install/i/add, yarn add, pnpm add/install
    if matches_npm_install(&joined) {
        let pkg_start = find_npm_pkg_start(tokens);
        if let Some(start) = pkg_start {
            extract_npm_packages(&tokens[start..], out);
        }
        return;
    }

    // Rust: cargo add
    if tokens.len() >= 2 && tokens[0] == "cargo" && tokens[1] == "add" {
        extract_cargo_packages(&tokens[2..], out);
        return;
    }

    // Go: go get
    if tokens.len() >= 2 && tokens[0] == "go" && tokens[1] == "get" {
        extract_go_packages(&tokens[2..], out);
        return;
    }

    // Ruby: gem install
    if tokens.len() >= 2 && tokens[0] == "gem" && tokens[1] == "install" {
        extract_gem_packages(&tokens[2..], out);
        return;
    }

    // C#: dotnet add package
    if tokens.len() >= 3 && tokens[0] == "dotnet" && tokens[1] == "add" && tokens[2] == "package" {
        extract_dotnet_packages(&tokens[3..], out);
        return;
    }

    // PHP: composer require
    if tokens.len() >= 2 && tokens[0] == "composer" && tokens[1] == "require" {
        extract_composer_packages(&tokens[2..], out);
        return;
    }

    // Maven: mvn dependency:get -Dartifact=group:artifact:version
    if tokens.len() >= 2 && tokens[0] == "mvn" {
        extract_maven_packages(tokens, out);
    }
}

fn matches_pip_install(cmd: &str) -> bool {
    let c = cmd.trim_start();
    c.starts_with("pip install")
        || c.starts_with("pip3 install")
        || c.starts_with("python -m pip install")
        || c.starts_with("python3 -m pip install")
        || c.starts_with("uv pip install")
        || c.starts_with("uv add")
}

fn find_pip_pkg_start(tokens: &[&str]) -> Option<usize> {
    for (i, t) in tokens.iter().enumerate() {
        if *t == "install" || *t == "add" {
            return Some(i + 1);
        }
    }
    None
}

fn extract_pip_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        // Skip flags (but not package specs that start with -)
        if t.starts_with('-') && !t.contains("==") {
            // Flags that take an argument: -r, -c, -e, -i, --index-url, etc.
            if matches!(
                t,
                "-r" | "-c"
                    | "-e"
                    | "-i"
                    | "--index-url"
                    | "--extra-index-url"
                    | "--find-links"
                    | "-f"
                    | "--constraint"
                    | "--requirement"
            ) {
                i += 2; // skip flag + its argument
            } else {
                i += 1;
            }
            continue;
        }
        // Parse package spec: name==version, name>=version, name~=version, or just name
        let (name, version, version_is_exact) = parse_pip_spec(t);
        if !name.is_empty() && !name.starts_with('#') {
            out.push(extracted_package(
                name,
                version,
                Ecosystem::PyPI,
                version_is_exact,
            ));
        }
        i += 1;
    }
}

fn parse_pip_spec(spec: &str) -> (String, Option<String>, bool) {
    let spec = spec.trim_matches(|c| c == '"' || c == '\'');
    // Handle extras: package[extra]==version
    let spec = if let Some(bracket) = spec.find('[') {
        if let Some(close) = spec.find(']') {
            format!("{}{}", &spec[..bracket], &spec[close + 1..])
        } else {
            spec.to_string()
        }
    } else {
        spec.to_string()
    };

    for sep in &["==", ">=", "<=", "~=", "!=", ">", "<"] {
        if let Some(pos) = spec.find(sep) {
            let name = spec[..pos].trim().to_lowercase();
            let version = spec[pos + sep.len()..].trim().to_string();
            // For ranges like >=1.0,<2.0 take the first version
            let version = version.split(',').next().unwrap_or("").trim().to_string();
            if !version.is_empty() {
                return (name, Some(version), *sep == "==");
            }
            return (name, None, false);
        }
    }
    (spec.trim().to_lowercase(), None, false)
}

fn matches_npm_install(cmd: &str) -> bool {
    let c = cmd.trim_start();
    c.starts_with("npm install")
        || c.starts_with("npm i ")
        || c.starts_with("npm add")
        || c.starts_with("yarn add")
        || c.starts_with("pnpm add")
        || c.starts_with("pnpm install")
        || c.starts_with("bun add")
        || c.starts_with("bun install")
}

fn find_npm_pkg_start(tokens: &[&str]) -> Option<usize> {
    for (i, t) in tokens.iter().enumerate() {
        if matches!(*t, "install" | "i" | "add") && i > 0 {
            return Some(i + 1);
        }
    }
    None
}

fn extract_npm_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    for t in tokens {
        if t.starts_with('-') {
            continue;
        }
        let (name, version, version_is_exact) = parse_npm_spec(t);
        if !name.is_empty() {
            out.push(extracted_package(
                name,
                version,
                Ecosystem::Npm,
                version_is_exact,
            ));
        }
    }
}

fn parse_npm_spec(spec: &str) -> (String, Option<String>, bool) {
    let spec = spec.trim_matches(|c| c == '"' || c == '\'');
    // Scoped packages: @scope/name@version
    if spec.starts_with('@') {
        if let Some(at_pos) = spec[1..].find('@') {
            let at_pos = at_pos + 1;
            let name = spec[..at_pos].to_string();
            let version = spec[at_pos + 1..].to_string();
            let version = if version.is_empty() {
                None
            } else {
                Some(version)
            };
            let version_is_exact = version
                .as_deref()
                .map(looks_like_exact_version)
                .unwrap_or(false);
            return (name, version, version_is_exact);
        }
        return (spec.to_string(), None, false);
    }
    // Regular: name@version
    if let Some(at_pos) = spec.find('@') {
        let name = spec[..at_pos].to_string();
        let version = spec[at_pos + 1..].to_string();
        let version = if version.is_empty() {
            None
        } else {
            Some(version)
        };
        let version_is_exact = version
            .as_deref()
            .map(looks_like_exact_version)
            .unwrap_or(false);
        (name, version, version_is_exact)
    } else {
        (spec.to_string(), None, false)
    }
}

fn extract_cargo_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if t.starts_with('-') {
            // Flags that take args: --features, --git, --path, --branch, --tag, --rev
            if matches!(
                t,
                "--features" | "--git" | "--path" | "--branch" | "--tag" | "--rev" | "-F"
            ) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        // cargo add serde@1.0
        let (name, version, version_is_exact) = if let Some(at_pos) = t.find('@') {
            let raw_version = t[at_pos + 1..].trim();
            let version_is_exact = raw_version.starts_with('=');
            let normalized = raw_version.trim_start_matches('=').to_string();
            (
                t[..at_pos].to_string(),
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                },
                version_is_exact,
            )
        } else {
            (t.to_string(), None, false)
        };
        if !name.is_empty() {
            out.push(extracted_package(
                name,
                version,
                Ecosystem::CratesIo,
                version_is_exact,
            ));
        }
        i += 1;
    }
}

fn extract_go_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    for t in tokens {
        if t.starts_with('-') {
            continue;
        }
        let (name, version, version_is_exact) = if let Some(at_pos) = t.find('@') {
            (
                t[..at_pos].to_string(),
                Some(t[at_pos + 1..].to_string()),
                true,
            )
        } else {
            (t.to_string(), None, false)
        };
        if !name.is_empty() {
            out.push(extracted_package(
                name,
                version,
                Ecosystem::Go,
                version_is_exact,
            ));
        }
    }
}

fn extract_gem_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    let mut i = 0;
    let mut last_version_exact = false;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "-v" || t == "--version" {
            // Version applies to the previous package
            if i + 1 < tokens.len() {
                if let Some(last) = out.last_mut() {
                    let raw = tokens[i + 1].trim_matches('\'').trim_matches('"');
                    last.version = Some(raw.to_string());
                    last.version_is_exact = looks_like_exact_version(raw);
                    last_version_exact = last.version_is_exact;
                }
                i += 2;
                continue;
            }
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        out.push(extracted_package(
            t.to_string(),
            None,
            Ecosystem::RubyGems,
            last_version_exact,
        ));
        last_version_exact = false;
        i += 1;
    }
}

fn extract_dotnet_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut version_is_exact = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "-v" || t == "--version" {
            if i + 1 < tokens.len() {
                version = Some(tokens[i + 1].to_string());
                version_is_exact = looks_like_exact_version(tokens[i + 1]);
                i += 2;
                continue;
            }
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if name.is_none() {
            name = Some(t.to_string());
        }
        i += 1;
    }
    if let Some(n) = name {
        out.push(extracted_package(
            n,
            version,
            Ecosystem::NuGet,
            version_is_exact,
        ));
    }
}

fn extract_composer_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    for t in tokens {
        if t.starts_with('-') {
            continue;
        }
        // composer require vendor/package:version or vendor/package
        let (name, version, version_is_exact) = if let Some(colon) = t.find(':') {
            let raw_version = t[colon + 1..].trim();
            (
                t[..colon].to_string(),
                Some(raw_version.to_string()),
                looks_like_exact_version(raw_version),
            )
        } else {
            (t.to_string(), None, false)
        };
        if !name.is_empty() && name.contains('/') {
            out.push(extracted_package(
                name,
                version,
                Ecosystem::Packagist,
                version_is_exact,
            ));
        }
    }
}

fn extract_maven_packages(tokens: &[&str], out: &mut Vec<ExtractedPackage>) {
    for t in tokens {
        if let Some(artifact) = t.strip_prefix("-Dartifact=") {
            // groupId:artifactId:version or groupId:artifactId:packaging:version
            let parts: Vec<&str> = artifact.split(':').collect();
            if parts.len() >= 3 {
                let name = format!("{}:{}", parts[0], parts[1]);
                let version = if parts.len() == 3 {
                    Some(parts[2].to_string())
                } else if parts.len() >= 4 {
                    Some(parts[parts.len() - 1].to_string())
                } else {
                    None
                };
                out.push(extracted_package(
                    name,
                    version.clone(),
                    Ecosystem::Maven,
                    version
                        .as_deref()
                        .map(looks_like_exact_version)
                        .unwrap_or(false),
                ));
            }
        }
    }
}

// ============================================================================
// Package extraction — dependency files
// ============================================================================

/// Check if a file path points to a known dependency file.
pub fn is_dependency_file(file_path: &str) -> bool {
    let basename = file_path.rsplit('/').next().unwrap_or(file_path);
    let lower = basename.to_lowercase();

    lower == "requirements.txt"
        || lower.starts_with("requirements") && lower.ends_with(".txt")
        || lower == "constraints.txt"
        || lower == "pyproject.toml"
        || lower == "package.json"
        || lower == "cargo.toml"
        || lower == "go.mod"
        || lower == "gemfile"
        || lower == "pom.xml"
        || lower == "build.gradle"
        || lower == "build.gradle.kts"
        || lower.ends_with(".csproj")
        || lower == "composer.json"
}

/// Extract package names and versions from a dependency file's content.
pub fn extract_packages_from_file(file_path: &str, content: &str) -> Vec<ExtractedPackage> {
    let basename = file_path.rsplit('/').next().unwrap_or(file_path);
    let lower = basename.to_lowercase();

    if lower.starts_with("requirements") && lower.ends_with(".txt") || lower == "constraints.txt" {
        return extract_from_requirements_txt(content);
    }
    if lower == "pyproject.toml" {
        return extract_from_pyproject_toml(content);
    }
    if lower == "package.json" {
        return extract_from_package_json(content);
    }
    if lower == "cargo.toml" {
        return extract_from_cargo_toml(content);
    }
    if lower == "go.mod" {
        return extract_from_go_mod(content);
    }
    if lower == "gemfile" {
        return extract_from_gemfile(content);
    }
    if lower == "pom.xml" {
        return extract_from_pom_xml(content);
    }
    if lower == "build.gradle" || lower == "build.gradle.kts" {
        return extract_from_gradle(content);
    }
    if lower.ends_with(".csproj") {
        return extract_from_csproj(content);
    }
    if lower == "composer.json" {
        return extract_from_composer_json(content);
    }
    Vec::new()
}

fn extract_from_requirements_txt(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        let (name, version, version_is_exact) = parse_pip_spec(line);
        if !name.is_empty() {
            out.push(extracted_package(
                name,
                version,
                Ecosystem::PyPI,
                version_is_exact,
            ));
        }
    }
    out
}

fn extract_from_pyproject_toml(content: &str) -> Vec<ExtractedPackage> {
    // Simple regex-based extraction for dependencies = ["pkg>=version", ...]
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("dependencies") && trimmed.contains('[') {
            in_deps = true;
        }
        if in_deps {
            // Extract quoted strings
            for cap in extract_quoted_strings(trimmed) {
                let (name, version, version_is_exact) = parse_pip_spec(&cap);
                if !name.is_empty() {
                    out.push(extracted_package(
                        name,
                        version,
                        Ecosystem::PyPI,
                        version_is_exact,
                    ));
                }
            }
            if trimmed.contains(']') && !trimmed.starts_with("dependencies") || (trimmed == "]") {
                in_deps = false;
            }
        }
    }
    out
}

fn extract_quoted_strings(s: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut in_quote = false;
    let mut quote_char = '"';
    let mut current = String::new();
    for c in s.chars() {
        if !in_quote {
            if c == '"' || c == '\'' {
                in_quote = true;
                quote_char = c;
                current.clear();
            }
        } else if c == quote_char {
            in_quote = false;
            if !current.is_empty() {
                results.push(current.clone());
            }
        } else {
            current.push(c);
        }
    }
    results
}

fn extract_from_package_json(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    let parsed: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return out,
    };
    for section in &[
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(deps) = parsed.get(section).and_then(|v| v.as_object()) {
            for (name, ver) in deps {
                let raw_version = ver.as_str();
                let version = raw_version.map(|s| {
                    // Strip range prefixes: ^, ~, >=, etc.
                    s.trim_start_matches('^')
                        .trim_start_matches('~')
                        .trim_start_matches(">=")
                        .trim_start_matches("<=")
                        .trim_start_matches('>')
                        .trim_start_matches('<')
                        .trim_start_matches('=')
                        .trim()
                        .to_string()
                });
                let version = version.filter(|v| {
                    !v.is_empty() && v.chars().next().map_or(false, |c| c.is_ascii_digit())
                });
                let version_is_exact = raw_version.map(looks_like_exact_version).unwrap_or(false);
                out.push(extracted_package(
                    name.clone(),
                    version,
                    Ecosystem::Npm,
                    version_is_exact,
                ));
            }
        }
    }
    out
}

fn extract_from_cargo_toml(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        // Section headers
        if trimmed.starts_with('[') {
            in_deps = trimmed == "[dependencies]"
                || trimmed == "[dev-dependencies]"
                || trimmed == "[build-dependencies]"
                || trimmed.starts_with("[dependencies.")
                || trimmed.starts_with("[dev-dependencies.")
                || trimmed.starts_with("[build-dependencies.");
            continue;
        }
        if !in_deps || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // name = "version" or name = { version = "..." }
        if let Some(eq_pos) = trimmed.find('=') {
            let name = trimmed[..eq_pos].trim().to_string();
            let rest = trimmed[eq_pos + 1..].trim();
            let (version, version_is_exact) = if rest.starts_with('"') {
                // name = "version"
                let raw = rest.trim_matches('"').to_string();
                let version_is_exact = raw.starts_with('=');
                (raw.trim_start_matches('=').to_string(), version_is_exact)
            } else if rest.starts_with('{') {
                // name = { version = "..." }
                extract_toml_inline_version(rest).unwrap_or_default()
            } else {
                (String::new(), false)
            };
            if !name.is_empty() && !name.contains(' ') {
                out.push(extracted_package(
                    name,
                    if version.is_empty() {
                        None
                    } else {
                        Some(version)
                    },
                    Ecosystem::CratesIo,
                    version_is_exact,
                ));
            }
        }
    }
    out
}

fn extract_toml_inline_version(inline: &str) -> Option<(String, bool)> {
    // Parse: { version = "1.0", ... }
    if let Some(ver_pos) = inline.find("version") {
        let rest = &inline[ver_pos + 7..];
        if let Some(eq) = rest.find('=') {
            let after_eq = rest[eq + 1..].trim();
            if after_eq.starts_with('"') {
                let end = after_eq[1..].find('"')?;
                let raw = after_eq[1..1 + end].to_string();
                let version_is_exact = raw.starts_with('=');
                return Some((raw.trim_start_matches('=').to_string(), version_is_exact));
            }
        }
    }
    None
}

fn extract_from_go_mod(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    let mut in_require = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("require (") || trimmed == "require (" {
            in_require = true;
            continue;
        }
        if trimmed == ")" {
            in_require = false;
            continue;
        }
        // Single-line require
        if trimmed.starts_with("require ") && !trimmed.contains('(') {
            let parts: Vec<&str> = trimmed[8..].split_whitespace().collect();
            if !parts.is_empty() {
                out.push(ExtractedPackage {
                    name: parts[0].to_string(),
                    version: parts.get(1).map(|v| v.to_string()),
                    ecosystem: Ecosystem::Go,
                    version_is_exact: parts.get(1).is_some(),
                });
            }
            continue;
        }
        if in_require {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 && !parts[0].starts_with("//") {
                out.push(extracted_package(
                    parts[0].to_string(),
                    Some(parts[1].to_string()),
                    Ecosystem::Go,
                    true,
                ));
            }
        }
    }
    out
}

fn extract_from_gemfile(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("gem ") {
            continue;
        }
        // gem 'name', '~> version'
        let strings = extract_quoted_strings(trimmed);
        if strings.is_empty() {
            continue;
        }
        let name = strings[0].clone();
        let raw_version = strings.get(1).cloned();
        let version = raw_version.as_deref().and_then(|v| {
            let cleaned = v
                .trim_start_matches("~>")
                .trim_start_matches(">=")
                .trim_start_matches("<=")
                .trim_start_matches('>')
                .trim_start_matches('<')
                .trim_start_matches('=')
                .trim();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_string())
            }
        });
        let version_is_exact = raw_version
            .as_deref()
            .map(looks_like_exact_version)
            .unwrap_or(false);
        out.push(extracted_package(
            name,
            version,
            Ecosystem::RubyGems,
            version_is_exact,
        ));
    }
    out
}

fn extract_from_pom_xml(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    // Regex-based: find <dependency> blocks with groupId, artifactId, version
    let mut in_dep = false;
    let mut group_id = String::new();
    let mut artifact_id = String::new();
    let mut version = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("<dependency>") {
            in_dep = true;
            group_id.clear();
            artifact_id.clear();
            version.clear();
            continue;
        }
        if trimmed.contains("</dependency>") {
            if in_dep && !artifact_id.is_empty() {
                let name = if group_id.is_empty() {
                    artifact_id.clone()
                } else {
                    format!("{}:{}", group_id, artifact_id)
                };
                let version = if version.is_empty() {
                    None
                } else {
                    Some(version.clone())
                };
                let version_is_exact = version
                    .as_deref()
                    .map(looks_like_exact_version)
                    .unwrap_or(false);
                out.push(extracted_package(
                    name,
                    version,
                    Ecosystem::Maven,
                    version_is_exact,
                ));
            }
            in_dep = false;
            continue;
        }
        if !in_dep {
            continue;
        }
        if let Some(val) = extract_xml_tag_value(trimmed, "groupId") {
            group_id = val;
        } else if let Some(val) = extract_xml_tag_value(trimmed, "artifactId") {
            artifact_id = val;
        } else if let Some(val) = extract_xml_tag_value(trimmed, "version") {
            version = val;
        }
    }
    out
}

fn extract_xml_tag_value(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = line.find(&open) {
        if let Some(end) = line.find(&close) {
            let val = &line[start + open.len()..end];
            return Some(val.trim().to_string());
        }
    }
    None
}

fn extract_from_gradle(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    // Match: implementation 'group:artifact:version', api "group:artifact:version", etc.
    for line in content.lines() {
        let trimmed = line.trim();
        for keyword in &[
            "implementation",
            "api",
            "compileOnly",
            "runtimeOnly",
            "testImplementation",
            "testRuntimeOnly",
        ] {
            if trimmed.starts_with(keyword) {
                let rest = trimmed[keyword.len()..].trim();
                // Extract quoted string
                let strings = extract_quoted_strings(rest);
                for s in strings {
                    let parts: Vec<&str> = s.split(':').collect();
                    if parts.len() >= 3 {
                        let name = format!("{}:{}", parts[0], parts[1]);
                        let version = parts[2].to_string();
                        let version_is_exact = looks_like_exact_version(&version);
                        out.push(extracted_package(
                            name,
                            if version.is_empty() {
                                None
                            } else {
                                Some(version)
                            },
                            Ecosystem::Maven,
                            version_is_exact,
                        ));
                    }
                }
            }
        }
    }
    out
}

fn extract_from_csproj(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    // <PackageReference Include="Name" Version="1.0" />
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("PackageReference") {
            continue;
        }
        let name = extract_xml_attr(trimmed, "Include");
        let version = extract_xml_attr(trimmed, "Version");
        if let Some(n) = name {
            let version_is_exact = version
                .as_deref()
                .map(looks_like_exact_version)
                .unwrap_or(false);
            out.push(extracted_package(
                n,
                version,
                Ecosystem::NuGet,
                version_is_exact,
            ));
        }
    }
    out
}

fn extract_xml_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    if let Some(start) = line.find(&pattern) {
        let rest = &line[start + pattern.len()..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn extract_from_composer_json(content: &str) -> Vec<ExtractedPackage> {
    let mut out = Vec::new();
    let parsed: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return out,
    };
    for section in &["require", "require-dev"] {
        if let Some(deps) = parsed.get(section).and_then(|v| v.as_object()) {
            for (name, ver) in deps {
                if name == "php" || name.starts_with("ext-") {
                    continue;
                }
                let raw_version = ver.as_str();
                let version = raw_version.map(|s| {
                    s.trim_start_matches('^')
                        .trim_start_matches('~')
                        .trim_start_matches(">=")
                        .trim()
                        .to_string()
                });
                let version = version.filter(|v| {
                    !v.is_empty() && v.chars().next().map_or(false, |c| c.is_ascii_digit())
                });
                let version_is_exact = raw_version.map(looks_like_exact_version).unwrap_or(false);
                out.push(extracted_package(
                    name.clone(),
                    version,
                    Ecosystem::Packagist,
                    version_is_exact,
                ));
            }
        }
    }
    out
}

// ============================================================================
// OSV vulnerability check
// ============================================================================

/// Query the OSV API for known vulnerabilities.
/// Returns `None` on network/parse errors (fail-open).
pub async fn check_osv(client: &reqwest::Client, pkg: &ExtractedPackage) -> Option<Vec<VulnInfo>> {
    let mut body = serde_json::json!({
        "package": {
            "name": pkg.name,
            "ecosystem": pkg.ecosystem.osv_name()
        }
    });
    if let Some(ref v) = pkg.version {
        body["version"] = serde_json::json!(v);
    }

    let resp = client
        .post("https://api.osv.dev/v1/query")
        .json(&body)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;

    let json: Value = resp.json().await.ok()?;
    let vulns = json.get("vulns")?.as_array()?;
    if vulns.is_empty() {
        return Some(Vec::new());
    }

    let mut results = Vec::new();
    for v in vulns {
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string();
        let summary = v
            .get("summary")
            .and_then(|x| x.as_str())
            .unwrap_or("No summary available")
            .to_string();
        results.push(VulnInfo { id, summary });
    }
    Some(results)
}

// ============================================================================
// Latest version check
// ============================================================================

/// Check the package registry for the latest available version.
/// Returns `None` on errors or if the ecosystem is unsupported (fail-open).
pub async fn check_latest_version(
    client: &reqwest::Client,
    pkg: &ExtractedPackage,
) -> Option<String> {
    // Update advice only makes sense for exact pins.
    if pkg.version.is_none() || !pkg.version_is_exact {
        return None;
    }

    lookup_latest_version(client, pkg).await
}

async fn lookup_latest_version(client: &reqwest::Client, pkg: &ExtractedPackage) -> Option<String> {
    match pkg.ecosystem {
        Ecosystem::PyPI => check_pypi(client, &pkg.name).await,
        Ecosystem::Npm => check_npm(client, &pkg.name).await,
        Ecosystem::CratesIo => check_crates_io(client, &pkg.name).await,
        Ecosystem::RubyGems => check_rubygems(client, &pkg.name).await,
        Ecosystem::NuGet => check_nuget(client, &pkg.name).await,
        Ecosystem::Go => check_go_proxy(client, &pkg.name).await,
        Ecosystem::Maven | Ecosystem::Packagist => None, // skip for v1
    }
}

struct OsvCheckResult {
    effective_version: Option<String>,
    vulns: Vec<VulnInfo>,
}

async fn check_osv_effective(
    client: &reqwest::Client,
    pkg: &ExtractedPackage,
) -> Option<OsvCheckResult> {
    let effective_version = if pkg.version_is_exact {
        pkg.version.clone()
    } else if pkg.version.is_none() {
        lookup_latest_version(client, pkg).await
    } else {
        return None;
    };

    let query_pkg = extracted_package(
        pkg.name.clone(),
        effective_version.clone(),
        pkg.ecosystem,
        true,
    );
    let vulns = check_osv(client, &query_pkg).await?;
    Some(OsvCheckResult {
        effective_version,
        vulns,
    })
}

async fn check_pypi(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{}/json", name);
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("info")?
        .get("version")?
        .as_str()
        .map(|s| s.to_string())
}

async fn check_npm(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://registry.npmjs.org/{}/latest", name);
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("version")?.as_str().map(|s| s.to_string())
}

async fn check_crates_io(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{}", name);
    let resp = client
        .get(&url)
        .header("User-Agent", "LLMwatcher/1.0 (dep-protection)")
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("crate")?
        .get("max_version")?
        .as_str()
        .map(|s| s.to_string())
}

async fn check_rubygems(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://rubygems.org/api/v1/gems/{}.json", name);
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("version")?.as_str().map(|s| s.to_string())
}

async fn check_nuget(client: &reqwest::Client, name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let url = format!(
        "https://api.nuget.org/v3-flatcontainer/{}/index.json",
        lower
    );
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    let versions = json.get("versions")?.as_array()?;
    versions.last()?.as_str().map(|s| s.to_string())
}

async fn check_go_proxy(client: &reqwest::Client, module: &str) -> Option<String> {
    let url = format!("https://proxy.golang.org/{}/@latest", module);
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("Version")?.as_str().map(|s| s.to_string())
}

// ============================================================================
// Main entry point
// ============================================================================

/// Run dependency protection checks on a set of extracted packages.
///
/// Returns a `DepCheckResult` indicating whether to block, and any
/// informational messages to relay to the agent.
pub async fn check_dependencies(
    client: &reqwest::Client,
    packages: &[ExtractedPackage],
    block_malicious: bool,
    inform_updates: bool,
) -> DepCheckResult {
    if packages.is_empty() {
        return DepCheckResult::default();
    }

    // Run OSV + version checks concurrently
    let osv_futs: Vec<_> = if block_malicious {
        packages
            .iter()
            .map(|p| check_osv_effective(client, p))
            .collect()
    } else {
        Vec::new()
    };

    let version_futs: Vec<_> = if inform_updates {
        packages
            .iter()
            .map(|p| check_latest_version(client, p))
            .collect()
    } else {
        Vec::new()
    };

    let (osv_results, version_results) = tokio::join!(
        futures::future::join_all(osv_futs),
        futures::future::join_all(version_futs),
    );

    let mut result = DepCheckResult::default();

    // Process OSV results
    if block_malicious {
        let mut block_reasons = Vec::new();
        for (i, osv_result) in osv_results.iter().enumerate() {
            if let Some(osv_result) = osv_result {
                if !osv_result.vulns.is_empty() {
                    let pkg = &packages[i];
                    let vuln_summary: Vec<String> = osv_result
                        .vulns
                        .iter()
                        .take(3) // show at most 3 vulns per package
                        .map(|v| format!("{}: {}", v.id, v.summary))
                        .collect();
                    let version_str = osv_result
                        .effective_version
                        .as_deref()
                        .unwrap_or("unresolved");
                    block_reasons.push(format!(
                        "  {} {} ({}) — {} known vulnerabilit{}:\n    {}",
                        pkg.name,
                        version_str,
                        pkg.ecosystem.osv_name(),
                        osv_result.vulns.len(),
                        if osv_result.vulns.len() == 1 {
                            "y"
                        } else {
                            "ies"
                        },
                        vuln_summary.join("\n    ")
                    ));
                }
            }
        }
        if !block_reasons.is_empty() {
            result.should_block = true;
            result.block_reason = Some(format!(
                "BLOCKED — Vulnerable dependencies detected:\n{}",
                block_reasons.join("\n")
            ));
        }
    }

    // Process version results (only if not already blocking)
    if inform_updates && !result.should_block {
        let mut updates = Vec::new();
        for (i, latest) in version_results.iter().enumerate() {
            if let Some(latest_ver) = latest {
                let pkg = &packages[i];
                if let Some(ref current_ver) = pkg.version {
                    if current_ver != latest_ver {
                        updates.push(format!(
                            "  {} {} → {} available ({})",
                            pkg.name,
                            current_ver,
                            latest_ver,
                            pkg.ecosystem.osv_name()
                        ));
                    }
                }
            }
        }
        if !updates.is_empty() {
            result.info_message = Some(format!(
                "Newer package versions available — ask the user if they want to update:\n{}",
                updates.join("\n")
            ));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_hooks::create_claude_hooks_router;
    use crate::codex_hooks::create_codex_hooks_router;
    use crate::cursor_hooks::create_cursor_hooks_router;
    use crate::database::Database;
    use crate::predefined_backend_settings::{CustomBackendSettings, DependencyProtectionSettings};
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}", name, nonce))
    }

    #[test]
    fn pip_ranges_are_not_treated_as_exact_versions() {
        let pkgs =
            extract_packages_from_file("requirements.txt", "requests>=2.31.0\nflask==3.0.2\n");

        assert_eq!(pkgs.len(), 2);
        assert!(!pkgs[0].version_is_exact);
        assert_eq!(pkgs[0].version.as_deref(), Some("2.31.0"));
        assert!(pkgs[1].version_is_exact);
        assert_eq!(pkgs[1].version.as_deref(), Some("3.0.2"));
    }

    #[test]
    fn package_json_ranges_are_not_treated_as_exact_versions() {
        let pkgs = extract_packages_from_file(
            "package.json",
            r#"{"dependencies":{"react":"^19.0.0","zod":"3.23.8"}}"#,
        );

        assert_eq!(pkgs.len(), 2);
        assert!(!pkgs[0].version_is_exact);
        assert!(pkgs[1].version_is_exact);
    }

    #[test]
    fn cargo_plain_versions_are_not_treated_as_exact_pins() {
        let pkgs = extract_packages_from_file(
            "Cargo.toml",
            r#"[dependencies]
serde = "1.0"
toml = { version = "=0.8.19" }
"#,
        );

        assert_eq!(pkgs.len(), 2);
        assert!(!pkgs[0].version_is_exact);
        assert_eq!(pkgs[0].version.as_deref(), Some("1.0"));
        assert!(pkgs[1].version_is_exact);
        assert_eq!(pkgs[1].version.as_deref(), Some("0.8.19"));
    }

    #[test]
    fn unpinned_npm_installs_have_no_exact_version() {
        let pkgs = extract_packages_from_command("npm install lodash @types/node");

        assert_eq!(pkgs.len(), 2);
        assert!(pkgs.iter().all(|pkg| pkg.version.is_none()));
        assert!(pkgs.iter().all(|pkg| !pkg.version_is_exact));
    }

    #[test]
    fn manifest_npm_install_reads_package_json_from_cwd() {
        let dir = temp_dir("dep_protection_npm_manifest");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"lodash":"4.17.21"}}"#,
        )
        .unwrap();

        let pkgs = extract_packages_from_command_with_context("npm install", dir.to_str());

        let _ = fs::remove_file(dir.join("package.json"));
        let _ = fs::remove_dir(&dir);

        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "lodash");
        assert_eq!(pkgs[0].version.as_deref(), Some("4.17.21"));
        assert!(pkgs[0].version_is_exact);
    }

    #[test]
    fn pip_requirement_install_reads_file_from_cwd() {
        let dir = temp_dir("dep_protection_pip_requirements");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("requirements.txt"),
            "urllib3==1.26.18\nrequests>=2.31.0\n",
        )
        .unwrap();

        let pkgs = extract_packages_from_command_with_context(
            "pip install -r requirements.txt",
            dir.to_str(),
        );

        let _ = fs::remove_file(dir.join("requirements.txt"));
        let _ = fs::remove_dir(&dir);

        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "urllib3");
        assert!(pkgs[0].version_is_exact);
        assert_eq!(pkgs[1].name, "requests");
        assert!(!pkgs[1].version_is_exact);
    }

    async fn spawn_router(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (format!("http://{}", addr), handle)
    }

    fn dep_block_settings() -> CustomBackendSettings {
        CustomBackendSettings {
            dlp_enabled: false,
            dependency_protection: DependencyProtectionSettings {
                inform_updated_packages: false,
                block_malicious_packages: true,
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    #[ignore = "manual live network e2e against OSV/registries"]
    async fn manual_live_e2e_manifest_install_blocks_vulnerable_package() {
        let workspace = temp_dir("dep_protection_live_workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            workspace.join("package.json"),
            r#"{"dependencies":{"lodash":"4.17.21"}}"#,
        )
        .unwrap();

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let req_client = reqwest::Client::new();

        // Claude
        let claude_db = temp_dir("dep_protection_live_claude.db");
        let claude_router = create_claude_hooks_router(
            Database::new(claude_db.to_str().unwrap()).unwrap(),
            dep_block_settings(),
            Some(Arc::new(Mutex::new(
                crate::ctx_read::cache::SessionCache::new(),
            ))),
            http_client.clone(),
        );
        let (claude_base, claude_task) = spawn_router(claude_router).await;
        let claude_resp = req_client
            .post(format!("{}/pre_bash", claude_base))
            .json(&serde_json::json!({
                "session_id": "s-claude",
                "cwd": workspace,
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": "npm install" },
                "tool_use_id": "tool-1"
            }))
            .send()
            .await
            .unwrap();
        let claude_json: Value = claude_resp.json().await.unwrap();
        claude_task.abort();
        assert_eq!(
            claude_json["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert!(
            claude_json["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap_or_default()
                .contains("lodash")
        );

        // Codex
        let codex_db = temp_dir("dep_protection_live_codex.db");
        let codex_router = create_codex_hooks_router(
            Database::new(codex_db.to_str().unwrap()).unwrap(),
            dep_block_settings(),
            http_client.clone(),
        );
        let (codex_base, codex_task) = spawn_router(codex_router).await;
        let codex_resp = req_client
            .post(format!("{}/pre_bash", codex_base))
            .json(&serde_json::json!({
                "session_id": "s-codex",
                "cwd": workspace,
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": "npm install" },
                "tool_use_id": "tool-2",
                "model": "gpt-5"
            }))
            .send()
            .await
            .unwrap();
        let codex_json: Value = codex_resp.json().await.unwrap();
        codex_task.abort();
        assert_eq!(
            codex_json["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert!(codex_json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap_or_default()
            .contains("lodash"));

        // Cursor
        let cursor_db = temp_dir("dep_protection_live_cursor.db");
        let cursor_router = create_cursor_hooks_router(
            Database::new(cursor_db.to_str().unwrap()).unwrap(),
            dep_block_settings(),
            http_client,
        );
        let (cursor_base, cursor_task) = spawn_router(cursor_router).await;
        let cursor_resp = req_client
            .post(format!("{}/before_shell_execution", cursor_base))
            .json(&serde_json::json!({
                "conversation_id": "conv-1",
                "generation_id": "gen-1",
                "model": "gpt-5",
                "hook_event_name": "beforeShellExecution",
                "cursor_version": "1.0.0",
                "workspace_roots": [workspace],
                "command": "npm install",
                "cwd": workspace,
                "sandbox": false
            }))
            .send()
            .await
            .unwrap();
        let cursor_json: Value = cursor_resp.json().await.unwrap();
        cursor_task.abort();
        assert_eq!(cursor_json["permission"], "deny");

        let _ = fs::remove_file(workspace.join("package.json"));
        let _ = fs::remove_dir(&workspace);
        let _ = fs::remove_file(&claude_db);
        let _ = fs::remove_file(&codex_db);
        let _ = fs::remove_file(&cursor_db);
    }
}
