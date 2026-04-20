pub mod ansible;
pub mod aws;
pub mod bazel;
pub mod bun;
pub mod cargo;
pub mod cmake;
pub mod composer;
pub mod curl;
pub mod deno;
pub mod docker;
pub mod dotnet;
pub mod env_filter;
pub mod eslint;
pub mod find;
pub mod flutter;
pub mod gh;
pub mod git;
pub mod golang;
pub mod grep;
pub mod helm;
pub mod json_schema;
pub mod kubectl;
pub mod log_dedup;
pub mod ls;
pub mod make;
pub mod maven;
pub mod mix;
pub mod mysql;
pub mod next_build;
pub mod npm;
pub mod pip;
pub mod playwright;
pub mod pnpm;
pub mod poetry;
pub mod prettier;
pub mod prisma;
pub mod psql;
pub mod ruby;
pub mod ruff;
pub mod swift;
pub mod systemd;
pub mod terraform;
pub mod test;
pub mod typescript;
pub mod wget;
pub mod zig;

pub fn compress_output(command: &str, output: &str) -> Option<String> {
    let specific = try_specific_pattern(command, output);
    if specific.is_some() {
        return specific;
    }

    if let Some(r) = json_schema::compress(output) {
        return Some(r);
    }

    if let Some(r) = log_dedup::compress(output) {
        return Some(r);
    }

    if let Some(r) = test::compress(output) {
        return Some(r);
    }

    None
}

fn try_specific_pattern(cmd: &str, output: &str) -> Option<String> {
    let cl = cmd.to_ascii_lowercase();
    let c = cl.as_str();

    if c.starts_with("git ") {
        return git::compress(c, output);
    }
    if c.starts_with("gh ") {
        return gh::compress(c, output);
    }
    if c == "terraform" || c.starts_with("terraform ") {
        return terraform::compress(c, output);
    }
    if c == "make" || c.starts_with("make ") {
        return make::compress(c, output);
    }
    if c.starts_with("mvn ")
        || c.starts_with("./mvnw ")
        || c.starts_with("mvnw ")
        || c.starts_with("gradle ")
        || c.starts_with("./gradlew ")
        || c.starts_with("gradlew ")
    {
        return maven::compress(c, output);
    }
    if c.starts_with("kubectl ") || c.starts_with("k ") {
        return kubectl::compress(c, output);
    }
    if c.starts_with("helm ") {
        return helm::compress(c, output);
    }
    if c.starts_with("pnpm ") {
        return pnpm::compress(c, output);
    }
    if c.starts_with("bun ") {
        return bun::compress(c, output);
    }
    if c.starts_with("deno ") {
        return deno::compress(c, output);
    }
    if c.starts_with("npm ") || c.starts_with("yarn ") {
        return npm::compress(c, output);
    }
    if c.starts_with("cargo ") {
        return cargo::compress(c, output);
    }
    if c.starts_with("docker ") || c.starts_with("docker-compose ") {
        return docker::compress(c, output);
    }
    if c.starts_with("pip ") || c.starts_with("pip3 ") || c.starts_with("python -m pip") {
        return pip::compress(c, output);
    }
    if c.starts_with("ruff ") {
        return ruff::compress(c, output);
    }
    if c.starts_with("eslint")
        || c.starts_with("npx eslint")
        || c.starts_with("biome ")
        || c.starts_with("stylelint")
    {
        return eslint::compress(c, output);
    }
    if c.starts_with("prettier") || c.starts_with("npx prettier") {
        return prettier::compress(output);
    }
    if c.starts_with("go ") || c.starts_with("golangci-lint") || c.starts_with("golint") {
        return golang::compress(c, output);
    }
    if c.starts_with("playwright")
        || c.starts_with("npx playwright")
        || c.starts_with("cypress")
        || c.starts_with("npx cypress")
    {
        return playwright::compress(c, output);
    }
    if c.starts_with("vitest") || c.starts_with("npx vitest") || c.starts_with("pnpm vitest") {
        return test::compress(output);
    }
    if c.starts_with("next ")
        || c.starts_with("npx next")
        || c.starts_with("vite ")
        || c.starts_with("npx vite")
    {
        return next_build::compress(c, output);
    }
    if c.starts_with("tsc") || c.contains("typescript") {
        return typescript::compress(output);
    }
    if c.starts_with("rubocop")
        || c.starts_with("bundle ")
        || c.starts_with("rake ")
        || c.starts_with("rails test")
        || c.starts_with("rspec")
    {
        return ruby::compress(c, output);
    }
    if c.starts_with("grep ") || c.starts_with("rg ") {
        return grep::compress(output);
    }
    if c.starts_with("find ") {
        return find::compress(output);
    }
    if c.starts_with("ls ") || c == "ls" {
        return ls::compress(output);
    }
    if c.starts_with("curl ") {
        return curl::compress(output);
    }
    if c.starts_with("wget ") {
        return wget::compress(output);
    }
    if c == "env" || c.starts_with("env ") || c.starts_with("printenv") {
        return env_filter::compress(output);
    }
    if c.starts_with("dotnet ") {
        return dotnet::compress(c, output);
    }
    if c.starts_with("flutter ")
        || (c.starts_with("dart ") && (c.contains(" analyze") || c.ends_with(" analyze")))
    {
        return flutter::compress(c, output);
    }
    if c.starts_with("poetry ")
        || c.starts_with("uv sync")
        || (c.starts_with("uv ") && c.contains("pip install"))
    {
        return poetry::compress(c, output);
    }
    if c.starts_with("aws ") {
        return aws::compress(c, output);
    }
    if c.starts_with("psql ") || c.starts_with("pg_") {
        return psql::compress(c, output);
    }
    if c.starts_with("mysql ") || c.starts_with("mariadb ") {
        return mysql::compress(c, output);
    }
    if c.starts_with("prisma ") || c.starts_with("npx prisma") {
        return prisma::compress(c, output);
    }
    if c.starts_with("swift ") {
        return swift::compress(c, output);
    }
    if c.starts_with("zig ") {
        return zig::compress(c, output);
    }
    if c.starts_with("cmake ") || c.starts_with("ctest") {
        return cmake::compress(c, output);
    }
    if c.starts_with("ansible") || c.starts_with("ansible-playbook") {
        return ansible::compress(c, output);
    }
    if c.starts_with("composer ") {
        return composer::compress(c, output);
    }
    if c.starts_with("mix ") || c.starts_with("iex ") {
        return mix::compress(c, output);
    }
    if c.starts_with("bazel ") || c.starts_with("blaze ") {
        return bazel::compress(c, output);
    }
    if c.starts_with("systemctl ") || c.starts_with("journalctl") {
        return systemd::compress(c, output);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::compress_output;

    fn compress(cmd: &str, output: &str) -> String {
        compress_output(cmd, output).unwrap_or_else(|| {
            panic!("expected compression for {cmd:?}, got None\noutput was:\n{output}")
        })
    }

    fn shorter_than_original(cmd: &str, output: &str) -> String {
        let result = compress(cmd, output);
        assert!(
            result.len() < output.len(),
            "compressed output was not shorter than original\ncompressed ({} bytes):\n{result}\noriginal ({} bytes):\n{output}",
            result.len(),
            output.len()
        );
        result
    }

    // ---------- curl ----------

    #[test]
    fn curl_json_body_produces_schema() {
        let body = r#"{
          "id": "req_7a1b",
          "object": "chat.completion",
          "created": 1731452900,
          "model": "gpt-5.4-nano",
          "choices": [
            {"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}
          ],
          "usage": {"prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15}
        }"#;
        let result = shorter_than_original(
            "curl -sS https://api.example.com/v1/chat/completions",
            body,
        );
        assert!(result.starts_with("JSON "), "expected schema header, got {result}");
        assert!(result.contains("model"));
        assert!(result.contains("choices"));
        assert!(result.contains("usage"));
    }

    #[test]
    fn curl_large_json_array_truncates_inner_items() {
        let items: Vec<String> = (0..30)
            .map(|i| format!("    {{\"id\": {i}, \"name\": \"item-{i}\", \"enabled\": true}}"))
            .collect();
        let body = format!("[\n{}\n]", items.join(",\n"));
        let result = shorter_than_original("curl -sS https://api.example.com/items", &body);
        assert!(result.starts_with("JSON "));
        assert!(result.contains("; 30]"), "should describe array length: {result}");
    }

    #[test]
    fn curl_html_captures_title() {
        let html = r#"<!DOCTYPE html>
<html>
<head>
  <title>Example Domain</title>
  <meta charset="utf-8">
</head>
<body>
  <h1>Example Domain</h1>
  <p>This domain is for use in illustrative examples in documents.</p>
  <p>You may use this domain in literature without prior coordination or asking for permission.</p>
  <p>Lorem ipsum dolor sit amet, consectetur adipiscing elit.</p>
</body>
</html>"#;
        let result = shorter_than_original("curl -sS https://example.com/", html);
        assert!(result.starts_with("HTML:"));
        assert!(result.contains("Example Domain"));
    }

    #[test]
    fn curl_http_headers_summarised() {
        let headers = "HTTP/1.1 200 OK\nDate: Mon, 12 Nov 2024 10:22:00 GMT\nContent-Type: application/json; charset=utf-8\nContent-Length: 1823\nServer: cloudflare\nX-Request-Id: abc123\nCache-Control: no-cache\n";
        let result = compress("curl -sI https://api.example.com/v1/status", headers);
        assert!(result.starts_with("HTTP/1.1 200 OK"));
        assert!(result.contains("application/json"));
        assert!(result.contains("1823B"));
    }

    // ---------- grep / rg ----------

    #[test]
    fn grep_recursive_output_groups_by_file() {
        let output = "\
src/server.rs:12:use tokio::sync::mpsc;
src/server.rs:45:    let tx = tx.clone();
src/server.rs:67:    tokio::spawn(async move {
src/handlers/auth.rs:8:use tokio::time::timeout;
src/handlers/auth.rs:33:        tokio::spawn(handle_auth(req));
src/db/pool.rs:5:use tokio::sync::Mutex;
src/db/pool.rs:22:    pool: tokio::sync::Mutex<Vec<Conn>>,
src/db/pool.rs:89:        tokio::spawn(cleanup_task());
src/db/pool.rs:104:        tokio::task::spawn_blocking(move || {\n";
        let result = shorter_than_original("grep -rn tokio src/", output);
        assert!(result.contains("9 matches"), "should count matches: {result}");
        assert!(result.contains("3F"), "should count distinct files: {result}");
        assert!(result.contains("src/db/pool.rs"));
    }

    #[test]
    fn rg_ripgrep_style_output() {
        // Many matches per file — grouping header amortises over enough lines
        // that compression actually wins.
        let mut lines = Vec::new();
        for i in 1..=20 {
            lines.push(format!(
                "src/server.rs:{i}:    let tokio_handle = tokio::runtime::Handle::current();"
            ));
        }
        for i in 1..=12 {
            lines.push(format!(
                "src/handlers/auth.rs:{i}:    tokio::spawn(async move {{ /* ... */ }});"
            ));
        }
        for i in 1..=8 {
            lines.push(format!(
                "src/db/pool.rs:{i}:        use tokio::sync::Mutex as TokioMutex;"
            ));
        }
        let output = lines.join("\n") + "\n";
        let result = shorter_than_original("rg tokio", &output);
        assert!(result.contains("40 matches"));
        assert!(result.contains("3F"));
        assert!(result.contains("src/server.rs"));
    }

    #[test]
    fn grep_truncates_long_lines() {
        let long = "x".repeat(200);
        let output = format!(
            "file.txt:1:{long}\nfile.txt:2:{long}\nfile.txt:3:{long}\nfile.txt:4:{long}\n"
        );
        let result = shorter_than_original("grep xxx file.txt", &output);
        assert!(result.contains('…'), "should truncate with ellipsis: {result}");
    }

    // ---------- find / ls ----------

    #[test]
    fn find_groups_by_directory() {
        let output = "\
./src/main.rs
./src/lib.rs
./src/server.rs
./src/handlers/auth.rs
./src/handlers/users.rs
./src/db/pool.rs
./src/db/migrations/001_init.sql
./Cargo.toml
./README.md
./.git/HEAD
./node_modules/foo/package.json
./target/debug/build.log\n";
        let result = shorter_than_original("find . -type f", output);
        assert!(result.contains('F') && result.contains('D'), "summary header: {result}");
        assert!(!result.contains(".git/HEAD"), "should skip .git: {result}");
        assert!(!result.contains("node_modules"), "should skip node_modules: {result}");
        assert!(!result.contains("target/debug"), "should skip target/debug: {result}");
    }

    #[test]
    fn ls_long_listing_summarises_files_and_dirs() {
        let output = "\
total 96
drwxr-xr-x   8 user  staff    256 Nov 12 10:22 .
drwxr-xr-x   5 user  staff    160 Nov 10 14:15 ..
drwxr-xr-x  12 user  staff    384 Nov 12 10:20 src
drwxr-xr-x   3 user  staff     96 Nov 12 10:18 tests
-rw-r--r--   1 user  staff   1823 Nov 12 10:22 Cargo.toml
-rw-r--r--   1 user  staff  40192 Nov 12 10:22 Cargo.lock
-rw-r--r--   1 user  staff   2048 Nov 10 14:15 README.md
-rw-r--r--   1 user  staff    512 Nov 10 14:15 .gitignore\n";
        let result = shorter_than_original("ls -la", output);
        assert!(result.contains("src/"));
        assert!(result.contains("Cargo.lock"));
        assert!(result.contains("files, ") && result.contains("dirs"));
    }

    // ---------- docker / kubectl ----------

    #[test]
    fn docker_ps_summarises_each_container() {
        let output = "\
CONTAINER ID   IMAGE               COMMAND                  CREATED         STATUS                   PORTS                    NAMES
7a3f9b2c1d8e   postgres:16         \"docker-entrypoint.s…\"   2 hours ago     Up 2 hours               0.0.0.0:5432->5432/tcp   db
a1b2c3d4e5f6   redis:7.2           \"docker-entrypoint.s…\"   2 hours ago     Up 2 hours               0.0.0.0:6379->6379/tcp   cache
9e8d7c6b5a4f   myapp:latest        \"./entrypoint.sh\"        45 minutes ago  Restarting (1) 5s ago                             web\n";
        let result = shorter_than_original("docker ps", output);
        assert!(result.contains("db"));
        assert!(result.contains("cache"));
        assert!(result.contains("web"));
    }

    #[test]
    fn git_status_compresses_untracked_and_modified() {
        let output = "\
On branch main
Your branch is ahead of 'origin/main' by 2 commits.
  (use \"git push\" to publish your local commits)

Changes not staged for commit:
  (use \"git add <file>...\" to update what will be committed)
  (use \"git restore <file>...\" to discard changes in working directory)
\tmodified:   src/server.rs
\tmodified:   src/handlers/auth.rs

Untracked files:
  (use \"git add <file>...\" to include in what will be committed)
\tsrc/handlers/new_feature.rs
\tsrc/handlers/new_feature_test.rs

no changes added to commit (use \"git add\" and/or \"git commit -a\")\n";
        let result = shorter_than_original("git status", output);
        assert!(result.contains("main"), "should mention branch: {result}");
    }

    // ---------- pass-through ----------

    #[test]
    fn unknown_command_returns_none() {
        let result = compress_output("my-custom-tool --flag", "some arbitrary output\nwith lines\n");
        assert!(result.is_none());
    }

    // ==================================================================
    // Extended real-world coverage: curl variants, git, docker, kubectl,
    // npm, grep edge cases, etc.
    // ==================================================================

    // ---------- curl: more variants ----------

    #[test]
    fn curl_http2_status_line_recognised() {
        let headers = "HTTP/2 301\ndate: Mon, 12 Nov 2024 10:22:00 GMT\ncontent-type: text/html; charset=iso-8859-1\ncontent-length: 229\nlocation: https://example.com/\nserver: nginx\n";
        let result = compress("curl -sI https://example.com", headers);
        assert!(result.starts_with("HTTP/2 301"));
        assert!(result.contains("text/html"));
        assert!(result.contains("229B"));
    }

    #[test]
    fn curl_error_response_json_still_schematized() {
        // 4xx/5xx JSON errors should go through the JSON schema compressor.
        let body = r#"{
          "error": {
            "code": "rate_limit_exceeded",
            "message": "You have exceeded the rate limit for this endpoint. Please slow down and retry after waiting some time.",
            "type": "rate_limit_error",
            "param": null,
            "request_id": "req_01J8K9S0ABC123DEF456GHI789"
          }
        }"#;
        let result = shorter_than_original("curl -sS https://api.example.com/v1/chat", body);
        assert!(result.starts_with("JSON "));
        assert!(result.contains("error"));
    }

    #[test]
    fn curl_json_top_level_array_schematized() {
        let items: Vec<String> = (0..50)
            .map(|i| {
                format!(
                    "  {{\"sha\": \"a{i:04x}deadbeef\", \"commit\": {{\"message\": \"fix #{i}\"}}}}"
                )
            })
            .collect();
        let body = format!("[\n{}\n]", items.join(",\n"));
        let result = shorter_than_original("curl -sS https://api.github.com/repos/x/y/commits", &body);
        assert!(result.starts_with("JSON "));
        assert!(result.contains("; 50]"), "should report array length: {result}");
    }

    #[test]
    fn curl_deeply_nested_json_summarises_as_schema() {
        let body = r#"{
          "a": { "b": { "c": { "d": { "e": { "f": "deep" } } } } }
        }"#;
        let result = compress("curl -sS https://api.example.com/deep", body);
        assert!(result.starts_with("JSON "));
        // Nested single-key objects inline as `{key}` — result stays compact.
        assert!(result.contains("a:"), "should describe top-level keys: {result}");
        assert!(result.len() < body.len());
    }

    #[test]
    fn curl_plain_text_body_is_not_compressed() {
        // Plaintext response (e.g. /health endpoints) isn't JSON/HTML/headers.
        // Pattern should decline; outer compressor falls through to a
        // tail/first fallback — here we just assert the pattern returns None.
        let result = compress_output(
            "curl -sS https://example.com/health",
            "OK\nuptime=142500\nbuild=abc123\n",
        );
        assert!(result.is_none(), "plaintext should not match curl patterns: {result:?}");
    }

    #[test]
    fn curl_verbose_with_leading_asterisks_is_not_compressed() {
        // `curl -v` starts with `*   Trying ...` / `> GET ...` — none of our
        // classifiers match, so we decline rather than corrupt the trace.
        let verbose = "*   Trying 93.184.216.34:443...\n* Connected to example.com\n> GET / HTTP/1.1\n> Host: example.com\n>\n< HTTP/1.1 200 OK\n< Content-Type: text/plain\n<\nhello world\n";
        let result = compress_output("curl -v https://example.com", verbose);
        assert!(result.is_none(), "verbose trace shouldn't match: {result:?}");
    }

    #[test]
    fn curl_malformed_json_declines_gracefully() {
        // JSON-like but syntactically broken — compress_json returns None.
        // Whole pattern returns None and we fall through.
        let body = "{\n  \"ok\": true,\n  \"items\": [1, 2, 3, oops]\n}\n";
        let result = compress_output("curl -sS https://api.example.com/bad", body);
        assert!(result.is_none(), "malformed JSON shouldn't match: {result:?}");
    }

    #[test]
    fn curl_json_with_long_string_values_is_compacted() {
        let long = "x".repeat(200);
        let body = format!(
            r#"{{
              "id": "req_123",
              "description": "{long}",
              "ok": true
            }}"#
        );
        let result = shorter_than_original("curl -sS https://api.example.com/item", &body);
        assert!(
            result.contains("string(200)"),
            "long strings should be summarised by length: {result}"
        );
    }

    // ---------- grep / rg edge cases ----------

    #[test]
    fn grep_single_file_output_without_filename_declines() {
        // `grep pattern file.txt` prints just matched lines, no file: prefix.
        // Our parser requires a `file:` prefix; output should pass through.
        let output = "match one here\nmatch two here\nmatch three here\n";
        let result = compress_output("grep foo single.txt", output);
        assert!(result.is_none(), "no-prefix output should decline: {result:?}");
    }

    #[test]
    fn grep_with_context_separator_still_groups_real_matches() {
        // `grep -C 1` output contains `--` separator lines and non-match
        // context. Context lines use `-` instead of `:` so they won't parse;
        // real `:` matches should still be grouped.
        let output = "\
src/a.rs-10-// header
src/a.rs:11:    let x = 1;
src/a.rs-12-// footer
--
src/b.rs-20-// header
src/b.rs:21:    let y = 2;
src/b.rs-22-// footer
";
        let result = shorter_than_original("grep -rn -C 1 'let ' src/", output);
        assert!(result.contains("2 matches"));
        assert!(result.contains("2F"));
    }

    // ---------- git: more subcommands ----------

    #[test]
    fn git_log_oneline_passes_through() {
        let output = "\
abc1234 feat: add X
def5678 fix: handle Y
9876543 chore: bump deps
1111222 docs: update README\n";
        let result = compress("git log --oneline", output);
        assert!(result.contains("abc1234"));
        assert!(result.contains("chore: bump deps"));
    }

    #[test]
    fn git_log_full_collapses_to_hash_and_subject() {
        let output = "\
commit abc1234deadbeef1234567890abcdef12345678
Author: Alice <alice@example.com>
Date:   Mon Nov 11 10:00:00 2024 +0000

    feat: add new feature

commit def5678cafef00d1234567890abcdef12345678
Author: Bob <bob@example.com>
Date:   Mon Nov 11 11:00:00 2024 +0000

    fix: broken thing
";
        let result = shorter_than_original("git log", output);
        assert!(result.contains("abc1234"));
        assert!(result.contains("def5678"));
        assert!(result.contains("feat: add new feature"));
        assert!(!result.contains("Alice"), "authors should be dropped: {result}");
    }

    #[test]
    fn git_diff_summarises_additions_and_deletions() {
        let output = "\
diff --git a/src/a.rs b/src/a.rs
index 1111..2222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
 unchanged
-old line 1
-old line 2
+new line 1
+new line 2
+new line 3
diff --git a/src/b.rs b/src/b.rs
index 3333..4444 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -10,1 +10,1 @@
-x
+y
";
        let result = shorter_than_original("git diff", output);
        assert!(result.contains("src/a.rs"));
        assert!(result.contains("+3/-2"));
        assert!(result.contains("src/b.rs"));
        assert!(result.contains("+1/-1"));
    }

    #[test]
    fn git_branch_listing_compresses() {
        let output = "\
  develop
* main
  feature/auth
  feature/payments
  feature/reports
  hotfix/crash-on-login
  release/v1.2.0\n";
        let result = compress("git branch", output);
        // Just assert it's handled (no panic) and non-empty.
        assert!(!result.is_empty());
    }

    // ---------- docker: more commands ----------

    #[test]
    fn docker_images_compresses() {
        let output = "\
REPOSITORY          TAG       IMAGE ID       CREATED         SIZE
postgres            16        1111abcd       2 weeks ago     442MB
redis               7.2       2222bcde       3 weeks ago     117MB
myapp               latest    3333cdef       2 hours ago     89MB
<none>              <none>    4444defa       4 weeks ago     1.2GB
nginx               alpine    5555efab       1 month ago     23MB\n";
        let result = shorter_than_original("docker images", output);
        assert!(result.contains("postgres:16"));
        assert!(result.contains("redis:7.2"));
        assert!(!result.contains("<none>"), "dangling images dropped: {result}");
    }

    #[test]
    fn docker_build_success_summarised() {
        let mut lines = Vec::new();
        for i in 1..=12 {
            lines.push(format!("Step {i}/12 : RUN some-build-step-{i}"));
            lines.push(format!(" ---> Running in abcd{i:04x}"));
            lines.push(format!(" ---> def{i:04x}"));
        }
        lines.push("Successfully built def0012".to_string());
        let output = lines.join("\n");
        let result = shorter_than_original("docker build -t myapp .", &output);
        assert!(result.contains("12 steps"), "should count steps: {result}");
    }

    #[test]
    fn docker_build_with_error_surfaces_error() {
        let output = "\
Step 1/5 : FROM python:3.12
Step 2/5 : RUN apt-get update
Step 3/5 : RUN pip install -r requirements.txt
ERROR: Could not find a version that satisfies the requirement missing-pkg==9.9.9
The command '/bin/sh -c pip install -r requirements.txt' returned a non-zero code: 1
";
        let result = shorter_than_original("docker build -t myapp .", output);
        assert!(result.contains("error") || result.contains("ERROR"));
    }

    #[test]
    fn docker_logs_are_returned() {
        let output = "\
2024-11-12T10:22:00Z starting server
2024-11-12T10:22:01Z listening on :8080
2024-11-12T10:22:05Z GET / 200 12ms
2024-11-12T10:22:07Z GET /health 200 1ms
";
        let result = compress("docker logs myapp", output);
        assert!(result.contains("starting server") || result.contains("listening"));
    }

    // ---------- kubectl ----------

    #[test]
    fn kubectl_get_pods_compresses() {
        let output = "\
NAME                      READY   STATUS    RESTARTS   AGE
api-7f9d8c6b4d-abcde       2/2     Running   0          5h
api-7f9d8c6b4d-fghij       2/2     Running   0          5h
api-7f9d8c6b4d-klmno       2/2     Running   1          3h
worker-6b8c7a9e2f-pqrst    1/1     Running   0          2d
worker-6b8c7a9e2f-uvwxy    0/1     CrashLoopBackOff   7  2d
db-0                       1/1     Running   0          14d
cache-0                    1/1     Running   0          14d\n";
        let result = shorter_than_original("kubectl get pods", output);
        assert!(result.contains("api-7f9d8c6b4d-abcde"));
        assert!(result.contains("CrashLoopBackOff"));
        assert!(result.contains("Running"));
    }

    #[test]
    fn kubectl_apply_summarises_resource_counts() {
        let output = "\
deployment.apps/api configured
deployment.apps/worker unchanged
service/api unchanged
configmap/api-config created
secret/api-secrets configured
ingress.networking.k8s.io/api created
";
        let result = shorter_than_original("kubectl apply -f manifests/", output);
        assert!(result.contains("created"));
        assert!(result.contains("configured"));
        assert!(result.contains("unchanged"));
    }

    #[test]
    fn kubectl_logs_dedups_repeated_lines() {
        let mut lines = Vec::new();
        for _ in 0..20 {
            lines.push(
                "2024-11-12T10:22:00Z INFO  ready for connections"
                    .to_string(),
            );
        }
        for i in 0..5 {
            lines.push(format!(
                "2024-11-12T10:22:{i:02}Z INFO  handled request {i}"
            ));
        }
        let output = lines.join("\n");
        let result = shorter_than_original("kubectl logs api-pod", &output);
        assert!(
            result.contains("(x20)") || result.contains("20"),
            "repeated line should be deduped: {result}"
        );
    }

    // ---------- npm ----------

    #[test]
    fn npm_install_summarises() {
        let output = "\
npm warn deprecated foo@1.0.0: use foo2
npm warn deprecated bar@2.0.0: use bar2

added 843 packages, and audited 844 packages in 42s

123 packages are looking for funding
  run `npm fund` for details

found 0 vulnerabilities
";
        let result = shorter_than_original("npm install", output);
        assert!(result.contains("843"), "should keep dep count: {result}");
    }

    #[test]
    fn npm_audit_summarises_vulnerabilities() {
        let output = "\
# npm audit report

axios 0.21.0 - 0.21.3
Severity: high
SSRF in axios - https://github.com/advisories/GHSA-xxxx

lodash <4.17.21
Severity: critical
Prototype pollution in lodash - https://github.com/advisories/GHSA-yyyy

5 vulnerabilities (2 low, 1 moderate, 1 high, 1 critical)

To address all issues, run:
  npm audit fix
";
        let result = shorter_than_original("npm audit", output);
        assert!(
            result.contains("vulnerabilit"),
            "should surface vuln summary: {result}"
        );
    }

    #[test]
    fn npm_test_counts_pass_fail() {
        let output = "\
> myapp@1.0.0 test
> jest

 PASS  src/a.test.ts
 PASS  src/b.test.ts
 FAIL  src/c.test.ts
 PASS  src/d.test.ts

Test Suites: 1 failed, 3 passed, 4 total
Tests:       1 failed, 47 passed, 48 total
";
        let result = compress("npm test", output);
        assert!(result.contains("pass"));
        assert!(result.contains("fail"));
    }
}
