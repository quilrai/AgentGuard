// Stats and Monitoring Tauri Commands

use crate::database::{
    get_port_from_db, open_connection, save_port_to_db, DLP_ACTION_BLOCKED, DLP_ACTION_PASSED,
    DLP_ACTION_REDACTED,
};
use crate::{ServerStatus, RESTART_SENDER, SERVER_PORT, SERVER_STATUS};
use serde::Serialize;

// ========================================================================
// Tray Menu Stats (Last 24h per backend)
// ========================================================================

#[derive(Serialize, Clone)]
pub struct BackendStats {
    pub backend: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_tokens: i64, // cache_read + cache_creation combined
}

#[derive(Serialize)]
pub struct TrayStats {
    pub backends: Vec<BackendStats>,
}

// Timeline point for input tokens chart in tray popup
#[derive(Serialize, Clone)]
pub struct TokenTimelinePoint {
    pub timestamp: String,
    pub input_tokens: i64,
}

#[derive(Serialize, Clone)]
pub struct BackendTimeline {
    pub backend: String,
    pub points: Vec<TokenTimelinePoint>,
}

#[derive(Serialize)]
pub struct TrayTokenTimeline {
    pub backends: Vec<BackendTimeline>,
}

#[tauri::command]
pub fn get_tray_stats() -> Result<TrayStats, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    // Last 24 hours
    let cutoff_ts = get_cutoff_timestamp(24);

    let mut stmt = conn
        .prepare(
            "SELECT backend,
                    COUNT(*) as request_count,
                    COALESCE(SUM(input_tokens), 0) as input_tokens,
                    COALESCE(SUM(output_tokens), 0) as output_tokens,
                    COALESCE(SUM(cache_read_tokens), 0) + COALESCE(SUM(cache_creation_tokens), 0) as cache_tokens
             FROM requests
             WHERE timestamp >= ?1
             GROUP BY backend
             ORDER BY request_count DESC"
        )
        .map_err(|e| e.to_string())?;

    let backends: Vec<BackendStats> = stmt
        .query_map([&cutoff_ts], |row| {
            Ok(BackendStats {
                backend: row.get(0)?,
                request_count: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_tokens: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(TrayStats { backends })
}

#[tauri::command]
pub fn get_tray_token_timeline() -> Result<TrayTokenTimeline, String> {
    use std::collections::HashMap;

    let conn = open_connection().map_err(|e| e.to_string())?;

    // Last 24 hours
    let cutoff_ts = get_cutoff_timestamp(24);

    let mut stmt = conn
        .prepare(
            "SELECT backend, timestamp, input_tokens
             FROM requests
             WHERE timestamp >= ?1 AND input_tokens > 0
             ORDER BY timestamp ASC",
        )
        .map_err(|e| e.to_string())?;

    // Group points by backend
    let mut backend_points: HashMap<String, Vec<TokenTimelinePoint>> = HashMap::new();

    let rows = stmt
        .query_map([&cutoff_ts], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows.filter_map(|r| r.ok()) {
        let (backend, timestamp, input_tokens) = row;
        backend_points
            .entry(backend)
            .or_default()
            .push(TokenTimelinePoint {
                timestamp,
                input_tokens,
            });
    }

    // Convert to sorted vec
    let mut backends: Vec<BackendTimeline> = backend_points
        .into_iter()
        .map(|(backend, points)| BackendTimeline { backend, points })
        .collect();

    // Sort by backend name for consistent ordering
    backends.sort_by(|a, b| a.backend.cmp(&b.backend));

    Ok(TrayTokenTimeline { backends })
}

#[derive(Serialize)]
pub struct ModelStats {
    model: String,
    count: i64,
}

#[derive(Serialize)]
pub struct FeatureStats {
    with_system_prompt: i64,
    with_tools: i64,
    with_thinking: i64,
    total_requests: i64,
}

#[derive(Serialize)]
pub struct TokenTotals {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
}

#[derive(Serialize)]
pub struct RecentRequest {
    id: i64,
    timestamp: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    latency_ms: i64,
    stop_reason: String,
    has_thinking: bool,
}

#[derive(Serialize)]
pub struct MessageLog {
    id: i64,
    timestamp: String,
    backend: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    latency_ms: i64,
    request_body: Option<String>,
    response_body: Option<String>,
    request_headers: Option<String>,
    response_headers: Option<String>,
    dlp_action: i64, // DLP_ACTION_PASSED=0, DLP_ACTION_REDACTED=1, DLP_ACTION_BLOCKED=2
    tokens_saved: i64,
    token_saving_meta: Option<String>,
}

#[derive(Serialize)]
pub struct PaginatedLogs {
    logs: Vec<MessageLog>,
    total: i64,
}

#[derive(Serialize)]
pub struct LatencyPoint {
    id: i64,
    latency_ms: i64,
}

#[derive(Serialize)]
pub struct DashboardData {
    models: Vec<ModelStats>,
    features: FeatureStats,
    token_totals: TokenTotals,
    recent_requests: Vec<RecentRequest>,
    latency_points: Vec<LatencyPoint>,
    total_requests: i64,
    avg_latency_ms: f64,
}

// Convert time range string to hours
fn time_range_to_hours(time_range: &str) -> i64 {
    match time_range {
        "1h" => 1,
        "6h" => 6,
        "1d" => 24,
        "7d" => 24 * 7,
        "all" => 24 * 365 * 10, // ~10 years, effectively all time
        _ => 24 * 365 * 10,     // default to all time
    }
}

// Get timestamp for time range filter
fn get_cutoff_timestamp(hours: i64) -> String {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);
    cutoff.to_rfc3339()
}

#[tauri::command]
pub fn get_dashboard_stats(time_range: String, backend: String) -> Result<DashboardData, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    // Build backend filter clause
    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND backend = '{}'", backend.replace('\'', "''"))
    };

    // Get model stats
    let mut model_stmt = conn
        .prepare(&format!(
            "SELECT COALESCE(model, 'unknown') as model, COUNT(*) as count
             FROM requests
             WHERE model IS NOT NULL AND timestamp >= ?1{}
             GROUP BY model
             ORDER BY count DESC",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let models: Vec<ModelStats> = model_stmt
        .query_map([&cutoff_ts], |row| {
            Ok(ModelStats {
                model: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Get feature stats
    let features: FeatureStats = conn
        .query_row(
            &format!(
                "SELECT
                    COALESCE(SUM(has_system_prompt), 0),
                    COALESCE(SUM(has_tools), 0),
                    COALESCE(SUM(has_thinking), 0),
                    COUNT(*)
                 FROM requests
                 WHERE timestamp >= ?1{}",
                backend_filter
            ),
            [&cutoff_ts],
            |row| {
                Ok(FeatureStats {
                    with_system_prompt: row.get(0)?,
                    with_tools: row.get(1)?,
                    with_thinking: row.get(2)?,
                    total_requests: row.get(3)?,
                })
            },
        )
        .unwrap_or(FeatureStats {
            with_system_prompt: 0,
            with_tools: 0,
            with_thinking: 0,
            total_requests: 0,
        });

    // Get token totals
    let token_totals: TokenTotals = conn
        .query_row(
            &format!(
                "SELECT
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cache_read_tokens), 0),
                    COALESCE(SUM(cache_creation_tokens), 0)
                 FROM requests
                 WHERE timestamp >= ?1{}",
                backend_filter
            ),
            [&cutoff_ts],
            |row| {
                Ok(TokenTotals {
                    input: row.get(0)?,
                    output: row.get(1)?,
                    cache_read: row.get(2)?,
                    cache_creation: row.get(3)?,
                })
            },
        )
        .unwrap_or(TokenTotals {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_creation: 0,
        });

    // Get recent requests for token chart
    let mut recent_stmt = conn
        .prepare(&format!(
            "SELECT id, timestamp, COALESCE(model, 'unknown'), input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, latency_ms,
                    COALESCE(stop_reason, 'unknown'), has_thinking
             FROM requests
             WHERE timestamp >= ?1{}
             ORDER BY id DESC",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let recent_requests: Vec<RecentRequest> = recent_stmt
        .query_map([&cutoff_ts], |row| {
            Ok(RecentRequest {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                model: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                cache_creation_tokens: row.get(6)?,
                latency_ms: row.get(7)?,
                stop_reason: row.get(8)?,
                has_thinking: row.get::<_, i32>(9)? == 1,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Get latency points for chart
    let mut latency_stmt = conn
        .prepare(&format!(
            "SELECT id, latency_ms
             FROM requests
             WHERE latency_ms > 0 AND timestamp >= ?1{}
             ORDER BY id DESC",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let latency_points: Vec<LatencyPoint> = latency_stmt
        .query_map([&cutoff_ts], |row| {
            Ok(LatencyPoint {
                id: row.get(0)?,
                latency_ms: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Get totals
    let total_requests: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM requests WHERE timestamp >= ?1{}",
                backend_filter
            ),
            [&cutoff_ts],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let avg_latency_ms: f64 = conn
        .query_row(
            &format!(
                "SELECT COALESCE(AVG(latency_ms), 0)
                 FROM requests
                 WHERE latency_ms > 0 AND timestamp >= ?1{}",
                backend_filter
            ),
            [&cutoff_ts],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    Ok(DashboardData {
        models,
        features,
        token_totals,
        recent_requests,
        latency_points,
        total_requests,
        avg_latency_ms,
    })
}

#[tauri::command]
pub fn get_backends() -> Result<Vec<String>, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT DISTINCT backend FROM requests ORDER BY backend")
        .map_err(|e| e.to_string())?;

    let backends: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(backends)
}

#[tauri::command]
pub fn get_models() -> Result<Vec<String>, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT DISTINCT COALESCE(model, 'unknown') FROM requests ORDER BY model")
        .map_err(|e| e.to_string())?;

    let models: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(models)
}

#[tauri::command]
pub fn get_message_logs(
    time_range: String,
    backend: String,
    model: String,
    dlp_action: String,
    search: String,
    page: i64,
    #[allow(non_snake_case)] pageSize: Option<i64>,
    view: Option<String>,
) -> Result<PaginatedLogs, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND backend = '{}'", backend.replace('\'', "''"))
    };

    let model_filter = if model == "all" {
        String::new()
    } else {
        format!(
            " AND COALESCE(model, 'unknown') = '{}'",
            model.replace('\'', "''")
        )
    };

    let dlp_filter = match dlp_action.as_str() {
        "passed" => format!(" AND COALESCE(dlp_action, 0) = {}", DLP_ACTION_PASSED),
        "redacted" => format!(" AND dlp_action = {}", DLP_ACTION_REDACTED),
        "blocked" => format!(" AND dlp_action = {}", DLP_ACTION_BLOCKED),
        _ => String::new(),
    };

    // View filter: token_saving shows only rows with savings, guardian shows only rows with actions
    let view_filter = match view.as_deref() {
        Some("token_saving") => " AND COALESCE(tokens_saved, 0) > 0".to_string(),
        Some("guardian") => " AND COALESCE(dlp_action, 0) > 0".to_string(),
        _ => String::new(),
    };

    // Search filter - case-insensitive LIKE on request_body and response_body
    let search_filter = if search.trim().is_empty() {
        String::new()
    } else {
        let escaped_search = search
            .replace('\'', "''")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!(
            " AND (LOWER(request_body) LIKE LOWER('%{}%') ESCAPE '\\' OR LOWER(response_body) LIKE LOWER('%{}%') ESCAPE '\\')",
            escaped_search, escaped_search
        )
    };

    let filters = format!(
        "{}{}{}{}{}",
        backend_filter, model_filter, dlp_filter, view_filter, search_filter
    );

    // Get total count
    let total: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM requests WHERE timestamp >= ?1{}",
                filters
            ),
            [&cutoff_ts],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let per_page = pageSize.unwrap_or(10).min(10000);
    let offset = page * per_page;

    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, timestamp, backend, COALESCE(model, 'unknown'),
                    input_tokens, output_tokens, latency_ms, request_body, response_body,
                    request_headers, response_headers, COALESCE(dlp_action, 0),
                    COALESCE(tokens_saved, 0), token_saving_meta
             FROM requests
             WHERE timestamp >= ?1{}
             ORDER BY id DESC
             LIMIT ?2 OFFSET ?3",
            filters
        ))
        .map_err(|e| e.to_string())?;

    let logs: Vec<MessageLog> = stmt
        .query_map(rusqlite::params![&cutoff_ts, per_page, offset], |row| {
            Ok(MessageLog {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                backend: row.get(2)?,
                model: row.get(3)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                latency_ms: row.get(6)?,
                request_body: row.get(7)?,
                response_body: row.get(8)?,
                request_headers: row.get(9)?,
                response_headers: row.get(10)?,
                dlp_action: row.get(11)?,
                tokens_saved: row.get(12)?,
                token_saving_meta: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(PaginatedLogs { logs, total })
}

#[derive(Serialize)]
pub struct ExportLog {
    pub id: i64,
    pub timestamp: String,
    pub backend: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: i64,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub dlp_action: i64,
    pub tokens_saved: i64,
    pub token_saving_meta: Option<String>,
}

#[tauri::command]
pub fn export_message_logs(
    time_range: String,
    backend: String,
    model: String,
    dlp_action: String,
    search: String,
) -> Result<Vec<ExportLog>, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND backend = '{}'", backend.replace('\'', "''"))
    };

    let model_filter = if model == "all" {
        String::new()
    } else {
        format!(
            " AND COALESCE(model, 'unknown') = '{}'",
            model.replace('\'', "''")
        )
    };

    let dlp_filter = match dlp_action.as_str() {
        "passed" => format!(" AND COALESCE(dlp_action, 0) = {}", DLP_ACTION_PASSED),
        "redacted" => format!(" AND dlp_action = {}", DLP_ACTION_REDACTED),
        "blocked" => format!(" AND dlp_action = {}", DLP_ACTION_BLOCKED),
        _ => String::new(),
    };

    let search_filter = if search.trim().is_empty() {
        String::new()
    } else {
        let escaped_search = search
            .replace('\'', "''")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!(
            " AND (LOWER(request_body) LIKE LOWER('%{}%') ESCAPE '\\' OR LOWER(response_body) LIKE LOWER('%{}%') ESCAPE '\\')",
            escaped_search, escaped_search
        )
    };

    let filters = format!(
        "{}{}{}{}",
        backend_filter, model_filter, dlp_filter, search_filter
    );

    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, timestamp, backend, COALESCE(model, 'unknown'),
                    input_tokens, output_tokens, latency_ms, request_body, response_body,
                    COALESCE(dlp_action, 0), COALESCE(tokens_saved, 0), token_saving_meta
             FROM requests
             WHERE timestamp >= ?1{}
             ORDER BY id DESC",
            filters
        ))
        .map_err(|e| e.to_string())?;

    let logs: Vec<ExportLog> = stmt
        .query_map([&cutoff_ts], |row| {
            Ok(ExportLog {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                backend: row.get(2)?,
                model: row.get(3)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                latency_ms: row.get(6)?,
                request_body: row.get(7)?,
                response_body: row.get(8)?,
                dlp_action: row.get(9)?,
                tokens_saved: row.get(10)?,
                token_saving_meta: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(logs)
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub fn get_port_setting() -> u16 {
    get_port_from_db()
}

#[derive(Serialize)]
pub struct ServerStatusResponse {
    pub status: String, // "starting", "running", "failed"
    pub port: u16,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_server_status() -> ServerStatusResponse {
    let status = SERVER_STATUS.lock().unwrap();
    match &*status {
        ServerStatus::Starting => ServerStatusResponse {
            status: "starting".to_string(),
            port: *SERVER_PORT.lock().unwrap(),
            error: None,
        },
        ServerStatus::Running(port) => ServerStatusResponse {
            status: "running".to_string(),
            port: *port,
            error: None,
        },
        ServerStatus::Failed(port, error) => ServerStatusResponse {
            status: "failed".to_string(),
            port: *port,
            error: Some(error.clone()),
        },
    }
}

#[tauri::command]
pub fn save_port_setting(port: u16) -> Result<(), String> {
    // Validate port range
    if !(1024..=65535).contains(&port) {
        return Err("Port must be between 1024 and 65535".to_string());
    }

    // Save to database
    save_port_to_db(port)?;

    // Update global state
    let mut current_port = SERVER_PORT.lock().unwrap();
    *current_port = port;

    Ok(())
}

#[tauri::command]
pub fn restart_server() -> Result<String, String> {
    let port = *SERVER_PORT.lock().unwrap();

    // Send restart signal
    let sender_guard = RESTART_SENDER.lock().unwrap();
    if let Some(sender) = sender_guard.as_ref() {
        sender.send(true).map_err(|e| e.to_string())?;
        Ok(format!("Server restarting on port {}", port))
    } else {
        Err("Server not initialized".to_string())
    }
}
// ========================================================================
// Tool Call Commands
// ========================================================================

#[derive(Serialize)]
pub struct ToolCallRecord {
    pub id: i64,
    pub request_id: i64,
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_input: String,
}

#[derive(Serialize)]
pub struct ToolCallStats {
    pub tool_name: String,
    pub count: i64,
}

#[tauri::command]
pub fn get_tool_calls_for_request(request_id: i64) -> Result<Vec<ToolCallRecord>, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, request_id, tool_call_id, tool_name, tool_input
             FROM tool_calls WHERE request_id = ?1 ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;

    let tool_calls: Vec<ToolCallRecord> = stmt
        .query_map([request_id], |row| {
            Ok(ToolCallRecord {
                id: row.get(0)?,
                request_id: row.get(1)?,
                tool_call_id: row.get(2)?,
                tool_name: row.get(3)?,
                tool_input: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tool_calls)
}

#[tauri::command]
pub fn get_tool_call_stats(
    time_range: String,
    backend: String,
) -> Result<Vec<ToolCallStats>, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    // Build backend filter clause
    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND r.backend = '{}'", backend.replace('\'', "''"))
    };

    let mut stmt = conn
        .prepare(&format!(
            "SELECT tc.tool_name, COUNT(*) as count
             FROM tool_calls tc
             JOIN requests r ON tc.request_id = r.id
             WHERE r.timestamp >= ?1{}
             GROUP BY tc.tool_name
             ORDER BY count DESC
             LIMIT 20",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let stats: Vec<ToolCallStats> = stmt
        .query_map([&cutoff_ts], |row| {
            Ok(ToolCallStats {
                tool_name: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(stats)
}

#[derive(Serialize)]
pub struct ToolTargetStats {
    pub target: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct ToolWithTargets {
    pub tool_name: String,
    pub count: i64,
    pub targets: Vec<ToolTargetStats>,
}

#[derive(Serialize)]
pub struct ToolInsights {
    pub tools: Vec<ToolWithTargets>,
}

/// Extract target from tool input JSON
fn extract_target(tool_name: &str, tool_input: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(tool_input).ok()?;

    match tool_name {
        // File-based tools: extract filename from path
        "Read" | "Write" | "Edit" | "NotebookEdit" => {
            let path = json
                .get("file_path")
                .or_else(|| json.get("notebook_path"))?
                .as_str()?;
            Some(path.rsplit('/').next()?.to_string())
        }
        "Glob" | "Grep" => {
            // For Glob/Grep, use path if available, otherwise pattern
            if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                Some(path.rsplit('/').next()?.to_string())
            } else if let Some(pattern) = json.get("pattern").and_then(|v| v.as_str()) {
                Some(pattern.chars().take(20).collect())
            } else {
                None
            }
        }
        // Bash: extract first word of command
        "Bash" => {
            let cmd = json.get("command")?.as_str()?;
            let first_word = cmd.trim().split_whitespace().next()?;
            let clean = first_word.trim_start_matches("sudo ");
            Some(clean.split('/').last()?.to_string())
        }
        _ => None,
    }
}

#[tauri::command]
pub fn get_tool_call_insights(time_range: String, backend: String) -> Result<ToolInsights, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND r.backend = '{}'", backend.replace('\'', "''"))
    };

    println!(
        "[STATS] get_tool_call_insights: time_range={}, backend={}, cutoff_ts={}",
        time_range, backend, cutoff_ts
    );

    // Debug: check tool_calls table
    let tc_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
        .unwrap_or(0);
    println!("[STATS] Total tool_calls in DB: {}", tc_count);

    // Debug: check recent requests with tool calls
    if let Ok(mut debug_stmt) = conn.prepare(
        "SELECT r.id, r.timestamp, r.backend, tc.tool_name
         FROM tool_calls tc
         JOIN requests r ON tc.request_id = r.id
         ORDER BY r.id DESC LIMIT 5",
    ) {
        let debug_rows: Vec<(i64, String, String, String)> = debug_stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for (id, ts, backend, tool) in debug_rows {
            println!(
                "[STATS] Recent tool call: request_id={}, timestamp={}, backend={}, tool={}",
                id, ts, backend, tool
            );
        }
    }

    // Get raw tool calls
    let query = format!(
        "SELECT tc.tool_name, tc.tool_input
         FROM tool_calls tc
         JOIN requests r ON tc.request_id = r.id
         WHERE r.timestamp >= ?1{}",
        backend_filter
    );
    println!("[STATS] Query: {}", query);

    let mut calls_stmt = conn.prepare(&query).map_err(|e| e.to_string())?;

    let calls: Vec<(String, String)> = calls_stmt
        .query_map([&cutoff_ts], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    println!("[STATS] Found {} tool calls", calls.len());

    // Count tools and targets
    use std::collections::HashMap;
    let mut tool_counts: HashMap<String, i64> = HashMap::new();
    let mut target_counts: HashMap<String, HashMap<String, i64>> = HashMap::new();

    for (tool_name, tool_input) in calls {
        *tool_counts.entry(tool_name.clone()).or_insert(0) += 1;

        if let Some(target) = extract_target(&tool_name, &tool_input) {
            *target_counts
                .entry(tool_name)
                .or_default()
                .entry(target)
                .or_insert(0) += 1;
        }
    }

    // Build sorted tools with their top targets
    let mut tools: Vec<ToolWithTargets> = tool_counts
        .into_iter()
        .map(|(tool_name, count)| {
            let mut targets: Vec<ToolTargetStats> = target_counts
                .remove(&tool_name)
                .unwrap_or_default()
                .into_iter()
                .map(|(target, count)| ToolTargetStats { target, count })
                .collect();

            targets.sort_by(|a, b| b.count.cmp(&a.count));
            targets.truncate(5); // Top 5 targets per tool

            ToolWithTargets {
                tool_name,
                count,
                targets,
            }
        })
        .collect();

    tools.sort_by(|a, b| b.count.cmp(&a.count));
    tools.truncate(8); // Top 8 tools

    Ok(ToolInsights { tools })
}
// ========================================================================
// Token Savings Stats (dashboard chart)
// ========================================================================

#[derive(Serialize)]
pub struct TokenSavingsByMethod {
    pub method: String,
    pub tokens_saved: i64,
}

#[derive(Serialize)]
pub struct TokenSavingsStats {
    pub total_saved: i64,
    pub by_method: Vec<TokenSavingsByMethod>,
}

#[tauri::command]
pub fn get_token_savings_stats(time_range: String, backend: String) -> Result<TokenSavingsStats, String> {
    use std::collections::HashMap;

    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    let backend_filter = if backend == "all" {
        String::new()
    } else {
        format!(" AND backend = '{}'", backend.replace('\'', "''"))
    };

    // Fetch rows with token savings and their meta
    let mut stmt = conn
        .prepare(&format!(
            "SELECT COALESCE(tokens_saved, 0), token_saving_meta
             FROM requests
             WHERE tokens_saved > 0 AND timestamp >= ?1{}",
            backend_filter
        ))
        .map_err(|e| e.to_string())?;

    let rows: Vec<(i64, Option<String>)> = stmt
        .query_map([&cutoff_ts], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Group by method from token_saving_meta JSON keys
    let mut method_totals: HashMap<String, i64> = HashMap::new();
    let mut total_saved: i64 = 0;

    for (saved, meta) in &rows {
        total_saved += saved;
        let method = meta
            .as_deref()
            .and_then(|m| {
                // meta is JSON like {"shell_compression":123} or {"ctx_read":456}
                serde_json::from_str::<serde_json::Value>(m)
                    .ok()
                    .and_then(|v| v.as_object()?.keys().next().cloned())
            })
            .unwrap_or_else(|| "unknown".to_string());
        *method_totals.entry(method).or_insert(0) += saved;
    }

    let mut by_method: Vec<TokenSavingsByMethod> = method_totals
        .into_iter()
        .map(|(method, tokens_saved)| TokenSavingsByMethod { method, tokens_saved })
        .collect();
    by_method.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved));

    Ok(TokenSavingsStats {
        total_saved,
        by_method,
    })
}

// ========================================================================
// Home Screen — facts pool + last-request-by-backend + token-saver stats
// ========================================================================

#[derive(Serialize)]
pub struct LastRequestEntry {
    pub backend: String,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct TopToolEntry {
    pub tool_name: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct HomeFacts {
    pub requests_last_hour: i64,
    pub requests_last_day: i64,
    pub total_requests: i64,
    pub last_request_by_backend: Vec<LastRequestEntry>,
    pub top_model_week: Option<String>,
    pub detections_week: i64,
    pub tokens_saved_today: i64,
    pub avg_latency_ms_day: f64,
    pub cache_hit_pct_day: f64,
    pub top_tool_week: Option<TopToolEntry>,
    pub enabled_pattern_count: i64,
}

#[tauri::command]
pub fn get_home_facts() -> Result<HomeFacts, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hour_cutoff = get_cutoff_timestamp(1);
    let day_cutoff = get_cutoff_timestamp(24);
    let week_cutoff = get_cutoff_timestamp(24 * 7);

    let requests_last_hour: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE timestamp >= ?1",
            [&hour_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let requests_last_day: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE timestamp >= ?1",
            [&day_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let total_requests: i64 = conn
        .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
        .unwrap_or(0);

    let last_request_by_backend: Vec<LastRequestEntry> = {
        let mut stmt = conn
            .prepare("SELECT backend, MAX(timestamp) FROM requests GROUP BY backend")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LastRequestEntry {
                    backend: row.get(0)?,
                    timestamp: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let top_model_week: Option<String> = conn
        .query_row(
            "SELECT model FROM requests
             WHERE timestamp >= ?1 AND model IS NOT NULL
             GROUP BY model
             ORDER BY COUNT(*) DESC LIMIT 1",
            [&week_cutoff],
            |row| row.get(0),
        )
        .ok();

    let detections_week: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM dlp_detections WHERE timestamp >= ?1",
            [&week_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let tokens_saved_today: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(tokens_saved), 0) FROM requests WHERE timestamp >= ?1",
            [&day_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let avg_latency_ms_day: f64 = conn
        .query_row(
            "SELECT COALESCE(AVG(latency_ms), 0) FROM requests
             WHERE latency_ms > 0 AND timestamp >= ?1",
            [&day_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    let day_total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE timestamp >= ?1",
            [&day_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let day_with_cache: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE timestamp >= ?1 AND cache_read_tokens > 0",
            [&day_cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let cache_hit_pct_day: f64 = if day_total > 0 {
        (day_with_cache as f64 / day_total as f64) * 100.0
    } else {
        0.0
    };

    let top_tool_week: Option<TopToolEntry> = conn
        .query_row(
            "SELECT tc.tool_name, COUNT(*) as cnt
             FROM tool_calls tc
             JOIN requests r ON tc.request_id = r.id
             WHERE r.timestamp >= ?1
             GROUP BY tc.tool_name
             ORDER BY cnt DESC
             LIMIT 1",
            [&week_cutoff],
            |row| {
                Ok(TopToolEntry {
                    tool_name: row.get(0)?,
                    count: row.get(1)?,
                })
            },
        )
        .ok();

    let enabled_pattern_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM dlp_patterns WHERE enabled = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(HomeFacts {
        requests_last_hour,
        requests_last_day,
        total_requests,
        last_request_by_backend,
        top_model_week,
        detections_week,
        tokens_saved_today,
        avg_latency_ms_day,
        cache_hit_pct_day,
        top_tool_week,
        enabled_pattern_count,
    })
}

// ========================================================================
// Garden Stats
// ========================================================================

/// Project summary for the garden picker.
#[derive(Serialize, Clone)]
pub struct GardenProject {
    pub cwd: String,
    pub display_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub request_count: i64,
    pub last_active: String,
    pub backends: Vec<String>,
}

#[derive(Serialize)]
pub struct GardenProjectList {
    pub projects: Vec<GardenProject>,
}

/// List all projects (for the garden picker).
#[tauri::command]
pub fn get_garden_stats(time_range: String) -> Result<GardenProjectList, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;

    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    // input_tokens here is the TOTAL context sent to the model
    // (uncached input + cache reads + cache creation), since the warnings
    // in the UI are based on this number.
    let mut stmt = conn
        .prepare(
            "SELECT
                json_extract(extra_metadata, '$.cwd') as cwd,
                COALESCE(SUM(input_tokens + cache_read_tokens + cache_creation_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_creation_tokens), 0),
                COUNT(*) as req_count,
                MAX(timestamp) as last_active,
                GROUP_CONCAT(DISTINCT backend) as backends
             FROM requests
             WHERE timestamp >= ?1
               AND extra_metadata IS NOT NULL
               AND json_extract(extra_metadata, '$.cwd') IS NOT NULL
             GROUP BY cwd
             ORDER BY req_count DESC",
        )
        .map_err(|e| e.to_string())?;

    let projects: Vec<GardenProject> = stmt
        .query_map([&cutoff_ts], |row| {
            let cwd: String = row.get(0)?;
            let display_name = cwd.rsplit('/').next().unwrap_or(&cwd).to_string();
            let backends_str: String = row.get::<_, String>(7)?;
            let backends: Vec<String> = backends_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            Ok(GardenProject {
                cwd,
                display_name,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                cache_read_tokens: row.get(3)?,
                cache_creation_tokens: row.get(4)?,
                request_count: row.get(5)?,
                last_active: row.get(6)?,
                backends,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(GardenProjectList { projects })
}

// ========================================================================
// Garden View — project-level file/module breakdown
// ========================================================================
//
// Each project is a garden. Files touched by the agent render as trees,
// grouped into groves by top-level module (first path segment). Sizes,
// touches, and per-backend breakdowns come out of this one command; the
// frontend takes it from there.

/// A single file that has been touched by an agent in this project.
#[derive(Serialize, Clone)]
pub struct GardenFile {
    /// Path, relative to project cwd if it lives inside.
    pub path: String,
    /// Line count of the file on disk (0 if the file no longer exists).
    pub lines: u64,
    /// Estimated token cost of the file's content (words * 1.5 — matches
    /// the hook-side heuristic). 0 if the file no longer exists.
    pub est_tokens: u64,
    /// Number of requests that touched this path in the time range.
    pub touch_count: u64,
    /// Most recent timestamp the file was touched.
    pub last_touched: String,
    /// Per-backend touch counts (e.g. [("claude-code", 12), ("codex", 3)]).
    pub backend_touches: Vec<(String, u64)>,
    /// False if the file no longer exists on disk — the frontend draws
    /// these as dead/leafless trees so stale references are visible.
    pub exists: bool,
}

/// Project-level detail for the Garden view.
#[derive(Serialize)]
pub struct GardenDetail {
    pub cwd: String,
    pub display_name: String,
    pub files: Vec<GardenFile>,
    // Project-wide aggregates.
    pub total_input: i64,
    pub total_output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub request_count: i64,
}

/// Tokenize a shell command into whitespace-separated pieces, keeping
/// single- and double-quoted strings together. Not a real shell parser —
/// just enough to pull out bare filename arguments from the kinds of
/// commands Codex agents run (sed/nl/cat/wc/python/grep/…).
fn tokenize_bash(cmd: &str) -> Vec<String> {
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

/// Does `s` end with something that looks like a file extension
/// (`.py`, `.rs`, `.md`, …)? Used to pick bare filenames that would
/// otherwise get skipped by the "contains `/`" heuristic.
fn has_file_extension(s: &str) -> bool {
    if let Some(dot) = s.rfind('.') {
        let ext = &s[dot + 1..];
        !ext.is_empty() && ext.len() <= 6 && ext.chars().all(|c| c.is_ascii_alphanumeric())
    } else {
        false
    }
}

/// Pull out tokens that resolve to real files under `cwd` from a shell
/// command string. Codex-style backends only log `Bash` tool calls, so
/// this is how we learn which files the agent actually touched.
///
/// The rules are deliberately strict (only paths that exist on disk
/// get through) so we don't hallucinate trees from random CLI arguments.
fn extract_paths_from_bash(cmd: &str, cwd: &std::path::Path) -> Vec<String> {
    use std::collections::HashSet;
    let mut out: HashSet<String> = HashSet::new();

    for raw in tokenize_bash(cmd) {
        // Strip matching surrounding quotes (the tokenizer keeps them).
        let trimmed = raw
            .trim_start_matches(|c| c == '\'' || c == '"')
            .trim_end_matches(|c| c == '\'' || c == '"');
        if trimmed.is_empty() {
            continue;
        }
        // Flags.
        if trimmed.starts_with('-') {
            continue;
        }
        // Shell operators.
        if matches!(
            trimmed,
            "|" | "||" | "&" | "&&" | ";" | ">" | ">>" | "<" | "2>" | "2>&1" | "&>" | "1>" | "1>&2"
        ) {
            continue;
        }
        // Anything with other shell metacharacters can't be a bare path.
        if trimmed
            .contains(|c: char| matches!(c, '|' | '&' | ';' | '<' | '>' | '`' | '$' | '*' | '?'))
        {
            continue;
        }
        // Has to look path-ish: either contains `/`, or ends with a
        // plausible extension. Skips command names, numeric args,
        // `foo:123` grep output refs, URLs (they contain `:` / `?`).
        let looks_like_path = trimmed.contains('/') || has_file_extension(trimmed);
        if !looks_like_path {
            continue;
        }

        // Resolve against cwd and verify it's a real file.
        let p = std::path::Path::new(trimmed);
        let abs = if p.is_absolute() {
            std::path::PathBuf::from(trimmed)
        } else {
            cwd.join(trimmed)
        };
        if abs.is_file() {
            // Insert the *original* token (possibly relative) — the
            // downstream code path resolves it against cwd again so
            // both forms work identically.
            out.insert(trimmed.to_string());
        }
    }
    out.into_iter().collect()
}

/// Project-level file breakdown for the Garden view.
#[tauri::command]
pub fn get_garden_detail(cwd: String, time_range: String) -> Result<GardenDetail, String> {
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    let conn = open_connection().map_err(|e| e.to_string())?;
    let hours = time_range_to_hours(&time_range);
    let cutoff_ts = get_cutoff_timestamp(hours);

    // Aggregate state per raw path: touches, per-backend touches, latest ts.
    struct Agg {
        touch_count: u64,
        backend_touches: HashMap<String, u64>,
        last_touched: String,
    }
    let mut touches: HashMap<String, Agg> = HashMap::new();

    let push =
        |raw_path: String, backend: String, ts: String, touches: &mut HashMap<String, Agg>| {
            let entry = touches.entry(raw_path).or_insert_with(|| Agg {
                touch_count: 0,
                backend_touches: HashMap::new(),
                last_touched: String::new(),
            });
            entry.touch_count += 1;
            *entry.backend_touches.entry(backend).or_insert(0) += 1;
            if ts > entry.last_touched {
                entry.last_touched = ts;
            }
        };

    // Source 1: Claude Code — file_path lives in requests.extra_metadata.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT json_extract(extra_metadata, '$.file_path') as fp,
                backend,
                timestamp
         FROM requests
         WHERE timestamp >= ?1
           AND extra_metadata IS NOT NULL
           AND json_extract(extra_metadata, '$.cwd') = ?2
           AND fp IS NOT NULL
           AND fp != ''",
    ) {
        let rows = stmt.query_map(rusqlite::params![&cutoff_ts, &cwd], |row| {
            let fp: String = row.get(0)?;
            let backend: String = row.get(1)?;
            let ts: String = row.get(2)?;
            Ok((fp, backend, ts))
        });
        if let Ok(rows) = rows {
            for (fp, backend, ts) in rows.filter_map(|r| r.ok()) {
                push(fp, backend, ts, &mut touches);
            }
        }
    }

    // Source 2: Codex / Cursor / anything that logs through tool_calls —
    // the path lives in tool_input JSON under file_path / path / notebook_path.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(
                    json_extract(t.tool_input, '$.file_path'),
                    json_extract(t.tool_input, '$.path'),
                    json_extract(t.tool_input, '$.notebook_path')
                ) as fp,
                r.backend,
                r.timestamp
         FROM tool_calls t
         JOIN requests r ON r.id = t.request_id
         WHERE r.timestamp >= ?1
           AND r.extra_metadata IS NOT NULL
           AND json_extract(r.extra_metadata, '$.cwd') = ?2
           AND fp IS NOT NULL
           AND fp != ''",
    ) {
        let rows = stmt.query_map(rusqlite::params![&cutoff_ts, &cwd], |row| {
            let fp: String = row.get(0)?;
            let backend: String = row.get(1)?;
            let ts: String = row.get(2)?;
            Ok((fp, backend, ts))
        });
        if let Ok(rows) = rows {
            for (fp, backend, ts) in rows.filter_map(|r| r.ok()) {
                push(fp, backend, ts, &mut touches);
            }
        }
    }

    // Source 3: Bash tool calls — Codex only logs Bash, and file paths
    // live inside the command string (`sed -n '1,240p' foo.py`, `cat bar.md`,
    // `nl -ba baz.rs`, `python -m qux run`, etc.). We tokenize the command
    // and accept any token that either contains a path separator or
    // resolves to an existing file under the project cwd — this keeps
    // noise low while catching the common cases.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT json_extract(t.tool_input, '$.command') as cmd,
                r.backend,
                r.timestamp
         FROM tool_calls t
         JOIN requests r ON r.id = t.request_id
         WHERE r.timestamp >= ?1
           AND r.extra_metadata IS NOT NULL
           AND json_extract(r.extra_metadata, '$.cwd') = ?2
           AND t.tool_name = 'Bash'
           AND cmd IS NOT NULL
           AND cmd != ''",
    ) {
        let rows = stmt.query_map(rusqlite::params![&cutoff_ts, &cwd], |row| {
            let cmd: String = row.get(0)?;
            let backend: String = row.get(1)?;
            let ts: String = row.get(2)?;
            Ok((cmd, backend, ts))
        });
        if let Ok(rows) = rows {
            let cwd_pb = std::path::PathBuf::from(&cwd);
            for (cmd, backend, ts) in rows.filter_map(|r| r.ok()) {
                for fp in extract_paths_from_bash(&cmd, &cwd_pb) {
                    push(fp, backend.clone(), ts.clone(), &mut touches);
                }
            }
        }
    }

    // Stat each touched path and build GardenFile records. Files that no
    // longer exist are kept (exists=false, lines=0, tokens=0) so the
    // frontend can render them as dead trees.
    let cwd_path = PathBuf::from(&cwd);
    let mut files: Vec<GardenFile> = Vec::with_capacity(touches.len());

    for (raw_path, agg) in touches {
        let abs_path = if Path::new(&raw_path).is_absolute() {
            PathBuf::from(&raw_path)
        } else {
            cwd_path.join(&raw_path)
        };

        let (lines, est_tokens, exists) = match fs::read_to_string(&abs_path) {
            Ok(content) => {
                let lines = content.lines().count() as u64;
                let words = content.split_whitespace().count() as u64;
                let est_tokens = (words as f64 * 1.5) as u64;
                (lines, est_tokens, true)
            }
            Err(_) => (0, 0, false),
        };

        // Skip directories / weird non-file paths entirely — they add noise
        // but produce no useful tree.
        if !exists && raw_path.ends_with('/') {
            continue;
        }

        let display_path = abs_path
            .strip_prefix(&cwd_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| abs_path.to_string_lossy().to_string());

        // Sort backend_touches descending so the frontend can just take [0].
        let mut backend_touches: Vec<(String, u64)> = agg.backend_touches.into_iter().collect();
        backend_touches.sort_by(|a, b| b.1.cmp(&a.1));

        files.push(GardenFile {
            path: display_path,
            lines,
            est_tokens,
            touch_count: agg.touch_count,
            last_touched: agg.last_touched,
            backend_touches,
            exists,
        });
    }

    // Rank by estimated "Claude work weight" — tokens * touches. This
    // surfaces the files that are both big AND hot, which is what a
    // developer cares about. Fall back to raw tokens, then touches.
    files.sort_by(|a, b| {
        let aw = a.est_tokens.saturating_mul(a.touch_count.max(1));
        let bw = b.est_tokens.saturating_mul(b.touch_count.max(1));
        bw.cmp(&aw)
            .then_with(|| b.est_tokens.cmp(&a.est_tokens))
            .then_with(|| b.touch_count.cmp(&a.touch_count))
    });
    files.truncate(200);

    // Project-wide aggregates.
    let (total_input, total_output, cache_read, cache_creation, request_count): (
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT
                COALESCE(SUM(input_tokens + cache_read_tokens + cache_creation_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_creation_tokens), 0),
                COUNT(*)
             FROM requests
             WHERE timestamp >= ?1
               AND extra_metadata IS NOT NULL
               AND json_extract(extra_metadata, '$.cwd') = ?2",
            rusqlite::params![&cutoff_ts, &cwd],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap_or((0, 0, 0, 0, 0));

    let display_name = cwd.rsplit('/').next().unwrap_or(&cwd).to_string();

    Ok(GardenDetail {
        cwd,
        display_name,
        files,
        total_input,
        total_output,
        cache_read,
        cache_creation,
        request_count,
    })
}

// ============================================================================
// Garden symbols — tree-sitter extracted imports & definitions
// ============================================================================

#[derive(Serialize)]
pub struct FileSymbolsResponse {
    pub file_path: String,
    pub symbols: Vec<crate::symbols::ExtractedSymbol>,
}

#[derive(Serialize)]
pub struct ImportEdge {
    /// File that contains the import statement.
    pub from_file: String,
    /// Import source string (e.g. "react", "../utils", "std::collections").
    pub import_source: String,
    /// Resolved target file within the project (if we can match it), or empty.
    pub to_file: String,
}

#[derive(Serialize)]
pub struct ImportGraphResponse {
    pub cwd: String,
    pub edges: Vec<ImportEdge>,
}

/// Get symbols for a single file in a project.
#[tauri::command]
pub fn get_file_symbols(cwd: String, file_path: String) -> Result<FileSymbolsResponse, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;
    let db = crate::database::Database::from_connection(conn);

    let symbols = db
        .get_file_symbols(&cwd, &file_path)
        .map_err(|e| e.to_string())?;

    Ok(FileSymbolsResponse { file_path, symbols })
}

/// Get the import graph for a project: which files import from where.
/// Tries to resolve import sources to files within the project.
#[tauri::command]
pub fn get_import_graph(cwd: String) -> Result<ImportGraphResponse, String> {
    let conn = open_connection().map_err(|e| e.to_string())?;
    let db = crate::database::Database::from_connection(conn);

    let all = db.get_project_symbols(&cwd).map_err(|e| e.to_string())?;

    // Collect all known file paths in the project for resolution.
    let known_files: std::collections::HashSet<String> =
        all.iter().map(|(fp, _)| fp.clone()).collect();

    let mut edges = Vec::new();

    for (file_path, sym) in &all {
        if sym.kind != "import" {
            continue;
        }
        let src = match &sym.source {
            Some(s) if !s.is_empty() => s.clone(),
            _ => sym.name.clone(),
        };

        // Try to resolve to a project file.
        let to_file = resolve_import(&src, file_path, &known_files);

        edges.push(ImportEdge {
            from_file: file_path.clone(),
            import_source: src,
            to_file,
        });
    }

    // Deduplicate by (from_file, import_source).
    edges.sort_by(|a, b| (&a.from_file, &a.import_source).cmp(&(&b.from_file, &b.import_source)));
    edges.dedup_by(|a, b| a.from_file == b.from_file && a.import_source == b.import_source);

    Ok(ImportGraphResponse { cwd, edges })
}

/// Best-effort resolution of an import source to a known project file.
fn resolve_import(
    import_src: &str,
    from_file: &str,
    known_files: &std::collections::HashSet<String>,
) -> String {
    // Relative imports: "./foo", "../bar/baz"
    if import_src.starts_with('.') {
        let from_dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        // Try common extensions.
        for ext in &[
            "",
            ".js",
            ".ts",
            ".tsx",
            ".jsx",
            ".py",
            ".rs",
            "/index.js",
            "/index.ts",
            "/mod.rs",
        ] {
            let candidate = format!(
                "{}/{}{}",
                from_dir,
                import_src.trim_start_matches("./"),
                ext
            );
            // Normalize ../
            let normalized = normalize_path(&candidate);
            if known_files.contains(&normalized) {
                return normalized;
            }
        }
    }

    // Bare imports: check if any known file's path ends with the import source.
    // e.g. "commands/stats" might match "src-tauri/src/commands/stats.rs"
    let cleaned = import_src.replace("::", "/");
    for ext in &["", ".js", ".ts", ".py", ".rs", ".go", ".java", ".rb", ".cs"] {
        let suffix = format!("{}{}", cleaned, ext);
        for f in known_files {
            if f.ends_with(&suffix) {
                return f.clone();
            }
        }
    }

    String::new()
}

/// Normalize a path by resolving `../` segments.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

// ========================================================================
// Browse directory from disk (for grove panel)
// ========================================================================

/// A file entry from disk with its line count.
#[derive(Serialize, Clone)]
pub struct DiskFileEntry {
    /// Path relative to the project cwd.
    pub path: String,
    /// Line count (0 for non-text / unreadable files).
    pub lines: u64,
}

/// Response for browsing a sub-directory on disk.
#[derive(Serialize)]
pub struct BrowseDirResponse {
    pub dir: String,
    pub files: Vec<DiskFileEntry>,
}

/// Text/code file extensions we count lines for.
fn is_text_ext(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "cs"
            | "rb"
            | "swift"
            | "kt"
            | "scala"
            | "lua"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "pl"
            | "pm"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "csv"
            | "md"
            | "txt"
            | "rst"
            | "ini"
            | "cfg"
            | "conf"
            | "sql"
            | "graphql"
            | "gql"
            | "proto"
            | "vue"
            | "svelte"
            | "astro"
            | "dockerfile"
            | "makefile"
            | "cmake"
            | "r"
            | "jl"
            | "ex"
            | "exs"
            | "erl"
            | "hrl"
            | "zig"
            | "nim"
            | "dart"
            | "v"
            | "vhdl"
    )
}

/// Directories to always skip.
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | ".git"
            | ".svn"
            | ".hg"
            | "target"
            | "build"
            | "dist"
            | ".next"
            | ".nuxt"
            | "__pycache__"
            | ".tox"
            | ".venv"
            | "venv"
            | ".idea"
            | ".vscode"
            | ".cache"
            | "vendor"
    )
}

/// Browse a directory on disk, returning all text/code files with line counts.
/// `dir` is relative to `cwd` (e.g. "src" or "src/commands"). Pass empty string for project root.
#[tauri::command]
pub fn browse_directory(cwd: String, dir: String) -> Result<BrowseDirResponse, String> {
    use std::fs;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    let base = Path::new(&cwd);
    let abs_dir = if dir.is_empty() {
        base.to_path_buf()
    } else {
        base.join(&dir)
    };

    if !abs_dir.is_dir() {
        return Err(format!("Not a directory: {}", abs_dir.display()));
    }

    let mut files = Vec::new();
    let mut stack = vec![abs_dir.clone()];

    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            if path.is_dir() {
                if !is_ignored_dir(&name.to_lowercase()) && !name.starts_with('.') {
                    stack.push(path);
                }
                continue;
            }

            // Check extension.
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            // Also accept extensionless files with known names.
            let fname_lower = name.to_lowercase();
            let is_text = is_text_ext(&ext)
                || matches!(
                    fname_lower.as_str(),
                    "makefile" | "dockerfile" | "rakefile" | "gemfile" | "procfile"
                );

            if !is_text {
                continue;
            }

            // Count lines.
            let lines = match fs::File::open(&path) {
                Ok(f) => {
                    let reader = BufReader::new(f);
                    reader.lines().count() as u64
                }
                Err(_) => 0,
            };

            // Make path relative to cwd.
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            files.push(DiskFileEntry { path: rel, lines });
        }
    }

    // Sort by lines descending.
    files.sort_by(|a, b| b.lines.cmp(&a.lines));

    Ok(BrowseDirResponse { dir, files })
}
