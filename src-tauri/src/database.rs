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

/// DLP action: Request was blocked due to token limit
pub const DLP_ACTION_RATELIMITED: i32 = 3;

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

        // Drop old logs if compressed schema is stale (must happen before VACUUM)
        Self::drop_stale_logs_if_needed(&conn);

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

    /// If the compressed requests table exists but lacks new columns, drop all logs.
    /// Settings and DLP patterns are preserved.
    fn drop_stale_logs_if_needed(conn: &Connection) {
        // First check if _requests_zstd table exists at all (compression was previously enabled)
        let has_compressed: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_requests_zstd'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_compressed {
            return; // Fresh DB or uncompressed — nothing to migrate
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

        println!("[DB] Schema outdated — dropping old logs...");
        let _ = conn.execute_batch("
            DROP TABLE IF EXISTS dlp_detections;
            DROP TABLE IF EXISTS tool_calls;
            DROP VIEW IF EXISTS requests;
            DROP TABLE IF EXISTS _requests_zstd;
            DROP TABLE IF EXISTS _requests_zstd_dicts;
            DROP TABLE IF EXISTS _requests_zstd_configs;
        ");
        let _ = conn.execute("DELETE FROM settings WHERE key = 'tool_calls_backfill_done'", []);
        println!("[DB] Done. Logs will be recreated.");
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
                    "", // placeholder column kept for backwards compat; no longer populated
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

        // Use the underlying table for the update (the `requests` view goes
        // through an INSTEAD OF trigger which doesn't honor UPDATEs the way
        // we need for the running, uncompressed tail).
        let rows_affected = conn.execute(
            "UPDATE _requests_zstd SET
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

