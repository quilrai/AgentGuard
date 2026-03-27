// Stats and Monitoring Tauri Commands

use crate::database::{get_port_from_db, open_connection, save_port_to_db, DLP_ACTION_BLOCKED, DLP_ACTION_PASSED, DLP_ACTION_REDACTED, DLP_ACTION_RATELIMITED, DLP_ACTION_NOTIFY_RATELIMIT};
use crate::{PROXY_PORT, PROXY_STATUS, RESTART_SENDER, ProxyStatus};
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
             ORDER BY timestamp ASC"
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
            .push(TokenTimelinePoint { timestamp, input_tokens });
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
        _ => 1, // default to 1 hour
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
        format!(" AND COALESCE(model, 'unknown') = '{}'", model.replace('\'', "''"))
    };

    let dlp_filter = match dlp_action.as_str() {
        "passed" => format!(" AND COALESCE(dlp_action, 0) = {}", DLP_ACTION_PASSED),
        "redacted" => format!(" AND dlp_action = {}", DLP_ACTION_REDACTED),
        "blocked" => format!(" AND dlp_action = {}", DLP_ACTION_BLOCKED),
        "ratelimited" => format!(" AND dlp_action = {}", DLP_ACTION_RATELIMITED),
        "notify-ratelimit" => format!(" AND dlp_action = {}", DLP_ACTION_NOTIFY_RATELIMIT),
        _ => String::new(),
    };

    // Search filter - case-insensitive LIKE on request_body and response_body
    let search_filter = if search.trim().is_empty() {
        String::new()
    } else {
        let escaped_search = search.replace('\'', "''").replace('%', "\\%").replace('_', "\\_");
        format!(
            " AND (LOWER(request_body) LIKE LOWER('%{}%') ESCAPE '\\' OR LOWER(response_body) LIKE LOWER('%{}%') ESCAPE '\\')",
            escaped_search, escaped_search
        )
    };

    let filters = format!("{}{}{}{}", backend_filter, model_filter, dlp_filter, search_filter);

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

    let offset = page * 10;

    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, timestamp, backend, COALESCE(model, 'unknown'),
                    input_tokens, output_tokens, latency_ms, request_body, response_body,
                    request_headers, response_headers, COALESCE(dlp_action, 0),
                    COALESCE(tokens_saved, 0), token_saving_meta
             FROM requests
             WHERE timestamp >= ?1{}
             ORDER BY id DESC
             LIMIT 10 OFFSET ?2",
            filters
        ))
        .map_err(|e| e.to_string())?;

    let logs: Vec<MessageLog> = stmt
        .query_map(rusqlite::params![&cutoff_ts, offset], |row| {
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
        format!(" AND COALESCE(model, 'unknown') = '{}'", model.replace('\'', "''"))
    };

    let dlp_filter = match dlp_action.as_str() {
        "passed" => format!(" AND COALESCE(dlp_action, 0) = {}", DLP_ACTION_PASSED),
        "redacted" => format!(" AND dlp_action = {}", DLP_ACTION_REDACTED),
        "blocked" => format!(" AND dlp_action = {}", DLP_ACTION_BLOCKED),
        "ratelimited" => format!(" AND dlp_action = {}", DLP_ACTION_RATELIMITED),
        "notify-ratelimit" => format!(" AND dlp_action = {}", DLP_ACTION_NOTIFY_RATELIMIT),
        _ => String::new(),
    };

    let search_filter = if search.trim().is_empty() {
        String::new()
    } else {
        let escaped_search = search.replace('\'', "''").replace('%', "\\%").replace('_', "\\_");
        format!(
            " AND (LOWER(request_body) LIKE LOWER('%{}%') ESCAPE '\\' OR LOWER(response_body) LIKE LOWER('%{}%') ESCAPE '\\')",
            escaped_search, escaped_search
        )
    };

    let filters = format!("{}{}{}{}", backend_filter, model_filter, dlp_filter, search_filter);

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
pub struct ProxyStatusResponse {
    pub status: String,  // "starting", "running", "failed"
    pub port: u16,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_proxy_status() -> ProxyStatusResponse {
    let status = PROXY_STATUS.lock().unwrap();
    match &*status {
        ProxyStatus::Starting => ProxyStatusResponse {
            status: "starting".to_string(),
            port: *PROXY_PORT.lock().unwrap(),
            error: None,
        },
        ProxyStatus::Running(port) => ProxyStatusResponse {
            status: "running".to_string(),
            port: *port,
            error: None,
        },
        ProxyStatus::Failed(port, error) => ProxyStatusResponse {
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
    let mut current_port = PROXY_PORT.lock().unwrap();
    *current_port = port;

    Ok(())
}

#[tauri::command]
pub fn restart_proxy() -> Result<String, String> {
    let port = *PROXY_PORT.lock().unwrap();

    // Send restart signal
    let sender_guard = RESTART_SENDER.lock().unwrap();
    if let Some(sender) = sender_guard.as_ref() {
        sender.send(true).map_err(|e| e.to_string())?;
        Ok(format!("Proxy server restarting on port {}", port))
    } else {
        Err("Proxy server not initialized".to_string())
    }
}

// Get env var name and route for a given tool
fn get_tool_env_config(tool: &str) -> Result<(&'static str, &'static str), String> {
    match tool {
        "claude-code" => Ok(("ANTHROPIC_BASE_URL", "/claude")),
        "codex" => Ok(("OPENAI_BASE_URL", "/codex")),
        _ => Err(format!("Unknown tool: {}", tool)),
    }
}

// Find fish shell binary - macOS apps don't inherit shell PATH
fn find_fish_binary() -> Option<&'static str> {
    const FISH_PATHS: &[&str] = &[
        "/opt/homebrew/bin/fish",  // Homebrew on Apple Silicon
        "/usr/local/bin/fish",     // Homebrew on Intel Mac
        "/opt/local/bin/fish",     // MacPorts
        "/usr/bin/fish",           // System install
        "/bin/fish",               // Unlikely but check
    ];

    for path in FISH_PATHS {
        if std::path::Path::new(path).exists() {
            return Some(path);
        }
    }
    None
}

#[tauri::command]
pub fn set_shell_env(shell: String, tool: String) -> Result<String, String> {
    let port = *PROXY_PORT.lock().unwrap();
    let (env_var, route) = get_tool_env_config(&tool)?;
    let base_url = format!("http://localhost:{}{}", port, route);

    // Codex uses function wrapper instead of global env var
    if tool == "codex" {
        return set_codex_function(shell, &base_url);
    }

    match shell.as_str() {
        "fish" => {
            // Fish: use set -Ux for universal export (persists automatically)
            let manual_cmd = format!("set -Ux {} \"{}\"", env_var, base_url);

            let fish_path = match find_fish_binary() {
                Some(path) => path,
                None => return Err(format!(
                    "Automated env variable update failed. Please set manually in fish:\n{}",
                    manual_cmd
                )),
            };

            let output = std::process::Command::new(fish_path)
                .args(["-c", &manual_cmd])
                .output()
                .map_err(|_| format!(
                    "Automated env variable update failed. Please set manually in fish:\n{}",
                    manual_cmd
                ))?;

            if output.status.success() {
                Ok(format!("{} set globally for Fish", env_var))
            } else {
                Err(format!(
                    "Automated env variable update failed. Please set manually in fish:\n{}",
                    manual_cmd
                ))
            }
        }
        "bash" => {
            // Bash: append to ~/.bashrc
            let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
            let bashrc_path = format!("{}/.bashrc", home);
            let export_line = format!("export {}=\"{}\"", env_var, base_url);

            match update_shell_config(&bashrc_path, &export_line, env_var) {
                Ok(_) => Ok(format!("{} added to ~/.bashrc. Run 'source ~/.bashrc' or restart your terminal.", env_var)),
                Err(_) => Err(format!(
                    "Automated env variable update failed. Please add manually to ~/.bashrc:\n{}",
                    export_line
                )),
            }
        }
        "zsh" => {
            // Zsh: append to ~/.zshrc
            let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
            let zshrc_path = format!("{}/.zshrc", home);
            let export_line = format!("export {}=\"{}\"", env_var, base_url);

            match update_shell_config(&zshrc_path, &export_line, env_var) {
                Ok(_) => Ok(format!("{} added to ~/.zshrc. Run 'source ~/.zshrc' or restart your terminal.", env_var)),
                Err(_) => Err(format!(
                    "Automated env variable update failed. Please add manually to ~/.zshrc:\n{}",
                    export_line
                )),
            }
        }
        _ => Err(format!("Unknown shell: {}", shell)),
    }
}

// Set codex as a shell function (not global env var)
fn set_codex_function(shell: String, base_url: &str) -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;

    match shell.as_str() {
        "fish" => {
            // Fish: create function file
            let func_dir = format!("{}/.config/fish/functions", home);
            let func_path = format!("{}/codex.fish", func_dir);
            let func_content = format!(
                "function codex\n    set -lx OPENAI_BASE_URL \"{}\"\n    command codex $argv\nend\n",
                base_url
            );

            std::fs::create_dir_all(&func_dir)
                .map_err(|e| format!("Failed to create fish functions dir: {}", e))?;
            std::fs::write(&func_path, func_content)
                .map_err(|e| format!("Failed to write codex.fish: {}", e))?;

            Ok("codex function installed for Fish".to_string())
        }
        "bash" => {
            let rc_path = format!("{}/.bashrc", home);
            let func_lines = format!(
                "codex() {{\n    OPENAI_BASE_URL=\"{}\" command codex \"$@\"\n}}",
                base_url
            );
            update_codex_function(&rc_path, &func_lines)?;
            Ok("codex function added to ~/.bashrc. Run 'source ~/.bashrc' or restart your terminal.".to_string())
        }
        "zsh" => {
            let rc_path = format!("{}/.zshrc", home);
            let func_lines = format!(
                "codex() {{\n    OPENAI_BASE_URL=\"{}\" command codex \"$@\"\n}}",
                base_url
            );
            update_codex_function(&rc_path, &func_lines)?;
            Ok("codex function added to ~/.zshrc. Run 'source ~/.zshrc' or restart your terminal.".to_string())
        }
        _ => Err(format!("Unknown shell: {}", shell)),
    }
}

// Add/update codex function in shell config
fn update_codex_function(path: &str, func_lines: &str) -> Result<(), String> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    let content = fs::read_to_string(path).unwrap_or_default();
    let mut new_content = String::new();
    let mut in_func = false;
    let mut found = false;

    for line in content.lines() {
        if line.trim().starts_with("codex()") || line.trim() == "codex ()" {
            in_func = true;
            found = true;
            new_content.push_str("# LLMwatcher\n");
            new_content.push_str(func_lines);
            new_content.push('\n');
            continue;
        }
        if in_func {
            if line.trim() == "}" {
                in_func = false;
            }
            continue;
        }
        // Skip old LLMwatcher comment if it precedes codex function
        if line.trim() == "# LLMwatcher" {
            continue;
        }
        new_content.push_str(line);
        new_content.push('\n');
    }

    if !found {
        new_content.push_str("\n# LLMwatcher\n");
        new_content.push_str(func_lines);
        new_content.push('\n');
    }

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;

    file.write_all(new_content.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn check_shell_env(shell: String, tool: String) -> Result<bool, String> {
    let (env_var, _) = get_tool_env_config(&tool)?;

    // Codex uses function wrapper
    if tool == "codex" {
        return check_codex_function(shell);
    }

    match shell.as_str() {
        "fish" => {
            // Fish: check if universal variable is set
            let fish_path = find_fish_binary()
                .ok_or_else(|| "Fish shell not found. Please install fish or check your installation.".to_string())?;

            let output = std::process::Command::new(fish_path)
                .args(["-c", &format!("set -q {}; and echo 1; or echo 0", env_var)])
                .output()
                .map_err(|e| format!("Failed to run fish: {}", e))?;

            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(result == "1")
        }
        "bash" => {
            let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
            let bashrc_path = format!("{}/.bashrc", home);
            Ok(check_env_in_config(&bashrc_path, env_var))
        }
        "zsh" => {
            let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
            let zshrc_path = format!("{}/.zshrc", home);
            Ok(check_env_in_config(&zshrc_path, env_var))
        }
        _ => Err(format!("Unknown shell: {}", shell)),
    }
}

// Check if codex function exists
fn check_codex_function(shell: String) -> Result<bool, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;

    match shell.as_str() {
        "fish" => {
            let func_path = format!("{}/.config/fish/functions/codex.fish", home);
            Ok(std::path::Path::new(&func_path).exists())
        }
        "bash" => {
            let rc_path = format!("{}/.bashrc", home);
            Ok(check_codex_func_in_config(&rc_path))
        }
        "zsh" => {
            let rc_path = format!("{}/.zshrc", home);
            Ok(check_codex_func_in_config(&rc_path))
        }
        _ => Err(format!("Unknown shell: {}", shell)),
    }
}

fn check_codex_func_in_config(path: &str) -> bool {
    if let Ok(content) = std::fs::read_to_string(path) {
        content.lines().any(|line| {
            let trimmed = line.trim();
            (trimmed.starts_with("codex()") || trimmed == "codex ()") && !trimmed.starts_with('#')
        })
    } else {
        false
    }
}

// Check if env var is in config file
fn check_env_in_config(path: &str, env_var: &str) -> bool {
    if let Ok(content) = std::fs::read_to_string(path) {
        let prefix = format!("export {}=", env_var);
        content.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with(&prefix) && !trimmed.starts_with('#')
        })
    } else {
        false
    }
}

#[tauri::command]
pub fn remove_shell_env(shell: String, tool: String) -> Result<String, String> {
    let (env_var, _) = get_tool_env_config(&tool)?;

    // Codex uses function wrapper
    if tool == "codex" {
        return remove_codex_function(shell);
    }

    match shell.as_str() {
        "fish" => {
            // Fish: erase universal variable
            let manual_cmd = format!("set -Ue {}", env_var);

            let fish_path = match find_fish_binary() {
                Some(path) => path,
                None => return Err(format!(
                    "Automated env variable removal failed. Please remove manually in fish:\n{}",
                    manual_cmd
                )),
            };

            let output = std::process::Command::new(fish_path)
                .args(["-c", &manual_cmd])
                .output()
                .map_err(|_| format!(
                    "Automated env variable removal failed. Please remove manually in fish:\n{}",
                    manual_cmd
                ))?;

            if output.status.success() {
                Ok(format!("{} removed from Fish", env_var))
            } else {
                Err(format!(
                    "Automated env variable removal failed. Please remove manually in fish:\n{}",
                    manual_cmd
                ))
            }
        }
        "bash" => {
            let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
            let bashrc_path = format!("{}/.bashrc", home);
            match remove_env_from_config(&bashrc_path, env_var) {
                Ok(_) => Ok(format!("{} removed from ~/.bashrc. Restart your terminal.", env_var)),
                Err(_) => Err(format!(
                    "Automated env variable removal failed. Please remove 'export {}=...' from ~/.bashrc manually.",
                    env_var
                )),
            }
        }
        "zsh" => {
            let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
            let zshrc_path = format!("{}/.zshrc", home);
            match remove_env_from_config(&zshrc_path, env_var) {
                Ok(_) => Ok(format!("{} removed from ~/.zshrc. Restart your terminal.", env_var)),
                Err(_) => Err(format!(
                    "Automated env variable removal failed. Please remove 'export {}=...' from ~/.zshrc manually.",
                    env_var
                )),
            }
        }
        _ => Err(format!("Unknown shell: {}", shell)),
    }
}

// Remove codex function
fn remove_codex_function(shell: String) -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;

    match shell.as_str() {
        "fish" => {
            let func_path = format!("{}/.config/fish/functions/codex.fish", home);
            if std::path::Path::new(&func_path).exists() {
                std::fs::remove_file(&func_path)
                    .map_err(|e| format!("Failed to remove codex.fish: {}", e))?;
            }
            Ok("codex function removed from Fish".to_string())
        }
        "bash" => {
            let rc_path = format!("{}/.bashrc", home);
            remove_codex_func_from_config(&rc_path)?;
            Ok("codex function removed from ~/.bashrc. Restart your terminal.".to_string())
        }
        "zsh" => {
            let rc_path = format!("{}/.zshrc", home);
            remove_codex_func_from_config(&rc_path)?;
            Ok("codex function removed from ~/.zshrc. Restart your terminal.".to_string())
        }
        _ => Err(format!("Unknown shell: {}", shell)),
    }
}

fn remove_codex_func_from_config(path: &str) -> Result<(), String> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    let content = fs::read_to_string(path).unwrap_or_default();
    let mut new_content = String::new();
    let mut in_func = false;
    let mut skip_comment = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track LLMwatcher comment that might precede function
        if trimmed == "# LLMwatcher" {
            skip_comment = true;
            continue;
        }

        if trimmed.starts_with("codex()") || trimmed == "codex ()" {
            in_func = true;
            skip_comment = false;
            continue;
        }

        if in_func {
            if trimmed == "}" {
                in_func = false;
            }
            continue;
        }

        // If we skipped comment but next line wasn't the function, add it back
        if skip_comment {
            new_content.push_str("# LLMwatcher\n");
            skip_comment = false;
        }

        new_content.push_str(line);
        new_content.push('\n');
    }

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;

    file.write_all(new_content.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;

    Ok(())
}

// Remove env var from config file
fn remove_env_from_config(path: &str, env_var: &str) -> Result<(), String> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    let content = fs::read_to_string(path).unwrap_or_default();
    let mut new_lines: Vec<&str> = Vec::new();
    let mut prev_was_comment = false;
    let export_prefix = format!("export {}=", env_var);

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip the "# LLMwatcher" comment if followed by the env var
        if trimmed == "# LLMwatcher" {
            prev_was_comment = true;
            continue;
        }

        if trimmed.starts_with(&export_prefix) {
            prev_was_comment = false;
            continue;
        }

        // If previous was comment but this isn't the env var, add back the comment
        if prev_was_comment {
            new_lines.push("# LLMwatcher");
            prev_was_comment = false;
        }

        new_lines.push(line);
    }

    // Remove trailing empty lines
    while new_lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        new_lines.pop();
    }

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;

    for line in new_lines {
        writeln!(file, "{}", line).map_err(|e| format!("Failed to write: {}", e))?;
    }

    Ok(())
}

// Helper function to update shell config files
fn update_shell_config(path: &str, export_line: &str, env_var: &str) -> Result<(), String> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    // Read existing content
    let content = fs::read_to_string(path).unwrap_or_default();

    // Check if env var is already set
    let lines: Vec<&str> = content.lines().collect();
    let mut found = false;
    let mut new_lines: Vec<String> = Vec::new();

    for line in &lines {
        if line.contains(env_var) {
            // Replace existing line
            new_lines.push(export_line.to_string());
            found = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found {
        // Add new line at the end
        new_lines.push(String::new()); // Empty line before
        new_lines.push("# LLMwatcher".to_string());
        new_lines.push(export_line.to_string());
    }

    // Write back
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;

    for line in new_lines {
        writeln!(file, "{}", line).map_err(|e| format!("Failed to write: {}", e))?;
    }

    Ok(())
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
pub fn get_tool_call_stats(time_range: String, backend: String) -> Result<Vec<ToolCallStats>, String> {
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
            let path = json.get("file_path").or_else(|| json.get("notebook_path"))?.as_str()?;
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
        _ => None
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

    println!("[STATS] get_tool_call_insights: time_range={}, backend={}, cutoff_ts={}", time_range, backend, cutoff_ts);

    // Debug: check tool_calls table
    let tc_count: i64 = conn.query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0)).unwrap_or(0);
    println!("[STATS] Total tool_calls in DB: {}", tc_count);

    // Debug: check recent requests with tool calls
    if let Ok(mut debug_stmt) = conn.prepare(
        "SELECT r.id, r.timestamp, r.backend, tc.tool_name
         FROM tool_calls tc
         JOIN requests r ON tc.request_id = r.id
         ORDER BY r.id DESC LIMIT 5"
    ) {
        let debug_rows: Vec<(i64, String, String, String)> = debug_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for (id, ts, backend, tool) in debug_rows {
            println!("[STATS] Recent tool call: request_id={}, timestamp={}, backend={}, tool={}", id, ts, backend, tool);
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

    let mut calls_stmt = conn
        .prepare(&query)
        .map_err(|e| e.to_string())?;

    let calls: Vec<(String, String)> = calls_stmt
        .query_map([&cutoff_ts], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
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

            ToolWithTargets { tool_name, count, targets }
        })
        .collect();

    tools.sort_by(|a, b| b.count.cmp(&a.count));
    tools.truncate(8); // Top 8 tools

    Ok(ToolInsights { tools })
}

// ========================================================================
// Claude Code Settings Commands
// ========================================================================

/// Get the path to Claude Code settings.json
fn get_claude_settings_path() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    Ok(format!("{}/.claude/settings.json", home))
}

/// Read Claude Code settings.json, creating it if it doesn't exist
fn read_claude_settings() -> Result<serde_json::Value, String> {
    let path = get_claude_settings_path()?;

    if std::path::Path::new(&path).exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read settings.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))
    } else {
        // Return empty object if file doesn't exist
        Ok(serde_json::json!({}))
    }
}

/// Write Claude Code settings.json
fn write_claude_settings(settings: &serde_json::Value) -> Result<(), String> {
    let path = get_claude_settings_path()?;
    let home = std::env::var("HOME").map_err(|_| "Could not get HOME directory")?;
    let claude_dir = format!("{}/.claude", home);

    // Create .claude directory if it doesn't exist
    if !std::path::Path::new(&claude_dir).exists() {
        std::fs::create_dir_all(&claude_dir)
            .map_err(|e| format!("Failed to create .claude directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings.json: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn check_claude_code_settings() -> Result<bool, String> {
    let settings = read_claude_settings()?;

    // Check if env.ANTHROPIC_BASE_URL exists
    if let Some(env) = settings.get("env") {
        if let Some(base_url) = env.get("ANTHROPIC_BASE_URL") {
            return Ok(base_url.as_str().is_some());
        }
    }

    Ok(false)
}

#[tauri::command]
pub fn set_claude_code_settings() -> Result<String, String> {
    let port = *PROXY_PORT.lock().unwrap();
    let base_url = format!("http://localhost:{}/claude", port);

    let mut settings = read_claude_settings()?;

    // Ensure settings is an object
    let obj = settings.as_object_mut()
        .ok_or("settings.json is not a valid JSON object")?;

    // Get or create the "env" object
    if !obj.contains_key("env") {
        obj.insert("env".to_string(), serde_json::json!({}));
    }

    let env = obj.get_mut("env")
        .and_then(|v| v.as_object_mut())
        .ok_or("Failed to access env object")?;

    // Set ANTHROPIC_BASE_URL
    env.insert("ANTHROPIC_BASE_URL".to_string(), serde_json::json!(base_url));

    write_claude_settings(&settings)?;

    Ok(format!("ANTHROPIC_BASE_URL set in ~/.claude/settings.json"))
}

#[tauri::command]
pub fn remove_claude_code_settings() -> Result<String, String> {
    let mut settings = read_claude_settings()?;

    // Remove ANTHROPIC_BASE_URL from env if it exists
    if let Some(obj) = settings.as_object_mut() {
        if let Some(env) = obj.get_mut("env").and_then(|v| v.as_object_mut()) {
            env.remove("ANTHROPIC_BASE_URL");

            // If env is now empty, remove it entirely
            if env.is_empty() {
                obj.remove("env");
            }
        }
    }

    write_claude_settings(&settings)?;

    Ok(format!("ANTHROPIC_BASE_URL removed from ~/.claude/settings.json"))
}
