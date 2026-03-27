// Database operations and schema management

use crate::builtin_patterns::get_builtin_patterns;
use crate::dlp::DlpDetection;
use crate::dlp_pattern_config::{get_db_path, DEFAULT_PORT};
use crate::requestresponsemetadata::{RequestMetadata, ResponseMetadata};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

// ============================================================================
// DLP Action Status Codes
// ============================================================================

/// DLP action: Content passed without any sensitive data detected
pub const DLP_ACTION_PASSED: i32 = 0;

/// DLP action: Sensitive data was detected and redacted
pub const DLP_ACTION_REDACTED: i32 = 1;

/// DLP action: Sensitive data was detected and request was blocked
pub const DLP_ACTION_BLOCKED: i32 = 2;

/// DLP action: Request was blocked due to rate limit or token limit
pub const DLP_ACTION_RATELIMITED: i32 = 3;

/// DLP action: Token limit exceeded but request was allowed (notify mode)
pub const DLP_ACTION_NOTIFY_RATELIMIT: i32 = 4;

/// Thread-safe database wrapper
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;

        // Load zstd compression extension
        sqlite_zstd::load(&conn).map_err(|e| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(1),
                Some(format!("Failed to load sqlite-zstd: {}", e)),
            )
        })?;

        // SQLite performance settings
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;
            PRAGMA temp_store = MEMORY;
            PRAGMA busy_timeout = 5000;
        ")?;

        // Check and migrate auto_vacuum mode if needed (one-time migration)
        // auto_vacuum can only be changed on empty DB or by running VACUUM after setting it
        let auto_vacuum_mode: i32 = conn
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .unwrap_or(0);
        if auto_vacuum_mode != 1 {
            // 1 = FULL
            println!("[DB] Migrating auto_vacuum to FULL mode (one-time operation)...");
            conn.execute_batch("
                PRAGMA auto_vacuum = FULL;
                VACUUM;
            ")?;
            println!("[DB] auto_vacuum migration complete");
        }

        // Migration: If zstd compression was already enabled, `requests` is a VIEW and
        // ALTER TABLE silently fails. Detect if underlying _requests_zstd is missing new columns
        // and rebuild the requests storage from scratch (discards old logs, keeps settings).
        Self::migrate_compressed_schema_if_needed(&conn);

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

        // Migration: Add extra_metadata column if it doesn't exist (for existing databases)
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN extra_metadata TEXT",
            [],
        );

        // Migration: Add request_headers column if it doesn't exist (for existing databases)
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN request_headers TEXT",
            [],
        );

        // Migration: Add response_headers column if it doesn't exist (for existing databases)
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN response_headers TEXT",
            [],
        );

        // Migration: Add dlp_action column if it doesn't exist
        // Uses DLP_ACTION_PASSED (0), DLP_ACTION_REDACTED (1), DLP_ACTION_BLOCKED (2)
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN dlp_action INTEGER DEFAULT 0",
            [],
        );

        // Migration: Add token saving columns if they don't exist
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN tokens_saved INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN token_saving_meta TEXT",
            [],
        );

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

        // Create tool_calls table (no FK constraint - requests is a view due to zstd compression)
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

        // Create custom backends table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS custom_backends (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                base_url TEXT NOT NULL,
                settings TEXT DEFAULT '{}',
                enabled INTEGER DEFAULT 1,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create index for faster backend name lookups
        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_custom_backends_name ON custom_backends(name)",
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

        // Enable transparent zstd compression on large columns if not already enabled
        Self::enable_compression_if_needed(&conn)?;

        // Backfill tool_calls for existing requests
        Self::backfill_tool_calls(&conn);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Detect if the compressed _requests_zstd table exists but is missing new columns.
    /// If so, drop all log-related tables and let the init flow recreate them.
    /// Settings, DLP patterns, and custom backends are preserved.
    fn migrate_compressed_schema_if_needed(conn: &Connection) {
        // Check if _requests_zstd exists (compression was previously enabled)
        let has_compressed: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_requests_zstd'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_compressed {
            return; // Fresh DB or uncompressed - normal migrations will work fine
        }

        // Check if _requests_zstd has the tokens_saved column
        let has_column: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('_requests_zstd') WHERE name = 'tokens_saved'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if has_column {
            return; // Schema is up to date
        }

        println!("[DB] Compressed schema missing new columns - rebuilding requests storage (old logs will be discarded)...");

        // Drop everything related to requests logs (order matters for dependencies)
        let _ = conn.execute_batch("
            DROP TABLE IF EXISTS dlp_detections;
            DROP TABLE IF EXISTS tool_calls;
            DROP VIEW IF EXISTS requests;
            DROP TABLE IF EXISTS _requests_zstd;
            DROP TABLE IF EXISTS _requests_zstd_dicts;
            DROP TABLE IF EXISTS _requests_zstd_configs;
        ");

        // Reset the tool_calls backfill flag so it doesn't try to backfill non-existent data
        let _ = conn.execute(
            "DELETE FROM settings WHERE key = 'tool_calls_backfill_done'",
            [],
        );

        println!("[DB] Requests storage cleared. Will be recreated with new schema.");
    }

    /// One-time backfill of tool_calls from existing response bodies
    fn backfill_tool_calls(conn: &Connection) {
        // Check if backfill already done
        let already_done: bool = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'tool_calls_backfill_done'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map(|v| v == "true")
            .unwrap_or(false);

        if already_done {
            return;
        }

        println!("[DB] Backfilling tool_calls from existing requests...");

        // Get all Claude and Codex requests that might have tool calls
        let mut stmt = match conn.prepare(
            "SELECT id, backend, response_body, is_streaming FROM requests WHERE backend IN ('claude', 'codex') AND response_body IS NOT NULL"
        ) {
            Ok(s) => s,
            Err(e) => {
                println!("[DB] Failed to prepare backfill query: {}", e);
                return;
            }
        };

        let rows: Vec<(i64, String, String, bool)> = match stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, i32>(3).unwrap_or(0) == 1,
            ))
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                println!("[DB] Failed to query requests for backfill: {}", e);
                return;
            }
        };

        let mut total_tool_calls = 0;
        for (request_id, backend, response_body, is_streaming) in rows {
            let tool_calls = match backend.as_str() {
                "claude" => Self::extract_tool_calls_claude(&response_body, is_streaming),
                "codex" => Self::extract_tool_calls_codex(&response_body, is_streaming),
                _ => Vec::new(),
            };
            for tc in &tool_calls {
                let input_json = serde_json::to_string(&tc.input).unwrap_or_default();
                if conn.execute(
                    "INSERT INTO tool_calls (request_id, tool_call_id, tool_name, tool_input) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![request_id, tc.id, tc.name, input_json],
                ).is_ok() {
                    total_tool_calls += 1;
                }
            }
        }

        // Mark backfill as done
        let _ = conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('tool_calls_backfill_done', 'true')",
            [],
        );

        println!("[DB] Backfill complete. Extracted {} tool calls.", total_tool_calls);
    }

    /// Extract tool calls from Claude response body (used for backfill)
    fn extract_tool_calls_claude(body: &str, is_streaming: bool) -> Vec<crate::requestresponsemetadata::ToolCall> {
        use std::collections::HashMap;
        use crate::requestresponsemetadata::ToolCall;

        let mut tool_calls = Vec::new();

        if is_streaming {
            let mut tool_calls_map: HashMap<i64, (String, String, String)> = HashMap::new();

            for line in body.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                let json_str = &line[6..];

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match event_type {
                        "content_block_start" => {
                            if let Some(content_block) = json.get("content_block") {
                                if content_block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                                    let index = json.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let id = content_block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let name = content_block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    tool_calls_map.insert(index, (id, name, String::new()));
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = json.get("delta") {
                                if delta.get("type").and_then(|v| v.as_str()) == Some("input_json_delta") {
                                    let index = json.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if let Some(partial_json) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                        if let Some(entry) = tool_calls_map.get_mut(&index) {
                                            entry.2.push_str(partial_json);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            let mut sorted: Vec<(i64, ToolCall)> = tool_calls_map
                .into_iter()
                .map(|(index, (id, name, input_str))| {
                    let input = serde_json::from_str(&input_str).unwrap_or(serde_json::Value::Null);
                    (index, ToolCall { id, name, input })
                })
                .collect();
            sorted.sort_by_key(|(index, _)| *index);
            tool_calls = sorted.into_iter().map(|(_, tc)| tc).collect();
        } else {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                if let Some(content) = json.get("content").and_then(|v| v.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                            tool_calls.push(ToolCall { id, name, input });
                        }
                    }
                }
            }
        }

        tool_calls
    }

    /// Extract tool calls from Codex response body (used for backfill)
    fn extract_tool_calls_codex(body: &str, is_streaming: bool) -> Vec<crate::requestresponsemetadata::ToolCall> {
        use std::collections::HashMap;
        use crate::requestresponsemetadata::ToolCall;

        let mut tool_calls = Vec::new();

        if is_streaming {
            // Track by item_id: (call_id, name, accumulated_arguments)
            let mut function_calls_map: HashMap<String, (String, String, String)> = HashMap::new();

            for line in body.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                let json_str = &line[6..];

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match event_type {
                        "response.output_item.added" => {
                            if let Some(item) = json.get("item") {
                                if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                                    let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    function_calls_map.insert(item_id, (call_id, name, String::new()));
                                }
                            }
                        }
                        "response.function_call_arguments.delta" => {
                            // Delta events use item_id to identify which function call
                            if let Some(item_id) = json.get("item_id").and_then(|v| v.as_str()) {
                                if let Some(delta) = json.get("delta").and_then(|v| v.as_str()) {
                                    if let Some(entry) = function_calls_map.get_mut(item_id) {
                                        entry.2.push_str(delta);
                                    }
                                }
                            }
                        }
                        "response.completed" => {
                            // Also extract from completed response output
                            if let Some(response) = json.get("response") {
                                if let Some(output) = response.get("output").and_then(|v| v.as_array()) {
                                    for item in output {
                                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                            let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let arguments = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            if !function_calls_map.contains_key(&item_id) {
                                                function_calls_map.insert(item_id, (call_id, name, arguments));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            tool_calls = function_calls_map
                .into_iter()
                .map(|(_item_id, (call_id, name, arguments))| {
                    let input = serde_json::from_str(&arguments).unwrap_or(serde_json::Value::Null);
                    ToolCall { id: call_id, name, input }
                })
                .collect();
        } else {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                if let Some(output) = json.get("output").and_then(|v| v.as_array()) {
                    for item in output {
                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let arguments = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                            let input = serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
                            tool_calls.push(ToolCall { id: call_id, name, input });
                        }
                    }
                }
            }
        }

        tool_calls
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

    /// Enable transparent zstd compression on large text columns
    /// This is a one-time migration that compresses existing data
    fn enable_compression_if_needed(conn: &Connection) -> Result<(), rusqlite::Error> {
        // Check if compression is already enabled (shadow table exists)
        let is_compressed: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_requests_zstd'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if is_compressed {
            // Compression enabled - check how much pending work there is
            let pending_rows: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM _requests_zstd WHERE request_body IS NOT NULL AND typeof(request_body) = 'text'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            if pending_rows > 100 {
                // Lots of pending work (likely incomplete migration) - run full compression
                println!("[DB] Found {} uncompressed rows, running full compression...", pending_rows);
                let _ = conn.query_row(
                    "SELECT zstd_incremental_maintenance(null, 1)",
                    [],
                    |_| Ok(()),
                );
                println!("[DB] Compression complete!");
            } else if pending_rows > 0 {
                // Just a few new rows - quick maintenance
                let _ = conn.query_row(
                    "SELECT zstd_incremental_maintenance(5.0, 1)",
                    [],
                    |_| Ok(()),
                );
            }
            return Ok(());
        }

        // Enable transparent compression on large columns (even if empty - ready for new data)
        // Using compression level 3 (fast) with single shared dictionary
        println!("[DB] Enabling zstd compression on requests table...");

        conn.execute(
            "SELECT zstd_enable_transparent('{\"table\": \"requests\", \"column\": \"request_body\", \"compression_level\": 3, \"dict_chooser\": \"''all''\"}')",
            [],
        )?;
        conn.execute(
            "SELECT zstd_enable_transparent('{\"table\": \"requests\", \"column\": \"response_body\", \"compression_level\": 3, \"dict_chooser\": \"''all''\"}')",
            [],
        )?;
        conn.execute(
            "SELECT zstd_enable_transparent('{\"table\": \"requests\", \"column\": \"request_headers\", \"compression_level\": 3, \"dict_chooser\": \"''all''\"}')",
            [],
        )?;
        conn.execute(
            "SELECT zstd_enable_transparent('{\"table\": \"requests\", \"column\": \"response_headers\", \"compression_level\": 3, \"dict_chooser\": \"''all''\"}')",
            [],
        )?;

        // Check if there's existing data to compress
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _requests_zstd", [], |row| row.get(0))
            .unwrap_or(0);

        if row_count > 0 {
            println!("[DB] Compressing {} existing rows (this may take a moment)...", row_count);
            conn.query_row(
                "SELECT zstd_incremental_maintenance(null, 1)",
                [],
                |_| Ok(()),
            )?;
            println!("[DB] Compression complete!");
        } else {
            println!("[DB] Compression enabled, ready for new data.");
        }

        Ok(())
    }

    /// Clean up data older than 7 days
    pub fn cleanup_old_data(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
        let cutoff_ts = cutoff.to_rfc3339();

        // Delete DLP detections for requests that will be deleted (by relationship, not timestamp)
        conn.execute(
            "DELETE FROM dlp_detections WHERE request_id IN (SELECT id FROM requests WHERE timestamp < ?1)",
            rusqlite::params![cutoff_ts],
        )?;

        // Delete tool calls for requests that will be deleted
        conn.execute(
            "DELETE FROM tool_calls WHERE request_id IN (SELECT id FROM requests WHERE timestamp < ?1)",
            rusqlite::params![cutoff_ts],
        )?;

        // Delete old requests
        conn.execute(
            "DELETE FROM requests WHERE timestamp < ?1",
            rusqlite::params![cutoff_ts],
        )
    }

    /// Run incremental compression maintenance if needed
    /// Returns Ok(true) if compression was performed, Ok(false) if skipped
    /// This is designed to be called periodically from a background task
    pub fn run_compression_maintenance(&self) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Check if compression is enabled (shadow table exists)
        let is_compressed: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_requests_zstd'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !is_compressed {
            return Ok(false);
        }

        // Check how many uncompressed rows we have (fast query)
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _requests_zstd WHERE request_body IS NOT NULL AND typeof(request_body) = 'text'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Only compress if we have > 20 pending rows (avoid unnecessary work)
        if pending <= 20 {
            return Ok(false);
        }

        // Very short burst (1 sec), low db_load (0.25 = 75% time available for other queries)
        // This means actual lock time is ~250ms max per cycle
        let _ = conn.query_row(
            "SELECT zstd_incremental_maintenance(1.0, 0.25)",
            [],
            |_| Ok(()),
        );

        Ok(true)
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

        // With zstd compression enabled, 'requests' is a view and last_insert_rowid()
        // returns 0 because the actual insert happens via an INSTEAD OF trigger.
        // Query the actual ID from the underlying table.
        let request_id: i64 = conn.query_row(
            "SELECT MAX(id) FROM _requests_zstd",
            [],
            |row| row.get(0),
        )?;

        Ok(request_id)
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
                    detection.placeholder,
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
    // Cursor Hooks Methods
    // ========================================================================

    /// Log a cursor hook request (creates new entry)
    #[allow(clippy::too_many_arguments)]
    pub fn log_cursor_hook_request(
        &self,
        generation_id: &str,
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

        println!("[DB] log_cursor_hook_request - generation_id: {}, endpoint: {}", generation_id, endpoint_name);

        // Check if entry already exists for this generation_id (within last 5 minutes for faster lookup)
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        let existing_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM requests WHERE timestamp >= ?1 AND backend = 'cursor-hooks' AND json_extract(extra_metadata, '$.generation_id') = ?2",
                rusqlite::params![cutoff, generation_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing_id {
            println!("[DB] log_cursor_hook_request - found existing entry id: {}, updating", id);
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

        println!("[DB] log_cursor_hook_request - creating new entry");
        // Create new entry
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
                "cursor-hooks",
                endpoint_name,
                "POST",
                "/cursor_hook",
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

        // With zstd compression enabled, 'requests' is a view and last_insert_rowid()
        // returns 0 because the actual insert happens via an INSTEAD OF trigger.
        // Query the actual ID from the underlying table.
        let request_id: i64 = conn.query_row(
            "SELECT MAX(id) FROM _requests_zstd",
            [],
            |row| row.get(0),
        )?;

        Ok(request_id)
    }

    /// Update cursor hook output tokens, response body, and latency by generation_id
    /// Returns true if an entry was found and updated, false otherwise
    pub fn update_cursor_hook_output(
        &self,
        generation_id: &str,
        output_token_count: i32,
        response_text: Option<&str>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Find the request by generation_id in extra_metadata (within last 5 minutes for faster lookup)
        // Also get timestamp for latency calculation
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        let existing: Option<(i64, i32, String)> = conn
            .query_row(
                "SELECT id, output_tokens, timestamp FROM requests WHERE timestamp >= ?1 AND backend = 'cursor-hooks' AND json_extract(extra_metadata, '$.generation_id') = ?2",
                rusqlite::params![cutoff, generation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        if let Some((id, current_output, timestamp_str)) = existing {
            let new_output = current_output + output_token_count;

            // Calculate latency from stored timestamp
            let latency_ms = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|start_time| {
                    let now = chrono::Utc::now();
                    (now.signed_duration_since(start_time)).num_milliseconds().max(0) as i64
                })
                .unwrap_or(0);

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
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Add thinking tokens to cursor hook output by generation_id
    /// Returns true if an entry was found and updated, false otherwise
    pub fn add_cursor_hook_thinking_tokens(
        &self,
        generation_id: &str,
        thinking_word_count: i32,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Find and update the request (within last 5 minutes for faster lookup)
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();

        // Debug: Check what entries exist and their extra_metadata values
        println!("[DB] add_cursor_hook_thinking_tokens - looking for generation_id: {}", generation_id);

        // Query from the underlying table to see what's actually stored
        let debug_result: Result<(i64, Option<String>), _> = conn.query_row(
            "SELECT id, extra_metadata FROM _requests_zstd WHERE timestamp >= ?1 AND backend = 'cursor-hooks' ORDER BY id DESC LIMIT 1",
            rusqlite::params![cutoff],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match debug_result {
            Ok((id, extra_meta)) => {
                println!("[DB] Found entry id: {}, extra_metadata: {:?}", id, extra_meta);
                if let Some(meta) = &extra_meta {
                    // Try to parse and extract generation_id
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(meta) {
                        println!("[DB] Parsed JSON, generation_id in db: {:?}", json.get("generation_id"));
                    }
                }
            }
            Err(e) => {
                println!("[DB] No entries found or error: {}", e);
            }
        }

        // Use the underlying table for the update
        let rows_affected = conn.execute(
            "UPDATE _requests_zstd SET
                output_tokens = output_tokens + ?1,
                has_thinking = 1
             WHERE timestamp >= ?2 AND backend = 'cursor-hooks' AND json_extract(extra_metadata, '$.generation_id') = ?3",
            rusqlite::params![thinking_word_count, cutoff, generation_id],
        )?;

        println!("[DB] add_cursor_hook_thinking_tokens - rows_affected: {}", rows_affected);

        Ok(rows_affected > 0)
    }

    // ========================================================================
    // Custom Backends Methods
    // ========================================================================

    /// Get all custom backends
    pub fn get_custom_backends(&self) -> Result<Vec<CustomBackendRecord>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, settings, enabled, created_at FROM custom_backends ORDER BY created_at DESC",
        )?;

        let backends = stmt
            .query_map([], |row| {
                Ok(CustomBackendRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    settings: row.get(3)?,
                    enabled: row.get::<_, i32>(4)? == 1,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(backends)
    }

    /// Get enabled custom backends only
    pub fn get_enabled_custom_backends(&self) -> Result<Vec<CustomBackendRecord>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, settings, enabled, created_at FROM custom_backends WHERE enabled = 1 ORDER BY created_at DESC",
        )?;

        let backends = stmt
            .query_map([], |row| {
                Ok(CustomBackendRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    settings: row.get(3)?,
                    enabled: row.get::<_, i32>(4)? == 1,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(backends)
    }

    /// Add a new custom backend
    pub fn add_custom_backend(
        &self,
        name: &str,
        base_url: &str,
        settings: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let created_at = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO custom_backends (name, base_url, settings, enabled, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![name, base_url, settings, created_at],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Update a custom backend
    pub fn update_custom_backend(
        &self,
        id: i64,
        name: &str,
        base_url: &str,
        settings: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "UPDATE custom_backends SET name = ?1, base_url = ?2, settings = ?3 WHERE id = ?4",
            rusqlite::params![name, base_url, settings, id],
        )?;

        Ok(())
    }

    /// Toggle a custom backend enabled/disabled
    pub fn toggle_custom_backend(&self, id: i64, enabled: bool) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "UPDATE custom_backends SET enabled = ?1 WHERE id = ?2",
            rusqlite::params![enabled as i32, id],
        )?;

        Ok(())
    }

    /// Delete a custom backend
    pub fn delete_custom_backend(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        conn.execute("DELETE FROM custom_backends WHERE id = ?1", rusqlite::params![id])?;

        Ok(())
    }

    /// Check if a backend name already exists (reserved or custom)
    pub fn backend_name_exists(&self, name: &str) -> Result<bool, rusqlite::Error> {
        // Check reserved names first
        let reserved = ["claude", "codex", "cursor_hook", "cursor-hooks"];
        if reserved.contains(&name.to_lowercase().as_str()) {
            return Ok(true);
        }

        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM custom_backends WHERE LOWER(name) = LOWER(?1)",
            rusqlite::params![name],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Check if a backend name exists excluding a specific id (for updates)
    pub fn backend_name_exists_excluding(&self, name: &str, exclude_id: i64) -> Result<bool, rusqlite::Error> {
        // Check reserved names first
        let reserved = ["claude", "codex", "cursor_hook", "cursor-hooks"];
        if reserved.contains(&name.to_lowercase().as_str()) {
            return Ok(true);
        }

        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM custom_backends WHERE LOWER(name) = LOWER(?1) AND id != ?2",
            rusqlite::params![name, exclude_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
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

/// Custom backend record from database
#[derive(Debug, Clone)]
pub struct CustomBackendRecord {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub settings: String,
    pub enabled: bool,
    pub created_at: String,
}

// Helper to open connection with zstd extension loaded
pub fn open_connection() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(get_db_path())?;
    sqlite_zstd::load(&conn).map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some(format!("Failed to load sqlite-zstd: {}", e)),
        )
    })?;
    Ok(conn)
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

// DLP action setting helpers

pub fn get_dlp_action_from_db() -> String {
    let conn = match open_connection() {
        Ok(c) => c,
        Err(_) => return "block".to_string(),
    };

    // Ensure settings table exists
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    );

    conn.query_row(
        "SELECT value FROM settings WHERE key = 'dlp_action'",
        [],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "block".to_string())
}

pub fn save_dlp_action_to_db(action: &str) -> Result<(), String> {
    // Validate action value
    if action != "redact" && action != "block" {
        return Err("Invalid dlp_action value. Must be 'redact' or 'block'".to_string());
    }

    let conn = open_connection().map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('dlp_action', ?1)",
        rusqlite::params![action],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

// Notification rate limiting helpers

pub fn get_last_notification_time() -> Option<u64> {
    let conn = match open_connection() {
        Ok(c) => c,
        Err(_) => return None,
    };

    conn.query_row(
        "SELECT value FROM settings WHERE key = 'last_notification_time'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse().ok())
}

pub fn set_last_notification_time(timestamp: u64) -> Result<(), String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('last_notification_time', ?1)",
        rusqlite::params![timestamp.to_string()],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
