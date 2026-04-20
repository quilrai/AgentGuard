// Symbol extraction via tree-sitter.
//
// Parses source code with tree-sitter grammars and extracts:
//   - Imports / use statements (what a file depends on)
//   - Function, method, and class definitions (what a file exports)
//
// Supports: JavaScript, TypeScript, Python, Rust, Go, Java, C, C++, Ruby, C#.
//
// The extractor is called from PostToolUse hooks whenever we see file content
// (Read responses, Write/Edit inputs). Results are stored in the file_symbols
// DB table and surfaced in the Garden view.

use serde::Serialize;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

// ---- Public types ----

#[derive(Debug, Clone, Serialize)]
pub struct ExtractedSymbol {
    /// "import", "function", "method", "class", "struct", "interface", "trait", "enum", "type_alias"
    pub kind: String,
    /// The symbol name (e.g. "useState", "MyClass", "handle_request")
    pub name: String,
    /// For imports: the module/path being imported from (e.g. "react", "../utils")
    pub source: Option<String>,
    /// 1-based line number where the symbol appears
    pub line: u32,
}

// ---- Language detection ----

/// Returns a tree-sitter Language for the given file extension, or None if unsupported.
fn language_for_ext(ext: &str) -> Option<Language> {
    match ext {
        "js" | "jsx" | "mjs" | "cjs" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "ts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "py" | "pyi" => Some(tree_sitter_python::LANGUAGE.into()),
        "rs" => Some(tree_sitter_rust::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "hh" => Some(tree_sitter_cpp::LANGUAGE.into()),
        "rb" | "rake" | "gemspec" => Some(tree_sitter_ruby::LANGUAGE.into()),
        "cs" => Some(tree_sitter_c_sharp::LANGUAGE.into()),
        _ => None,
    }
}

/// Returns a language key string for query selection.
fn lang_key(ext: &str) -> Option<&'static str> {
    match ext {
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "py" | "pyi" => Some("python"),
        "rs" => Some("rust"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" | "h" => Some("c"),
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "hh" => Some("cpp"),
        "rb" | "rake" | "gemspec" => Some("ruby"),
        "cs" => Some("csharp"),
        _ => None,
    }
}

// ---- Tree-sitter queries per language ----
//
// Each query uses @name for the symbol name and @source for import paths.
// We use separate queries for imports vs definitions to keep things simple.

fn import_query(lang_key: &str, language: &Language) -> Option<Query> {
    let src = match lang_key {
        "javascript" | "typescript" => {
            r#"
            (import_statement
              source: (string) @source)
            (import_statement
              (import_clause
                (named_imports
                  (import_specifier
                    name: (identifier) @name))))
            (import_statement
              (import_clause
                (identifier) @name))
            (call_expression
              function: (identifier) @_fn
              arguments: (arguments (string) @source)
              (#eq? @_fn "require"))
        "#
        }
        "python" => {
            r#"
            (import_statement
              name: (dotted_name) @name)
            (import_from_statement
              module_name: (dotted_name) @source
              name: (dotted_name) @name)
            (import_from_statement
              module_name: (relative_import) @source
              name: (dotted_name) @name)
        "#
        }
        "rust" => {
            r#"
            (use_declaration
              argument: (scoped_identifier) @name)
            (use_declaration
              argument: (use_as_clause
                path: (scoped_identifier) @name))
            (use_declaration
              argument: (scoped_use_list
                path: (scoped_identifier) @source))
            (use_declaration
              argument: (identifier) @name)
        "#
        }
        "go" => {
            r#"
            (import_spec
              path: (interpreted_string_literal) @source)
            (import_spec
              name: (package_identifier) @name
              path: (interpreted_string_literal) @source)
        "#
        }
        "java" => {
            r#"
            (import_declaration
              (scoped_identifier) @name)
        "#
        }
        "c" | "cpp" => {
            r#"
            (preproc_include
              path: (string_literal) @source)
            (preproc_include
              path: (system_lib_string) @source)
        "#
        }
        "ruby" => {
            r#"
            (call
              method: (identifier) @_fn
              arguments: (argument_list (string (string_content) @source))
              (#match? @_fn "^(require|require_relative|load)$"))
        "#
        }
        "csharp" => {
            r#"
            (using_directive
              (qualified_name) @name)
        "#
        }
        _ => return None,
    };
    Query::new(language, src).ok()
}

fn definition_query(lang_key: &str, language: &Language) -> Option<Query> {
    let src = match lang_key {
        "javascript" | "typescript" => {
            r#"
            (function_declaration
              name: (identifier) @name)
            (class_declaration
              name: (identifier) @name)
            (method_definition
              name: (property_identifier) @name)
            (lexical_declaration
              (variable_declarator
                name: (identifier) @name
                value: (arrow_function)))
            (variable_declaration
              (variable_declarator
                name: (identifier) @name
                value: (arrow_function)))
            (export_statement
              declaration: (function_declaration
                name: (identifier) @name))
            (export_statement
              declaration: (class_declaration
                name: (identifier) @name))
        "#
        }
        "python" => {
            r#"
            (function_definition
              name: (identifier) @name)
            (class_definition
              name: (identifier) @name)
        "#
        }
        "rust" => {
            r#"
            (function_item
              name: (identifier) @name)
            (struct_item
              name: (type_identifier) @name)
            (enum_item
              name: (type_identifier) @name)
            (trait_item
              name: (type_identifier) @name)
            (impl_item
              trait: (type_identifier) @name)
            (type_item
              name: (type_identifier) @name)
        "#
        }
        "go" => {
            r#"
            (function_declaration
              name: (identifier) @name)
            (method_declaration
              name: (field_identifier) @name)
            (type_declaration
              (type_spec
                name: (type_identifier) @name))
        "#
        }
        "java" => {
            r#"
            (method_declaration
              name: (identifier) @name)
            (class_declaration
              name: (identifier) @name)
            (interface_declaration
              name: (identifier) @name)
            (enum_declaration
              name: (identifier) @name)
        "#
        }
        "c" => {
            r#"
            (function_definition
              declarator: (function_declarator
                declarator: (identifier) @name))
            (struct_specifier
              name: (type_identifier) @name)
            (enum_specifier
              name: (type_identifier) @name)
            (type_definition
              declarator: (type_identifier) @name)
        "#
        }
        "cpp" => {
            r#"
            (function_definition
              declarator: (function_declarator
                declarator: (identifier) @name))
            (function_definition
              declarator: (function_declarator
                declarator: (qualified_identifier) @name))
            (class_specifier
              name: (type_identifier) @name)
            (struct_specifier
              name: (type_identifier) @name)
            (enum_specifier
              name: (type_identifier) @name)
        "#
        }
        "ruby" => {
            r#"
            (method
              name: (identifier) @name)
            (singleton_method
              name: (identifier) @name)
            (class
              name: (constant) @name)
            (module
              name: (constant) @name)
        "#
        }
        "csharp" => {
            r#"
            (method_declaration
              name: (identifier) @name)
            (class_declaration
              name: (identifier) @name)
            (interface_declaration
              name: (identifier) @name)
            (struct_declaration
              name: (identifier) @name)
            (enum_declaration
              name: (identifier) @name)
        "#
        }
        _ => return None,
    };
    Query::new(language, src).ok()
}

// ---- Main extraction entry point ----

/// Extract symbols from source code. `file_path` is used to detect language
/// via extension. Returns an empty vec for unsupported languages or parse
/// failures. This is designed to be best-effort — a failed parse just means
/// no symbols, not an error.
pub fn extract_symbols(file_path: &str, source: &str) -> Vec<ExtractedSymbol> {
    // Don't parse huge files — cap at 500KB to avoid slow parses.
    if source.len() > 500_000 {
        return Vec::new();
    }

    let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

    let language = match language_for_ext(&ext) {
        Some(l) => l,
        None => return Vec::new(),
    };
    let lk = match lang_key(&ext) {
        Some(k) => k,
        None => return Vec::new(),
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let root = tree.root_node();
    let src_bytes = source.as_bytes();
    let mut symbols = Vec::new();

    // ---- Extract imports ----
    if let Some(query) = import_query(lk, &language) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, src_bytes);
        while let Some(m) = matches.next() {
            let mut name: Option<String> = None;
            let mut source_path: Option<String> = None;

            for cap in m.captures {
                let cap_name = query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(src_bytes).unwrap_or("").to_string();
                // Strip quotes from string literals.
                let text = text
                    .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                    .to_string();

                match cap_name {
                    "name" => {
                        name = Some(text);
                    }
                    "source" => {
                        source_path = Some(text);
                    }
                    _ => {}
                }
            }

            // For imports we want at least a name or a source.
            if name.is_some() || source_path.is_some() {
                let node = m.captures[0].node;
                symbols.push(ExtractedSymbol {
                    kind: "import".to_string(),
                    name: name.unwrap_or_default(),
                    source: source_path,
                    line: node.start_position().row as u32 + 1,
                });
            }
        }
    }

    // ---- Extract definitions ----
    if let Some(query) = definition_query(lk, &language) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, src_bytes);
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let cap_name = query.capture_names()[cap.index as usize];
                if cap_name != "name" {
                    continue;
                }
                let text = cap.node.utf8_text(src_bytes).unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }

                let parent = cap.node.parent();
                let kind = classify_definition(lk, parent);

                symbols.push(ExtractedSymbol {
                    kind,
                    name: text,
                    source: None,
                    line: cap.node.start_position().row as u32 + 1,
                });
            }
        }
    }

    // Deduplicate by (kind, name, line).
    symbols.sort_by(|a, b| a.line.cmp(&b.line));
    symbols.dedup_by(|a, b| a.kind == b.kind && a.name == b.name && a.line == b.line);
    symbols
}

/// Classify a definition node into a more specific kind string.
fn classify_definition(lk: &str, parent: Option<tree_sitter::Node>) -> String {
    let parent = match parent {
        Some(p) => p,
        None => return "function".to_string(),
    };
    let kind = parent.kind();

    match lk {
        "javascript" | "typescript" => match kind {
            "class_declaration" => "class",
            "method_definition" => "method",
            _ => "function",
        },
        "python" => match kind {
            "class_definition" => "class",
            _ => "function",
        },
        "rust" => match kind {
            "struct_item" => "struct",
            "enum_item" => "enum",
            "trait_item" => "trait",
            "impl_item" => "impl",
            "type_item" => "type_alias",
            _ => "function",
        },
        "go" => match kind {
            "method_declaration" => "method",
            "type_spec" => "type_alias",
            _ => "function",
        },
        "java" => match kind {
            "class_declaration" => "class",
            "interface_declaration" => "interface",
            "enum_declaration" => "enum",
            _ => "method",
        },
        "c" | "cpp" => match kind {
            "struct_specifier" | "class_specifier" => "struct",
            "enum_specifier" => "enum",
            "type_definition" => "type_alias",
            _ => "function",
        },
        "ruby" => match kind {
            "class" => "class",
            "module" => "module",
            "singleton_method" => "method",
            _ => "method",
        },
        "csharp" => match kind {
            "class_declaration" => "class",
            "interface_declaration" => "interface",
            "struct_declaration" => "struct",
            "enum_declaration" => "enum",
            _ => "method",
        },
        _ => "function",
    }
    .to_string()
}

/// Extract file paths from a bash command that have supported extensions and
/// exist on disk. Used by Codex hooks (which only have a Bash tool).
pub fn paths_from_bash_for_symbols(cmd: &str, cwd: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokenize_shell_words(cmd) {
        let t = token.trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if t.is_empty() || t.starts_with('-') {
            continue;
        }
        if !is_supported_extension(t) {
            continue;
        }
        let p = std::path::Path::new(t);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if abs.is_file() {
            out.push(abs.to_string_lossy().to_string());
        }
    }
    out
}

fn tokenize_shell_words(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in cmd.chars() {
        if c == '\'' && !in_double {
            in_single = !in_single;
            cur.push(c);
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            cur.push(c);
            continue;
        }
        if c.is_whitespace() && !in_single && !in_double {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        cur.push(c);
    }

    if !cur.is_empty() {
        out.push(cur);
    }

    out
}

/// Check if we support the file extension for symbol extraction.
pub fn is_supported_extension(file_path: &str) -> bool {
    let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();
    language_for_ext(&ext).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_javascript_extraction() {
        let src = r#"
import React from 'react';
import { useState, useEffect } from 'react';
const fs = require('fs');

function handleClick(e) {
  console.log(e);
}

class MyComponent extends React.Component {
  render() { return null; }
}

const arrow = () => {};
"#;
        let symbols = extract_symbols("app.js", src);
        let imports: Vec<_> = symbols.iter().filter(|s| s.kind == "import").collect();
        let funcs: Vec<_> = symbols.iter().filter(|s| s.kind == "function").collect();
        let classes: Vec<_> = symbols.iter().filter(|s| s.kind == "class").collect();

        assert!(!imports.is_empty(), "should find imports");
        assert!(imports.iter().any(|i| i.source.as_deref() == Some("react")));
        assert!(funcs.iter().any(|f| f.name == "handleClick"));
        assert!(classes.iter().any(|c| c.name == "MyComponent"));
    }

    #[test]
    fn test_python_extraction() {
        let src = r#"
import os
from pathlib import Path
from . import utils

def main():
    pass

class Server:
    def handle(self):
        pass
"#;
        let symbols = extract_symbols("app.py", src);
        let imports: Vec<_> = symbols.iter().filter(|s| s.kind == "import").collect();
        assert!(!imports.is_empty(), "should find imports");
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == "function"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Server" && s.kind == "class"));
    }

    #[test]
    fn test_rust_extraction() {
        let src = r#"
use std::collections::HashMap;
use crate::database::Database;

pub fn handle_request(req: Request) -> Response {
    todo!()
}

struct AppState {
    db: Database,
}

enum Status {
    Running,
    Stopped,
}

trait Handler {
    fn handle(&self);
}
"#;
        let symbols = extract_symbols("main.rs", src);
        assert!(symbols.iter().any(|s| s.kind == "import"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "handle_request" && s.kind == "function"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "AppState" && s.kind == "struct"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Status" && s.kind == "enum"));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Handler" && s.kind == "trait"));
    }

    #[test]
    fn test_unsupported_extension() {
        let symbols = extract_symbols("data.json", "{}");
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_bash_path_extraction_keeps_quoted_paths_together() {
        let dir = std::env::temp_dir().join(format!(
            "symbols_paths_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("quoted dir")).unwrap();
        let file = dir.join("quoted dir").join("demo.py");
        std::fs::write(&file, "print('hi')\n").unwrap();

        let found = paths_from_bash_for_symbols(r#"cat "quoted dir/demo.py""#, &dir);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0], file.to_string_lossy());

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(dir.join("quoted dir"));
        let _ = std::fs::remove_dir(&dir);
    }
}
