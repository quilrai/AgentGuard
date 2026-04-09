// Database operations and schema management

use crate::builtin_patterns::get_builtin_patterns;
use crate::dlp::DlpDetection;
use crate::dlp_pattern_config::{get_db_path, DEFAULT_PORT};
use crate::requestresponsemetadata::{RequestMetadata, ResponseMetadata};
use rusqlite::Connection;
use std::sync::{Arc, Mutex, Once};

const DB_VERSION: &str = "2026-april-09";

static VERSION_CHECK: Once = Once::new();

/// Ensure the DB version matches. If missing or mismatched, delete all DB files
/// so we start clean.
fn ensure_db_version(path: &str) {
    let db_path = std::path::Path::new(path);
    if !db_path.exists() {
        return;
    }

    let version_ok = Connection::open(path)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT value FROM settings WHERE key = 'db_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
        })
        .map(|v| v == DB_VERSION)
        .unwrap_or(false);

    if version_ok {
        return;
    }

    println!("[DB] Version mismatch or missing — deleting old DB files to start clean...");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path));
    let _ = std::fs::remove_file(format!("{}-shm", path));
    println!("[DB] Old DB files removed.");
}

/// Run the DB version check exactly once before the first connection is opened.
fn ensure_db_version_once(path: &str) {
    VERSION_CHECK.call_once(|| ensure_db_version(path));
}

// ============================================================================
// DLP Action Status Codes
// ============================================================================

/// DLP action: Content passed without any sensitive data detected
pub const DLP_ACTION_PASSED: i32 = 0;

/// DLP action: Sensitive data was detected and redacted
pub const DLP_ACTION_REDACTED: i32 = 1;

/// DLP action: Sensitive data was detected and request was blocked
pub const DLP_ACTION_BLOCKED: i32 = 2;

/// DLP action: Request was blocked due to token limit
pub const DLP_ACTION_RATELIMITED: i32 = 3;

/// Thread-safe database wrapper
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, rusqlite::Error> {
        ensure_db_version_once(path);

        let conn = Connection::open(path)?;

        // SQLite performance settings
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;
            PRAGMA temp_store = MEMORY;
            PRAGMA busy_timeout = 5000;
            PRAGMA auto_vacuum = FULL;
        ")?;

        // Create requests table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                backend TEXT NOT NULL DEFAULT 'claude',
                endpoint_name TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                model TEXT,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cache_read_tokens INTEGER DEFAULT 0,
                cache_creation_tokens INTEGER DEFAULT 0,
                latency_ms INTEGER DEFAULT 0,
                has_system_prompt INTEGER DEFAULT 0,
                has_tools INTEGER DEFAULT 0,
                has_thinking INTEGER DEFAULT 0,
                stop_reason TEXT,
                user_message_count INTEGER DEFAULT 0,
                assistant_message_count INTEGER DEFAULT 0,
                response_status INTEGER,
                is_streaming INTEGER NOT NULL DEFAULT 0,
                request_body TEXT,
                response_body TEXT,
                extra_metadata TEXT,
                request_headers TEXT,
                response_headers TEXT,
                dlp_action INTEGER DEFAULT 0,
                tokens_saved INTEGER DEFAULT 0,
                token_saving_meta TEXT
            )",
            [],
        )?;

        // Create index for faster generation_id lookups (timestamp + backend filtering)
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_requests_timestamp_backend ON requests(timestamp, backend)",
            [],
        );

        // Create settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        // Create DLP patterns table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dlp_patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                pattern_type TEXT NOT NULL,
                patterns TEXT NOT NULL,
                negative_pattern_type TEXT,
                negative_patterns TEXT,
                enabled INTEGER DEFAULT 1,
                min_occurrences INTEGER DEFAULT 1,
                min_unique_chars INTEGER DEFAULT 0,
                is_builtin INTEGER DEFAULT 0,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Seed builtin patterns if not exists
        Self::seed_builtin_patterns(&conn)?;

        // Create DLP detections table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dlp_detections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id INTEGER,
                timestamp TEXT NOT NULL,
                pattern_name TEXT NOT NULL,
                pattern_type TEXT NOT NULL,
                original_value TEXT NOT NULL,
                placeholder TEXT NOT NULL,
                message_index INTEGER,
                FOREIGN KEY (request_id) REFERENCES requests(id)
            )",
            [],
        )?;

        // Index for faster cleanup of dlp_detections by request_id
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_dlp_detections_request_id ON dlp_detections(request_id)",
            [],
        );

        // Create tool_calls table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tool_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id INTEGER NOT NULL,
                tool_call_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                tool_input TEXT NOT NULL
            )",
            [],
        )?;

        // Index for faster lookup of tool_calls by request_id
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tool_calls_request_id ON tool_calls(request_id)",
            [],
        );

        // Create predefined backend settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS predefined_backend_settings (
                name TEXT PRIMARY KEY,
                settings TEXT DEFAULT '{}',
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Store version
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('db_version', ?1)",
            rusqlite::params![DB_VERSION],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Seed builtin DLP patterns, overwriting if they already exist
    fn seed_builtin_patterns(conn: &Connection) -> Result<(), rusqlite::Error> {
        let builtin_patterns = get_builtin_patterns();
        let created_at = chrono::Utc::now().to_rfc3339();

        for pattern in builtin_patterns {
            // Convert static slices to JSON strings for storage
            let patterns_vec: Vec<&str> = pattern.patterns.to_vec();
            let patterns_json =
                serde_json::to_string(&patterns_vec).unwrap_or_else(|_| "[]".to_string());
            let negative_patterns_json = pattern.negative_patterns.map(|np| {
                let np_vec: Vec<&str> = np.to_vec();
                serde_json::to_string(&np_vec).unwrap_or_else(|_| "[]".to_string())
            });

            // Check if this builtin pattern already exists
            let existing_id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM dlp_patterns WHERE is_builtin = 1 AND name = ?1",
                    rusqlite::params![pattern.name],
                    |row| row.get(0),
                )
                .ok();

            if let Some(id) = existing_id {
                // Update existing pattern (preserve enabled state)
                conn.execute(
                    "UPDATE dlp_patterns SET pattern_type = ?1, patterns = ?2, negative_pattern_type = ?3, negative_patterns = ?4, min_occurrences = ?5, min_unique_chars = ?6 WHERE id = ?7",
                    rusqlite::params![
                        pattern.pattern_type,
                        patterns_json,
                        pattern.negative_pattern_type,
                        negative_patterns_json,
                        pattern.min_occurrences,
                        pattern.min_unique_chars,
                        id
                    ],
                )?;
            } else {
                // Insert new pattern
                conn.execute(
                    "INSERT INTO dlp_patterns (name, pattern_type, patterns, negative_pattern_type, negative_patterns, enabled, min_occurrences, min_unique_chars, is_builtin, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, 1, ?8)",
                    rusqlite::params![
                        pattern.name,
                        pattern.pattern_type,
                        patterns_json,
                        pattern.negative_pattern_type,
                        negative_patterns_json,
                        pattern.min_occurrences,
                        pattern.min_unique_chars,
                        created_at
                    ],
                )?;
            }
        }

        Ok(())
    }


    #[allow(clippy::too_many_arguments)]
    pub fn log_request(
        &self,
        backend: &str,
        method: &str,
        path: &str,
        endpoint_name: &str,
        request_body: &str,
        response_body: &str,
        response_status: u16,
        is_streaming: bool,
        latency_ms: u64,
        req_meta: &RequestMetadata,
        resp_meta: &ResponseMetadata,
        extra_metadata: Option<&str>,
        request_headers: Option<&str>,
        response_headers: Option<&str>,
        dlp_action: i32,
        tokens_saved: i32,
        token_saving_meta: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let timestamp = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO requests (
                timestamp, backend, endpoint_name, method, path, model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                latency_ms, has_system_prompt, has_tools, has_thinking, stop_reason,
                user_message_count, assistant_message_count,
                response_status, is_streaming, request_body, response_body, extra_metadata,
                request_headers, response_headers, dlp_action, tokens_saved, token_saving_meta
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
            rusqlite::params![
                timestamp,
                backend,
                endpoint_name,
                method,
                path,
                req_meta.model,
                resp_meta.input_tokens,
                resp_meta.output_tokens,
                resp_meta.cache_read_tokens,
                resp_meta.cache_creation_tokens,
                latency_ms as i64,
                req_meta.has_system_prompt as i32,
                req_meta.has_tools as i32,
                resp_meta.has_thinking as i32,
                resp_meta.stop_reason,
                req_meta.user_message_count,
                req_meta.assistant_message_count,
                response_status,
                is_streaming as i32,
                request_body,
                response_body,
                extra_metadata,
                request_headers,
                response_headers,
                dlp_action,
                tokens_saved,
                token_saving_meta,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    pub fn log_dlp_detections(
        &self,
        request_id: i64,
        detections: &[DlpDetection],
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let timestamp = chrono::Utc::now().to_rfc3339();

        for detection in detections {
            conn.execute(
                "INSERT INTO dlp_detections (request_id, timestamp, pattern_name, pattern_type, original_value, placeholder, message_index)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    request_id,
                    timestamp,
                    detection.pattern_name,
                    detection.pattern_type,
                    detection.original_value,
                    "",
                    detection.message_index,
                ],
            )?;
        }

        Ok(())
    }

    pub fn log_tool_calls(
        &self,
        request_id: i64,
        tool_calls: &[crate::requestresponsemetadata::ToolCall],
    ) -> Result<(), rusqlite::Error> {
        if tool_calls.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().unwrap();

        for tool_call in tool_calls {
            let input_json = serde_json::to_string(&tool_call.input).unwrap_or_default();
            conn.execute(
                "INSERT INTO tool_calls (request_id, tool_call_id, tool_name, tool_input)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    request_id,
                    tool_call.id,
                    tool_call.name,
                    input_json,
                ],
            )?;
        }

        Ok(())
    }

    // ========================================================================
    // Agent Hooks Methods (shared by Cursor / Claude Code / Codex hook receivers)
    // ========================================================================
}

/// Real token usage pulled from a transcript / API response. When passed to
/// `update_agent_hook_output`, these values **overwrite** the existing row's
/// columns instead of being added to them. Used by Claude Code's Stop hook,
/// which has access to the actual API usage block in the transcript JSONL
/// (Cursor calls pass `None` and keep the additive behavior).
#[derive(Debug, Clone, Default)]
pub struct RealUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: i32,
    pub cache_creation_tokens: i32,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
}

impl Database {

    /// Log an agent hook request (creates new entry, or upgrades an existing
    /// row keyed on `correlation_id` for the given `backend`).
    ///
    /// `correlation_id` is a stable per-turn / per-tool-call identifier that
    /// joins multiple hook events for the same logical request:
    ///   - Cursor hooks pass `generation_id`
    ///   - Claude Code hooks pass `session_id` (for prompt rows) or
    ///     `tool_use_id` (for individual tool rows)
    ///
    /// `backend` is the value written to (and matched on) the `backend` column
    /// — e.g. `"cursor-hooks"`, `"claude-hooks"`.
    #[allow(clippy::too_many_arguments)]
    pub fn log_agent_hook_request(
        &self,
        backend: &str,
        correlation_id: &str,
        endpoint_name: &str,
        model: &str,
        input_tokens: i32,
        output_tokens: i32,
        request_body: &str,
        response_body: &str,
        response_status: u16,
        extra_metadata: Option<&str>,
        request_headers: Option<&str>,
        response_headers: Option<&str>,
        dlp_action: i32,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let timestamp = chrono::Utc::now().to_rfc3339();

        println!("[DB] log_agent_hook_request - backend: {}, correlation_id: {}, endpoint: {}", backend, correlation_id, endpoint_name);

        // Check if entry already exists for this correlation_id (within last 5 minutes for faster lookup)
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        let existing_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM requests WHERE timestamp >= ?1 AND backend = ?2 AND json_extract(extra_metadata, '$.correlation_id') = ?3",
                rusqlite::params![cutoff, backend, correlation_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing_id {
            println!("[DB] log_agent_hook_request - found existing entry id: {}, updating", id);
            // Update existing entry - only upgrade dlp_action (blocked > redacted > passed)
            conn.execute(
                "UPDATE requests SET
                    input_tokens = input_tokens + ?1,
                    response_status = CASE WHEN ?2 > response_status THEN ?2 ELSE response_status END,
                    dlp_action = CASE WHEN ?3 > dlp_action THEN ?3 ELSE dlp_action END
                 WHERE id = ?4",
                rusqlite::params![input_tokens, response_status, dlp_action, id],
            )?;
            return Ok(id);
        }

        println!("[DB] log_agent_hook_request - creating new entry");
        // Create new entry
        let path_value = format!("/{}", backend);
        conn.execute(
            "INSERT INTO requests (
                timestamp, backend, endpoint_name, method, path, model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                latency_ms, has_system_prompt, has_tools, has_thinking, stop_reason,
                user_message_count, assistant_message_count,
                response_status, is_streaming, request_body, response_body, extra_metadata,
                request_headers, response_headers, dlp_action
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            rusqlite::params![
                timestamp,
                backend,
                endpoint_name,
                "POST",
                path_value,
                if model.is_empty() { None } else { Some(model) },
                input_tokens,
                output_tokens,
                0, // cache_read_tokens
                0, // cache_creation_tokens
                0, // latency_ms (not applicable for hooks)
                0, // has_system_prompt
                0, // has_tools
                0, // has_thinking
                None::<String>, // stop_reason
                1, // user_message_count (prompt)
                0, // assistant_message_count
                response_status,
                0, // is_streaming
                request_body,
                response_body,
                extra_metadata,
                request_headers,
                response_headers,
                dlp_action,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Update agent-hook output tokens, response body, and latency by
    /// `correlation_id`. Returns true if an entry was found and updated.
    ///
    /// When `real_usage` is `Some`, the row's token columns are **overwritten**
    /// with the values it carries (used by Claude Code's Stop hook, which gets
    /// real numbers from the transcript). When `None`, `output_token_count` is
    /// **added** to the existing `output_tokens` (matches Cursor's additive
    /// flow where each `after_*` event accumulates).
    pub fn update_agent_hook_output(
        &self,
        backend: &str,
        correlation_id: &str,
        output_token_count: i32,
        response_text: Option<&str>,
        real_usage: Option<&RealUsage>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Find the request by correlation_id in extra_metadata. The 30-minute
        // window matches `update_latest_agent_hook_with_usage` /
        // `close_latest_agent_hook_row_additive` and is wide enough for long
        // turn-scoped flows (Codex Stop following UserPromptSubmit can be more
        // than 5 minutes apart on long agentic turns).
        // Also pulls timestamp for latency calculation.
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();
        let existing: Option<(i64, i32, String)> = conn
            .query_row(
                "SELECT id, output_tokens, timestamp FROM requests WHERE timestamp >= ?1 AND backend = ?2 AND json_extract(extra_metadata, '$.correlation_id') = ?3",
                rusqlite::params![cutoff, backend, correlation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        if let Some((id, current_output, timestamp_str)) = existing {
            // Calculate latency from stored timestamp
            let latency_ms = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|start_time| {
                    let now = chrono::Utc::now();
                    (now.signed_duration_since(start_time)).num_milliseconds().max(0) as i64
                })
                .unwrap_or(0);

            if let Some(usage) = real_usage {
                // Overwrite mode: real numbers from a transcript / API response.
                conn.execute(
                    "UPDATE requests SET
                        input_tokens = ?1,
                        output_tokens = ?2,
                        cache_read_tokens = ?3,
                        cache_creation_tokens = ?4,
                        model = COALESCE(?5, model),
                        stop_reason = COALESCE(?6, stop_reason),
                        response_body = COALESCE(?7, response_body),
                        assistant_message_count = 1,
                        latency_ms = ?8
                     WHERE id = ?9",
                    rusqlite::params![
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.cache_read_tokens,
                        usage.cache_creation_tokens,
                        usage.model,
                        usage.stop_reason,
                        response_text,
                        latency_ms,
                        id,
                    ],
                )?;
            } else {
                // Additive mode (Cursor flow).
                let new_output = current_output + output_token_count;
                if let Some(text) = response_text {
                    conn.execute(
                        "UPDATE requests SET output_tokens = ?1, response_body = ?2, assistant_message_count = 1, latency_ms = ?3 WHERE id = ?4",
                        rusqlite::params![new_output, text, latency_ms, id],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE requests SET output_tokens = ?1, latency_ms = ?2 WHERE id = ?3",
                        rusqlite::params![new_output, latency_ms, id],
                    )?;
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Find the most-recent agent-hook row for a given backend / session,
    /// matching on `endpoint_name` and the `session_id` field inside
    /// `extra_metadata`, and **overwrite** its token columns with `usage`.
    ///
    /// Used by Claude Code's Stop hook, which fires per-turn but doesn't
    /// expose a per-turn ID — so we generate a unique correlation_id at
    /// UserPromptSubmit time and resolve "which row to close" at Stop time
    /// by picking the latest row in this session that hasn't been closed yet
    /// (`assistant_message_count = 0`).
    ///
    /// Returns true if a row was found and updated.
    pub fn update_latest_agent_hook_with_usage(
        &self,
        backend: &str,
        session_id: &str,
        endpoint_name: &str,
        usage: &RealUsage,
        response_text: Option<&str>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();

        let existing: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, timestamp FROM requests
                 WHERE timestamp >= ?1
                   AND backend = ?2
                   AND endpoint_name = ?3
                   AND json_extract(extra_metadata, '$.session_id') = ?4
                   AND assistant_message_count = 0
                 ORDER BY id DESC
                 LIMIT 1",
                rusqlite::params![cutoff, backend, endpoint_name, session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        let Some((id, timestamp_str)) = existing else {
            return Ok(false);
        };

        let latency_ms = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
            .map(|start_time| {
                let now = chrono::Utc::now();
                (now.signed_duration_since(start_time))
                    .num_milliseconds()
                    .max(0) as i64
            })
            .unwrap_or(0);

        conn.execute(
            "UPDATE requests SET
                input_tokens = ?1,
                output_tokens = ?2,
                cache_read_tokens = ?3,
                cache_creation_tokens = ?4,
                model = COALESCE(?5, model),
                stop_reason = COALESCE(?6, stop_reason),
                response_body = COALESCE(?7, response_body),
                assistant_message_count = 1,
                latency_ms = ?8
             WHERE id = ?9",
            rusqlite::params![
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                usage.model,
                usage.stop_reason,
                response_text,
                latency_ms,
                id,
            ],
        )?;

        Ok(true)
    }

    /// Close out the most-recent open agent-hook row for a session **without
    /// touching its `input_tokens`**: additively bumps `output_tokens`, sets
    /// `assistant_message_count = 1`, updates `response_body` / `latency_ms` /
    /// `model` / `stop_reason`. Used by Codex's Stop handler in the rare path
    /// where neither `turn_id` nor a parseable transcript is available, so we
    /// only know an *estimated* output token count and must preserve the
    /// estimated input token count minted at UserPromptSubmit time.
    ///
    /// Returns true if a row was found and closed.
    #[allow(clippy::too_many_arguments)]
    pub fn close_latest_agent_hook_row_additive(
        &self,
        backend: &str,
        session_id: &str,
        endpoint_name: &str,
        output_token_count: i32,
        response_text: Option<&str>,
        model: Option<&str>,
        stop_reason: Option<&str>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();

        let existing: Option<(i64, i32, String)> = conn
            .query_row(
                "SELECT id, output_tokens, timestamp FROM requests
                 WHERE timestamp >= ?1
                   AND backend = ?2
                   AND endpoint_name = ?3
                   AND json_extract(extra_metadata, '$.session_id') = ?4
                   AND assistant_message_count = 0
                 ORDER BY id DESC
                 LIMIT 1",
                rusqlite::params![cutoff, backend, endpoint_name, session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let Some((id, current_output, timestamp_str)) = existing else {
            return Ok(false);
        };

        let latency_ms = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
            .map(|start_time| {
                let now = chrono::Utc::now();
                (now.signed_duration_since(start_time))
                    .num_milliseconds()
                    .max(0) as i64
            })
            .unwrap_or(0);

        let new_output = current_output + output_token_count;

        conn.execute(
            "UPDATE requests SET
                output_tokens = ?1,
                model = COALESCE(?2, model),
                stop_reason = COALESCE(?3, stop_reason),
                response_body = COALESCE(?4, response_body),
                assistant_message_count = 1,
                latency_ms = ?5
             WHERE id = ?6",
            rusqlite::params![
                new_output,
                model,
                stop_reason,
                response_text,
                latency_ms,
                id,
            ],
        )?;

        Ok(true)
    }

    /// Add thinking tokens to an agent-hook row by `correlation_id`.
    /// Returns true if an entry was found and updated, false otherwise.
    pub fn add_agent_hook_thinking_tokens(
        &self,
        backend: &str,
        correlation_id: &str,
        thinking_word_count: i32,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Find and update the request (within last 5 minutes for faster lookup)
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();

        println!("[DB] add_agent_hook_thinking_tokens - backend: {}, correlation_id: {}", backend, correlation_id);

        let rows_affected = conn.execute(
            "UPDATE requests SET
                output_tokens = output_tokens + ?1,
                has_thinking = 1
             WHERE timestamp >= ?2 AND backend = ?3 AND json_extract(extra_metadata, '$.correlation_id') = ?4",
            rusqlite::params![thinking_word_count, cutoff, backend, correlation_id],
        )?;

        println!("[DB] add_agent_hook_thinking_tokens - rows_affected: {}", rows_affected);

        Ok(rows_affected > 0)
    }

    // ========================================================================
    // Predefined Backend Settings Methods
    // ========================================================================

    /// Get settings for a predefined backend (returns default settings if not set)
    pub fn get_predefined_backend_settings(&self, name: &str) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let settings: Option<String> = conn
            .query_row(
                "SELECT settings FROM predefined_backend_settings WHERE name = ?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .ok();

        // Return stored settings or default
        Ok(settings.unwrap_or_else(|| "{}".to_string()))
    }

    /// Update settings for a predefined backend
    pub fn update_predefined_backend_settings(&self, name: &str, settings: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let updated_at = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO predefined_backend_settings (name, settings, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET settings = ?2, updated_at = ?3",
            rusqlite::params![name, settings, updated_at],
        )?;

        Ok(())
    }

    /// Reset predefined backend settings to defaults (delete the record)
    pub fn reset_predefined_backend_settings(&self, name: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "DELETE FROM predefined_backend_settings WHERE name = ?1",
            rusqlite::params![name],
        )?;

        Ok(())
    }
}

pub fn open_connection() -> Result<Connection, rusqlite::Error> {
    let path = get_db_path();
    ensure_db_version_once(path);
    Connection::open(path)
}

// Port management helpers

pub fn get_port_from_db() -> u16 {
    let conn = match open_connection() {
        Ok(c) => c,
        Err(_) => return DEFAULT_PORT,
    };

    // Ensure settings table exists
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    );

    conn.query_row(
        "SELECT value FROM settings WHERE key = 'proxy_port'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(DEFAULT_PORT)
}

pub fn save_port_to_db(port: u16) -> Result<(), String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_port', ?1)",
        rusqlite::params![port.to_string()],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
