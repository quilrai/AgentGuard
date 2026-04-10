// DLP Demo App - Main Library
//
// A Tauri app that hosts a local HTTP server for hook receivers (Cursor IDE,
// Claude Code, and Codex CLI) with DLP (Data Loss Prevention) detection. Logs,
// analytics, and detection results flow in via hooks installed in the agent
// CLIs/IDEs.

mod builtin_patterns;
mod claude_hooks;
mod codex_hooks;
mod commands;
mod compression;
mod ctx_read;
mod cursor_hooks;
mod database;
mod dlp;
mod dlp_pattern_config;
mod pattern_utils;
mod predefined_backend_settings;
mod requestresponsemetadata;
mod server;
mod shell_compression;
mod token_saving;

use database::get_port_from_db;
use dlp_pattern_config::DEFAULT_PORT;
use std::sync::{Arc, Mutex};
use tauri::{
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
    AppHandle, Manager, WindowEvent, PhysicalPosition,
};
use tokio::sync::watch;

#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;

// Helper to show window and set dock visibility on macOS
fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// Helper to hide window and hide from dock on macOS
fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    }
}

// Command to show main window (called from tray popup)
#[tauri::command]
fn show_main_window(app: AppHandle) {
    show_window(&app);
}

// Command to quit app (called from tray popup)
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// Server status enum
#[derive(Clone, Debug)]
pub enum ServerStatus {
    Starting,
    Running(u16),        // port
    Failed(u16, String), // port, error message
}

// Global state for HTTP server control
pub static SERVER_PORT: std::sync::LazyLock<Arc<Mutex<u16>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(DEFAULT_PORT)));
pub static RESTART_SENDER: std::sync::LazyLock<Arc<Mutex<Option<watch::Sender<bool>>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));
pub static SERVER_STATUS: std::sync::LazyLock<Arc<Mutex<ServerStatus>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(ServerStatus::Starting)));

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize HTTP server port from environment variable or database
    {
        let port = std::env::var("QPORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or_else(get_port_from_db);

        if std::env::var("QPORT").is_ok() {
            println!("[SERVER] Using port {} from QPORT environment variable", port);
        }

        let mut current_port = SERVER_PORT.lock().unwrap();
        *current_port = port;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Spawn HTTP server with app handle for events
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[SERVER] Failed to create tokio runtime: {}", e);
                        return;
                    }
                };
                rt.block_on(server::start_server(app_handle));
            });

            // Build tray icon with click handler to toggle popup
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(popup) = app.get_webview_window("tray_popup") {
                            // Toggle: if visible, hide; otherwise show
                            if popup.is_visible().unwrap_or(false) {
                                let _ = popup.hide();
                                return;
                            }

                            // Get tray icon position and compute popup position
                            if let Ok(Some(tray_rect)) = tray.rect() {
                                let popup_width = 320.0;

                                // Convert position and size to physical values (scale factor 1.0)
                                let pos = tray_rect.position.to_physical::<f64>(1.0);
                                let size = tray_rect.size.to_physical::<f64>(1.0);

                                // Position popup below tray icon, centered
                                let x = pos.x - (popup_width / 2.0) + (size.width / 2.0);
                                let y = pos.y + size.height + 4.0;

                                let _ = popup.set_position(PhysicalPosition::new(x as i32, y as i32));
                            }

                            // Reload the page to fetch fresh stats
                            let _ = popup.eval("loadStats()");
                            let _ = popup.show();
                            let _ = popup.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // Prevent the window from closing, hide it instead
                    api.prevent_close();
                    let app = window.app_handle();
                    hide_window(&app);
                }
                WindowEvent::Focused(false) => {
                    // Hide tray popup when it loses focus (click outside)
                    if window.label() == "tray_popup" {
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Tray popup commands
            show_main_window,
            quit_app,
            commands::get_tray_stats,
            commands::get_tray_token_timeline,
            // Main app commands
            commands::greet,
            commands::get_dashboard_stats,
            commands::get_backends,
            commands::get_models,
            commands::get_message_logs,
            commands::export_message_logs,
            commands::get_port_setting,
            commands::get_server_status,
            commands::save_port_setting,
            commands::restart_server,
            commands::get_dlp_settings,
            commands::add_dlp_pattern,
            commands::update_dlp_pattern,
            commands::toggle_dlp_pattern,
            commands::delete_dlp_pattern,
            commands::get_dlp_detection_stats,
            commands::get_dlp_detections_for_request,
            commands::test_dlp_pattern,
            // Tool call commands
            commands::get_tool_calls_for_request,
            commands::get_tool_call_stats,
            commands::get_tool_call_insights,
            commands::install_cursor_hooks,
            commands::uninstall_cursor_hooks,
            commands::check_cursor_hooks_installed,
            commands::install_claude_hooks,
            commands::uninstall_claude_hooks,
            commands::check_claude_hooks_installed,
            commands::install_codex_hooks,
            commands::uninstall_codex_hooks,
            commands::check_codex_hooks_installed,
            // Predefined backends commands
            commands::get_predefined_backends,
            commands::update_predefined_backend,
            commands::reset_predefined_backend,
            // Shell compression commands
            commands::install_compression_hook_claude,
            commands::uninstall_compression_hook_claude,
            commands::check_compression_hook_claude,
            commands::install_compression_hook_cursor,
            commands::uninstall_compression_hook_cursor,
            commands::check_compression_hook_cursor,
            // File read caching commands
            commands::install_ctx_read_hook_claude,
            commands::uninstall_ctx_read_hook_claude,
            commands::check_ctx_read_hook_claude,
            // Home screen
            commands::get_home_facts,
            // Garden
            commands::get_garden_stats,
            commands::get_garden_detail,
            commands::get_garden_live,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
