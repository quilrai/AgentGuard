// Local HTTP server for hook receivers and shell-compression endpoint.
//
// The server hosts:
//   - /cursor_hook/*    Cursor IDE hook receivers (cursor_hooks.rs)
//   - /claude_hook/*    Claude Code hook receivers (claude_hooks.rs)
//   - /codex_hook/*     Codex CLI hook receivers (codex_hooks.rs)
//   - /cli_compression  Shell-output compression endpoint used by the
//                       token-saver hook scripts installed for
//                       Claude Code, Cursor, and Codex.
//   - /                 Health check
//
// Earlier versions of this app also passthrough-proxied LLM API requests
// for Claude/Codex/custom backends. That layer was removed; everything is
// now driven by hooks installed in the agent CLIs/IDEs.

use crate::claude_hooks::create_claude_hooks_router;
use crate::codex_hooks::create_codex_hooks_router;
use crate::cursor_hooks::create_cursor_hooks_router;
use crate::database::Database;
use crate::dlp_pattern_config::get_db_path;
use crate::predefined_backend_settings::CustomBackendSettings;
use crate::requestresponsemetadata::ResponseMetadata;
use crate::shell_compression;
use crate::{ServerStatus, RESTART_SENDER, SERVER_PORT, SERVER_STATUS};
use tauri::{AppHandle, Emitter};

use axum::{
    body::{Body, Bytes},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::watch;

async fn health_handler() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"status":"healthy"}"#))
        .unwrap()
}

// ============================================================================
// Shell Output Compression Endpoint
// ============================================================================

async fn cli_compression_handler(body: Bytes) -> impl IntoResponse {
    // Parse request body
    let body_str = String::from_utf8_lossy(&body).to_string();
    let parsed: serde_json::Value = match serde_json::from_str(&body_str) {
        Ok(v) => v,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "text/plain")
                .body(Body::from(format!("Invalid JSON: {e}")))
                .unwrap();
        }
    };

    let command = match parsed.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "text/plain")
                .body(Body::from("Missing 'command' field"))
                .unwrap();
        }
    };

    let cwd = parsed.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let backend_name = parsed.get("backend").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Look up per-backend shell_compression setting (predefined backends only)
    let compression_enabled = if !backend_name.is_empty() {
        let settings_json = if let Ok(db) = Database::new(&get_db_path()) {
            db.get_predefined_backend_settings(&backend_name)
                .ok()
                .unwrap_or_else(|| "{}".to_string())
        } else {
            "{}".to_string()
        };
        let settings: CustomBackendSettings = serde_json::from_str(&settings_json).unwrap_or_default();
        settings.token_saving.shell_compression
    } else {
        // No backend specified — default to disabled
        false
    };

    // Run in blocking task since shell execution is synchronous
    let result = tokio::task::spawn_blocking(move || {
        if compression_enabled {
            shell_compression::compress_command(&command, cwd.as_deref())
        } else {
            shell_compression::run_command_raw(&command, cwd.as_deref())
        }
    })
    .await;

    let result = match result {
        Ok(r) => r,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "text/plain")
                .body(Body::from(format!("Execution failed: {e}")))
                .unwrap();
        }
    };

    // Append exit code info for non-zero exits, then re-account for the suffix
    // tokens so headers and DB logs match the bytes the agent actually sees.
    // We add the suffix tokens to BOTH original and compressed counts: the
    // server adds [exit: N] to the response regardless of whether compression
    // ran, so the conceptual baseline ("what would have been returned without
    // compression") also carries the suffix. tokens_saved therefore stays
    // anchored to the actual compression delta.
    let mut output = result.output;
    let mut original_tokens = result.original_tokens;
    let mut compressed_tokens = result.compressed_tokens;
    if result.exit_code != 0 {
        let mut suffix = String::new();
        if !output.is_empty() && !output.ends_with('\n') {
            suffix.push('\n');
        }
        suffix.push_str(&format!("[exit: {}]", result.exit_code));
        let suffix_tokens = shell_compression::tokens::count_tokens(&suffix);
        output.push_str(&suffix);
        original_tokens += suffix_tokens;
        compressed_tokens += suffix_tokens;
    }
    let tokens_saved: i32 = if original_tokens > compressed_tokens {
        (original_tokens - compressed_tokens) as i32
    } else {
        0
    };

    // Log to database
    if let Ok(db) = Database::new(&get_db_path()) {
        let req_meta = crate::requestresponsemetadata::RequestMetadata {
            model: None,
            has_system_prompt: false,
            has_tools: false,
            user_message_count: 0,
            assistant_message_count: 0,
        };
        let resp_meta = ResponseMetadata {
            input_tokens: original_tokens as i32,
            output_tokens: compressed_tokens as i32,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            stop_reason: Some(format!("exit:{}", result.exit_code)),
            has_thinking: false,
            tool_calls: vec![],
        };
        let meta_json = if tokens_saved > 0 {
            Some(format!("{{\"shell_compression\":{}}}", tokens_saved))
        } else {
            None
        };
        let log_backend = if backend_name.is_empty() {
            "shell_compression"
        } else {
            &backend_name
        };
        let _ = db.log_request(
            log_backend,
            "POST",
            "/cli_compression",
            "/cli_compression",
            &body_str,
            &output,
            200,
            false,
            result.duration_ms,
            &req_meta,
            &resp_meta,
            None,
            None,
            None,
            0, // dlp_action = passed
            tokens_saved,
            meta_json.as_deref(),
        );
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain")
        .header("X-Exit-Code", result.exit_code.to_string())
        .header("X-Original-Tokens", original_tokens.to_string())
        .header("X-Compressed-Tokens", compressed_tokens.to_string())
        .header("X-Tokens-Saved", tokens_saved.to_string())
        .body(Body::from(output))
        .unwrap()
}

// ============================================================================
// Server bootstrap
// ============================================================================

pub async fn start_server(app_handle: AppHandle) {
    loop {
        // Get current port
        let port = *SERVER_PORT.lock().unwrap();

        // Set status to starting
        {
            let mut status = SERVER_STATUS.lock().unwrap();
            *status = ServerStatus::Starting;
        }

        let db_path = get_db_path();
        let db = match Database::new(db_path) {
            Ok(db) => db,
            Err(e) => {
                eprintln!("[SERVER] Failed to initialize database: {}", e);
                let mut status = SERVER_STATUS.lock().unwrap();
                *status = ServerStatus::Failed(port, format!("Database error: {}", e));
                let _ = app_handle.emit(
                    "server-failed",
                    serde_json::json!({
                        "port": port,
                        "error": format!("Database initialization failed: {}", e)
                    }),
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        };
        println!("Database initialized: {}", db_path);

        // Load cursor-hooks settings and create router
        let cursor_hooks_settings_json = db
            .get_predefined_backend_settings("cursor-hooks")
            .unwrap_or_else(|_| "{}".to_string());
        let cursor_hooks_settings: CustomBackendSettings =
            serde_json::from_str(&cursor_hooks_settings_json).unwrap_or_default();

        let cursor_hooks_router = create_cursor_hooks_router(
            db.clone(),
            cursor_hooks_settings,
        );

        // Load claude-hooks settings and build its router.
        let claude_hooks_settings_json = db
            .get_predefined_backend_settings("claude")
            .unwrap_or_else(|_| "{}".to_string());
        let claude_hooks_settings: CustomBackendSettings =
            serde_json::from_str(&claude_hooks_settings_json).unwrap_or_default();
        let claude_hooks_router = create_claude_hooks_router(
            db.clone(),
            claude_hooks_settings,
        );

        // Load codex-hooks settings and build its router.
        let codex_hooks_settings_json = db
            .get_predefined_backend_settings("codex")
            .unwrap_or_else(|_| "{}".to_string());
        let codex_hooks_settings: CustomBackendSettings =
            serde_json::from_str(&codex_hooks_settings_json).unwrap_or_default();
        let codex_hooks_router = create_codex_hooks_router(
            db.clone(),
            codex_hooks_settings,
        );

        // Build shell compression router
        let cli_compression_router = Router::new().route("/", post(cli_compression_handler));

        // Build app
        let app = Router::new()
            .route("/", get(health_handler))
            .nest("/cli_compression", cli_compression_router)
            .nest("/cursor_hook", cursor_hooks_router)
            .nest("/claude_hook", claude_hooks_router)
            .nest("/codex_hook", codex_hooks_router);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind to port {}: {}", port, e);
                // Set status to failed
                {
                    let mut status = SERVER_STATUS.lock().unwrap();
                    *status = ServerStatus::Failed(port, format!("{}", e));
                }
                // Emit failure event to frontend
                let _ = app_handle.emit(
                    "server-failed",
                    serde_json::json!({
                        "port": port,
                        "error": format!("{}", e)
                    }),
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        };
        println!("Server running on http://0.0.0.0:{}", port);
        // Set status to running
        {
            let mut status = SERVER_STATUS.lock().unwrap();
            *status = ServerStatus::Running(port);
        }
        // Emit success event to frontend
        let _ = app_handle.emit(
            "server-started",
            serde_json::json!({
                "port": port
            }),
        );

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
                    println!("Received restart signal, shutting down server...");
                    break;
                }
            }
        });

        if let Err(e) = server.await {
            eprintln!("Server error: {}", e);
        }

        println!("Server stopped, restarting with new configuration...");
        // Small delay before restart
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}
