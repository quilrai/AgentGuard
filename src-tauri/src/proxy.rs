// HTTP Proxy Server and Handler

use crate::backends::custom::CustomBackendSettings;
use crate::backends::{Backend, ClaudeBackend, CodexBackend, CustomBackend};
use crate::cursor_hooks::create_cursor_hooks_router;
use crate::database::{get_dlp_action_from_db, get_last_notification_time, set_last_notification_time, Database, DLP_ACTION_BLOCKED, DLP_ACTION_PASSED, DLP_ACTION_REDACTED, DLP_ACTION_RATELIMITED, DLP_ACTION_NOTIFY_RATELIMIT};
use crate::dlp::{apply_dlp_redaction, apply_dlp_unredaction, DlpDetection};
use crate::dlp_pattern_config::get_db_path;
use crate::requestresponsemetadata::ResponseMetadata;
use crate::token_saving::apply_token_saving;
use crate::{PROXY_PORT, PROXY_STATUS, RESTART_SENDER, ProxyStatus};
use tauri::{AppHandle, Emitter};

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::Client;
use std::collections::HashMap;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::watch;

/// Rate limiter for tracking request counts per backend
#[derive(Clone, Default)]
pub struct RateLimiter {
    /// Map of backend_name -> list of request timestamps (in seconds since epoch)
    requests: Arc<Mutex<HashMap<String, Vec<u64>>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a request is allowed and record it if so
    /// Returns true if allowed, false if rate limited
    pub fn check_and_record(&self, backend_name: &str, max_requests: u32, window_minutes: u32) -> bool {
        if max_requests == 0 {
            return true; // No rate limit
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window_secs = (window_minutes as u64) * 60;
        let cutoff = now.saturating_sub(window_secs);

        let mut requests = self.requests.lock().unwrap();
        let timestamps = requests.entry(backend_name.to_string()).or_default();

        // Remove old timestamps outside the window
        timestamps.retain(|&ts| ts > cutoff);

        // Check if we're at the limit
        if timestamps.len() >= max_requests as usize {
            return false;
        }

        // Record this request
        timestamps.push(now);
        true
    }
}

/// Convert axum HeaderMap to JSON string
fn headers_to_json(headers: &HeaderMap) -> String {
    let map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

/// Convert reqwest HeaderMap to JSON string
fn reqwest_headers_to_json(headers: &reqwest::header::HeaderMap) -> String {
    let map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

/// Decompress gzip data
fn decompress_gzip(data: &[u8]) -> Option<String> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = String::new();
    match decoder.read_to_string(&mut decompressed) {
        Ok(_) => Some(decompressed),
        Err(_) => None,
    }
}

/// Format detection pattern names for error message
fn format_detection_patterns(detections: &[DlpDetection]) -> String {
    let mut pattern_names: Vec<&str> = detections
        .iter()
        .map(|d| d.pattern_name.as_str())
        .collect();
    pattern_names.sort();
    pattern_names.dedup();
    pattern_names.join(", ")
}

/// Estimate token count from text (words * 1.5)
fn estimate_tokens(text: &str) -> u32 {
    let word_count = text.split_whitespace().count();
    (word_count as f64 * 1.5).ceil() as u32
}

/// Create Claude API error response body
fn create_claude_error_response(pattern_names: &str) -> String {
    serde_json::json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": format!("Request blocked: sensitive data detected ({})", pattern_names)
        }
    })
    .to_string()
}

/// Create Codex/OpenAI API error response body
fn create_codex_error_response(pattern_names: &str) -> String {
    serde_json::json!({
        "error": {
            "message": format!("Request blocked: sensitive data detected ({})", pattern_names),
            "type": "invalid_request_error",
            "code": "content_policy_violation"
        }
    })
    .to_string()
}

#[derive(Clone)]
struct ProxyState {
    db: Database,
    backend: Arc<dyn Backend>,
    rate_limiter: RateLimiter,
    app_handle: AppHandle,
}

async fn health_handler() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"status":"healthy"}"#))
        .unwrap()
}

async fn proxy_handler(State(state): State<ProxyState>, req: Request) -> impl IntoResponse {
    let start_time = Instant::now();
    let client = Client::new();
    let backend = &state.backend;
    let db = &state.db;
    let rate_limiter = &state.rate_limiter;

    let method = req.method().clone();
    // When using nest("/claude", ...), axum automatically strips the prefix
    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let full_path = format!("{}{}", path, query);
    let headers = req.headers().clone();

    let target_url = format!("{}{}", backend.base_url(), full_path);

    // Read request body first (needed for logging rate-limited requests)
    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Failed to read request body"))
                .unwrap();
        }
    };

    let request_body_str = String::from_utf8_lossy(&body_bytes).to_string();
    let req_meta = backend.parse_request_metadata(&request_body_str);
    let request_headers_json = headers_to_json(&headers);
    let should_log = backend.should_log(&request_body_str);

    // Track if we should use notify-ratelimit status (token limit exceeded in notify mode)
    let mut notify_ratelimit = false;

    // Check rate limiting
    let (rate_requests, rate_minutes) = backend.get_rate_limit();
    if rate_requests > 0 && !rate_limiter.check_and_record(backend.name(), rate_requests, rate_minutes) {
        println!(
            "[PROXY] Rate limited request for backend '{}': {} requests per {} minute(s)",
            backend.name(), rate_requests, rate_minutes
        );
        let error_body = serde_json::json!({
            "error": {
                "message": format!("Rate limit exceeded: {} requests per {} minute(s)", rate_requests, rate_minutes),
                "type": "rate_limit_error",
                "code": "rate_limit_exceeded"
            }
        }).to_string();

        // Log the rate-limited request
        if should_log {
            let resp_meta = ResponseMetadata::default();
            let _ = db.log_request(
                backend.name(),
                &method.to_string(),
                &full_path,
                "Messages",
                &request_body_str,
                &error_body,
                429,
                false,
                0,
                &req_meta,
                &resp_meta,
                None,
                Some(&request_headers_json),
                None,
                DLP_ACTION_RATELIMITED,
                0,
                None,
            );
        }

        return Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Content-Type", "application/json")
            .header("Retry-After", (rate_minutes * 60).to_string())
            .body(Body::from(error_body))
            .unwrap();
    }

    // Check token limit (only for requests that should be logged, i.e., messages endpoints)
    let (max_tokens, token_action) = backend.get_max_tokens_limit();
    if max_tokens > 0 && should_log {
        let estimated_tokens = estimate_tokens(&request_body_str);
        if estimated_tokens > max_tokens {
            println!(
                "[PROXY] Token limit exceeded for backend '{}': {} tokens (limit: {}, action: {})",
                backend.name(), estimated_tokens, max_tokens, token_action
            );

            if token_action == "block" {
                let error_body = serde_json::json!({
                    "error": {
                        "message": format!("Token limit exceeded: {} tokens (limit: {})", estimated_tokens, max_tokens),
                        "type": "rate_limit_error",
                        "code": "token_limit_exceeded"
                    }
                }).to_string();

                // Log the token-limited request
                let resp_meta = ResponseMetadata::default();
                let _ = db.log_request(
                    backend.name(),
                    &method.to_string(),
                    &full_path,
                    "Messages",
                    &request_body_str,
                    &error_body,
                    429,
                    false,
                    0,
                    &req_meta,
                    &resp_meta,
                    None,
                    Some(&request_headers_json),
                    None,
                    DLP_ACTION_RATELIMITED,
                    0,
                    None,
                );

                return Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header("Content-Type", "application/json")
                    .body(Body::from(error_body))
                    .unwrap();
            } else {
                // Notify mode: allow request but flag for logging
                notify_ratelimit = true;

                // Send notification if more than 1 minute has passed since last notification
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let last_notification = get_last_notification_time().unwrap_or(0);
                if now - last_notification >= 60 {
                    let _ = set_last_notification_time(now);
                    let backend_name = backend.name().to_string();
                    let app_handle = state.app_handle.clone();
                    // Send notification in background to not block the request
                    tokio::spawn(async move {
                        use tauri_plugin_notification::NotificationExt;
                        let _ = app_handle
                            .notification()
                            .builder()
                            .title("LLMwatcher")
                            .body(format!("{} hitting rate limits", backend_name))
                            .show();
                    });
                }
            }
        }
    }

    // Check if DLP is enabled for this backend
    let dlp_enabled = backend.is_dlp_enabled();

    // Apply DLP redaction to request body (only if DLP is enabled)
    let dlp_result = if dlp_enabled {
        apply_dlp_redaction(&request_body_str)
    } else {
        // No DLP - pass through unchanged
        crate::dlp::DlpRedactionResult {
            redacted_body: request_body_str.clone(),
            replacements: HashMap::new(),
            detections: vec![],
        }
    };
    let redacted_body = dlp_result.redacted_body;
    let dlp_replacements = dlp_result.replacements;

    // Check if we should block (instead of redact) when DLP detections are found
    let dlp_action = get_dlp_action_from_db();
    if dlp_enabled && dlp_action == "block" && !dlp_result.detections.is_empty() {
        println!(
            "[PROXY] Blocking request due to DLP detections: {} patterns",
            dlp_result.detections.len()
        );

        let pattern_names = format_detection_patterns(&dlp_result.detections);
        let error_body = if backend.name() == "codex" {
            create_codex_error_response(&pattern_names)
        } else {
            create_claude_error_response(&pattern_names)
        };

        // Log the blocked request
        if backend.should_log(&request_body_str) {
            let request_headers_json = headers_to_json(&headers);
            let resp_meta = ResponseMetadata::default();

            if let Ok(request_id) = db.log_request(
                backend.name(),
                &method.to_string(),
                &full_path,
                "Messages",
                &request_body_str,
                &error_body,
                400,
                false,
                0,
                &req_meta,
                &resp_meta,
                None,
                Some(&request_headers_json),
                None,
                DLP_ACTION_BLOCKED,
                0,
                None,
            ) {
                let _ = db.log_dlp_detections(request_id, &dlp_result.detections);
            }
        }

        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Body::from(error_body))
            .unwrap();
    }

    // Apply token saving transformations (after DLP, before forwarding)
    let token_saving_settings = backend.get_token_saving_settings();
    let token_saving_result = apply_token_saving(&redacted_body, &token_saving_settings);
    let body_to_send = token_saving_result.body.clone();
    let tokens_saved = token_saving_result.total_tokens_saved;
    let token_saving_meta = token_saving_result.meta_json();

    // Use the post-token-saving body as canonical for logging and metadata
    let logged_request_body = body_to_send.clone();
    let req_meta = if tokens_saved > 0 {
        backend.parse_request_metadata(&logged_request_body)
    } else {
        req_meta
    };

    if tokens_saved > 0 {
        println!(
            "[PROXY] Token saving for backend '{}': {} tokens saved",
            backend.name(), tokens_saved
        );
    }

    let mut reqwest_req = match method.clone() {
        Method::GET => client.get(&target_url),
        Method::POST => client.post(&target_url),
        Method::PUT => client.put(&target_url),
        Method::DELETE => client.delete(&target_url),
        Method::PATCH => client.patch(&target_url),
        _ => client.request(method.clone(), &target_url),
    };

    // Skip headers that we need to recalculate or that shouldn't be forwarded
    let skip_request_headers = ["host", "content-length"];
    for (name, value) in headers.iter() {
        let header_lower = name.as_str().to_lowercase();
        if !skip_request_headers.contains(&header_lower.as_str()) {
            if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_ref()) {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
                {
                    reqwest_req = reqwest_req.header(header_name, header_value);
                }
            }
        }
    }

    // Use token-saving-transformed body for the request
    if !body_bytes.is_empty() {
        reqwest_req = reqwest_req.body(body_to_send.into_bytes());
    }

    let is_streaming = body_bytes
        .windows(13)
        .any(|w| w == b"\"stream\":true" || w == b"\"stream\": true");

    println!("[PROXY] Sending request to upstream: {}", target_url);
    let response = match reqwest_req.send().await {
        Ok(resp) => {
            println!("[PROXY] Got response from upstream: {}", resp.status());
            resp
        }
        Err(e) => {
            println!("[PROXY] Upstream error: {:?}", e);
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from(format!("Proxy error: {}", e)))
                .unwrap();
        }
    };

    let status = response.status();
    let resp_headers = response.headers().clone();

    let mut response_headers = HeaderMap::new();
    let skip_headers = ["content-encoding", "content-length", "transfer-encoding"];

    for (name, value) in resp_headers.iter() {
        if !skip_headers.contains(&name.as_str().to_lowercase().as_str()) {
            if let Ok(header_name) = axum::http::header::HeaderName::from_bytes(name.as_ref()) {
                if let Ok(header_value) = HeaderValue::from_bytes(value.as_bytes()) {
                    response_headers.insert(header_name, header_value);
                }
            }
        }
    }

    let method_str = method.to_string();
    let backend_name = backend.name().to_string();

    if is_streaming {
        response_headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        response_headers.insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        );

        let db_clone = db.clone();
        let backend_clone = state.backend.clone();
        let path_clone = full_path.clone();
        let req_body_clone = logged_request_body.clone();
        let status_code = status.as_u16();
        let req_meta_clone = req_meta.clone();
        let dlp_replacements_clone = dlp_replacements.clone();
        let dlp_detections_clone = dlp_result.detections.clone();
        let headers_clone = headers.clone();
        let request_headers_json = headers_to_json(&headers);
        let response_headers_json = reqwest_headers_to_json(&resp_headers);
        let notify_ratelimit_clone = notify_ratelimit;
        let tokens_saved_clone = tokens_saved;
        let token_saving_meta_clone = token_saving_meta.clone();

        let collected_chunks: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let chunks_for_stream = collected_chunks.clone();
        let dlp_for_stream = dlp_replacements.clone();

        println!("[PROXY] Starting streaming response...");
        let stream = response.bytes_stream().map(move |result| {
            match result {
                Ok(bytes) => {
                    let chunk_str = String::from_utf8_lossy(&bytes).to_string();
                    chunks_for_stream.lock().unwrap().push(chunk_str.clone());

                    // Apply DLP unredaction to each chunk
                    let unredacted_chunk = apply_dlp_unredaction(&chunk_str, &dlp_for_stream);
                    Ok(Bytes::from(unredacted_chunk))
                }
                Err(e) => {
                    println!("[PROXY] Stream error: {}", e);
                    Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                }
            }
        });

        let logged_stream = async_stream::stream! {
            let mut inner = std::pin::pin!(stream);
            while let Some(item) = inner.next().await {
                yield item;
            }

            let latency_ms = start_time.elapsed().as_millis() as u64;
            let response_body = collected_chunks.lock().unwrap().join("");
            let unredacted_response = apply_dlp_unredaction(&response_body, &dlp_replacements_clone);
            let resp_meta = backend_clone.parse_response_metadata(&unredacted_response, true);

            // Only log if backend says we should
            if backend_clone.should_log(&req_body_clone) {
                // Extract extra metadata
                let extra_meta = backend_clone.extract_extra_metadata(
                    &req_body_clone,
                    &unredacted_response,
                    &headers_clone,
                );

                // Determine dlp_action: notify-ratelimit if flagged and no DLP detections,
                // otherwise redacted if detections, otherwise passed
                let dlp_action_value = if notify_ratelimit_clone && dlp_detections_clone.is_empty() {
                    DLP_ACTION_NOTIFY_RATELIMIT
                } else if dlp_detections_clone.is_empty() {
                    DLP_ACTION_PASSED
                } else {
                    DLP_ACTION_REDACTED
                };

                if let Ok(request_id) = db_clone.log_request(
                    &backend_name,
                    &method_str,
                    &path_clone,
                    &path_clone,  // Use actual path as endpoint name
                    &req_body_clone,
                    &unredacted_response,
                    status_code,
                    true,
                    latency_ms,
                    &req_meta_clone,
                    &resp_meta,
                    extra_meta.as_deref(),
                    Some(&request_headers_json),
                    Some(&response_headers_json),
                    dlp_action_value,
                    tokens_saved_clone,
                    token_saving_meta_clone.as_deref(),
                ) {
                    // Log DLP detections if any
                    if !dlp_detections_clone.is_empty() {
                        let _ = db_clone.log_dlp_detections(request_id, &dlp_detections_clone);
                    }
                    // Log tool calls if any
                    if !resp_meta.tool_calls.is_empty() {
                        println!("[PROXY] Logging {} tool calls for request_id={}", resp_meta.tool_calls.len(), request_id);
                        match db_clone.log_tool_calls(request_id, &resp_meta.tool_calls) {
                            Ok(_) => println!("[PROXY] Tool calls logged successfully"),
                            Err(e) => println!("[PROXY] Failed to log tool calls: {}", e),
                        }
                    }
                }
            }
        };

        Response::builder()
            .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
            .body(Body::from_stream(logged_stream))
            .unwrap()
    } else {
        // Check if response is gzip encoded
        let is_gzip = resp_headers
            .get("content-encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("gzip"))
            .unwrap_or(false);

        let body = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("Failed to read response: {}", e)))
                    .unwrap();
            }
        };

        let latency_ms = start_time.elapsed().as_millis() as u64;

        // Decompress if gzip, otherwise use as-is
        let response_body_str = if is_gzip {
            decompress_gzip(&body).unwrap_or_else(|| String::from_utf8_lossy(&body).to_string())
        } else {
            String::from_utf8_lossy(&body).to_string()
        };

        // Apply DLP unredaction to response
        let unredacted_response = apply_dlp_unredaction(&response_body_str, &dlp_replacements);

        let resp_meta = backend.parse_response_metadata(&unredacted_response, false);

        // Only log if backend says we should
        if backend.should_log(&request_body_str) {
            // Extract extra metadata
            let extra_meta = backend.extract_extra_metadata(
                &logged_request_body,
                &unredacted_response,
                &headers,
            );

            // Convert headers to JSON
            let request_headers_json = headers_to_json(&headers);
            let response_headers_json = reqwest_headers_to_json(&resp_headers);

            // Determine dlp_action: notify-ratelimit if flagged and no DLP detections,
            // otherwise redacted if detections, otherwise passed
            let dlp_action_value = if notify_ratelimit && dlp_result.detections.is_empty() {
                DLP_ACTION_NOTIFY_RATELIMIT
            } else if dlp_result.detections.is_empty() {
                DLP_ACTION_PASSED
            } else {
                DLP_ACTION_REDACTED
            };

            if let Ok(request_id) = db.log_request(
                backend.name(),
                &method_str,
                &full_path,
                &full_path,  // Use actual path as endpoint name
                &logged_request_body,
                &unredacted_response,
                status.as_u16(),
                false,
                latency_ms,
                &req_meta,
                &resp_meta,
                extra_meta.as_deref(),
                Some(&request_headers_json),
                Some(&response_headers_json),
                dlp_action_value,
                tokens_saved,
                token_saving_meta.as_deref(),
            ) {
                // Log DLP detections if any
                if !dlp_result.detections.is_empty() {
                    let _ = db.log_dlp_detections(request_id, &dlp_result.detections);
                }
                // Log tool calls if any
                if !resp_meta.tool_calls.is_empty() {
                    let _ = db.log_tool_calls(request_id, &resp_meta.tool_calls);
                }
            }
        }

        let mut resp = Response::builder()
            .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK));

        for (name, value) in response_headers.iter() {
            resp = resp.header(name, value);
        }

        // Return unredacted response body
        resp.body(Body::from(unredacted_response.into_bytes()))
            .unwrap()
    }
}

pub async fn start_proxy_server(app_handle: AppHandle) {
    loop {
        // Get current port
        let port = *PROXY_PORT.lock().unwrap();

        // Set status to starting
        {
            let mut status = PROXY_STATUS.lock().unwrap();
            *status = ProxyStatus::Starting;
        }

        let db_path = get_db_path();
        let db = Database::new(db_path).expect("Failed to initialize database");
        println!("Database initialized: {}", db_path);

        // Clean up data older than 7 days on startup
        match db.cleanup_old_data() {
            Ok(deleted) => {
                if deleted > 0 {
                    println!("Cleaned up {} old records (>7 days)", deleted);
                }
            }
            Err(e) => eprintln!("Failed to cleanup old data: {}", e),
        }

        // Spawn background compression worker
        // Runs every 5 minutes, compresses in short bursts to avoid blocking live requests
        {
            let db_for_compression = db.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
                interval.tick().await; // Skip immediate first tick
                loop {
                    interval.tick().await;
                    let db = db_for_compression.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        db.run_compression_maintenance()
                    }).await;

                    if let Ok(Ok(did_compress)) = result {
                        if did_compress {
                            println!("[DB] Background compression maintenance completed");
                        }
                    }
                }
            });
        }

        // Create shared rate limiter
        let rate_limiter = RateLimiter::new();

        // Load predefined backend settings
        let claude_settings = db
            .get_predefined_backend_settings("claude")
            .unwrap_or_else(|_| "{}".to_string());
        let codex_settings = db
            .get_predefined_backend_settings("codex")
            .unwrap_or_else(|_| "{}".to_string());

        // Create backends with settings
        let claude_backend: Arc<dyn Backend> = Arc::new(ClaudeBackend::with_settings(&claude_settings));
        let codex_backend: Arc<dyn Backend> = Arc::new(CodexBackend::with_settings(&codex_settings));

        // Log predefined backend settings
        let (claude_rate_requests, claude_rate_minutes) = claude_backend.get_rate_limit();
        let (codex_rate_requests, codex_rate_minutes) = codex_backend.get_rate_limit();
        if claude_rate_requests > 0 {
            println!(
                "[PROXY] Claude backend: rate limit {} requests per {} minute(s), DLP: {}",
                claude_rate_requests, claude_rate_minutes,
                if claude_backend.is_dlp_enabled() { "enabled" } else { "disabled" }
            );
        }
        if codex_rate_requests > 0 {
            println!(
                "[PROXY] Codex backend: rate limit {} requests per {} minute(s), DLP: {}",
                codex_rate_requests, codex_rate_minutes,
                if codex_backend.is_dlp_enabled() { "enabled" } else { "disabled" }
            );
        }

        // Create states for each backend
        let claude_state = ProxyState {
            db: db.clone(),
            backend: claude_backend,
            rate_limiter: rate_limiter.clone(),
            app_handle: app_handle.clone(),
        };
        let codex_state = ProxyState {
            db: db.clone(),
            backend: codex_backend,
            rate_limiter: rate_limiter.clone(),
            app_handle: app_handle.clone(),
        };

        // Create routers for each backend
        let claude_router = Router::new()
            .fallback(proxy_handler)
            .with_state(claude_state);
        let codex_router = Router::new()
            .fallback(proxy_handler)
            .with_state(codex_state);

        // Load cursor-hooks settings and create router
        let cursor_hooks_settings_json = db
            .get_predefined_backend_settings("cursor-hooks")
            .unwrap_or_else(|_| "{}".to_string());
        let cursor_hooks_settings: CustomBackendSettings = serde_json::from_str(&cursor_hooks_settings_json)
            .unwrap_or_default();

        // Log cursor-hooks settings
        if cursor_hooks_settings.rate_limit_requests > 0 {
            println!(
                "[PROXY] Cursor-hooks: rate limit {} requests per {} minute(s), DLP: {}",
                cursor_hooks_settings.rate_limit_requests,
                cursor_hooks_settings.rate_limit_minutes.max(1),
                if cursor_hooks_settings.dlp_enabled { "enabled" } else { "disabled" }
            );
        }

        let cursor_hooks_router = create_cursor_hooks_router(
            db.clone(),
            rate_limiter.clone(),
            cursor_hooks_settings,
        );

        // Build base app with builtin backends
        let mut app = Router::new()
            .route("/", get(health_handler))
            .nest("/claude", claude_router)
            .nest("/codex", codex_router)
            .nest("/cursor_hook", cursor_hooks_router);

        // Load and add custom backends
        let custom_backends = Database::new(&get_db_path())
            .ok()
            .and_then(|db| db.get_enabled_custom_backends().ok())
            .unwrap_or_default();
        for backend_record in custom_backends {
            let custom_backend: Arc<dyn Backend> = Arc::new(CustomBackend::new(
                backend_record.name.clone(),
                backend_record.base_url.clone(),
                &backend_record.settings,
            ));

            // Log rate limit and DLP status
            let (rate_requests, rate_minutes) = custom_backend.get_rate_limit();
            let dlp_status = if custom_backend.is_dlp_enabled() { "enabled" } else { "disabled" };

            if rate_requests > 0 {
                println!(
                    "[PROXY] Custom backend '{}': rate limit {} requests per {} minute(s)",
                    backend_record.name, rate_requests, rate_minutes
                );
            }

            let route_path = format!("/{}", backend_record.name);
            println!(
                "[PROXY] Registering custom backend: {} -> {} (DLP: {})",
                route_path,
                backend_record.base_url,
                dlp_status
            );

            let custom_state = ProxyState {
                db: db.clone(),
                backend: custom_backend,
                rate_limiter: rate_limiter.clone(),
                app_handle: app_handle.clone(),
            };
            let custom_router = Router::new()
                .fallback(proxy_handler)
                .with_state(custom_state);

            app = app.nest(&route_path, custom_router);
        }

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind to port {}: {}", port, e);
                // Set status to failed
                {
                    let mut status = PROXY_STATUS.lock().unwrap();
                    *status = ProxyStatus::Failed(port, format!("{}", e));
                }
                // Emit failure event to frontend
                let _ = app_handle.emit("proxy-failed", serde_json::json!({
                    "port": port,
                    "error": format!("{}", e)
                }));
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        };
        println!("Proxy server running on http://0.0.0.0:{}", port);
        // Set status to running
        {
            let mut status = PROXY_STATUS.lock().unwrap();
            *status = ProxyStatus::Running(port);
        }
        // Emit success event to frontend
        let _ = app_handle.emit("proxy-started", serde_json::json!({
            "port": port
        }));

        // Create shutdown channel
        let (tx, mut rx) = watch::channel(false);
        {
            let mut sender = RESTART_SENDER.lock().unwrap();
            *sender = Some(tx);
        }

        // Run server with graceful shutdown
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            loop {
                rx.changed().await.ok();
                if *rx.borrow() {
                    println!("Received restart signal, shutting down proxy server...");
                    break;
                }
            }
        });

        if let Err(e) = server.await {
            eprintln!("Proxy server error: {}", e);
        }

        println!("Proxy server stopped, restarting with new configuration...");
        // Small delay before restart
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}
