// DLP Demo App - Main Library
//
// A Tauri app that proxies LLM API requests with DLP (Data Loss Prevention) capabilities.
// Currently supports Claude (Anthropic), with plans for OpenAI, Gemini, etc.

mod backends;
mod builtin_patterns;
mod commands;
mod cursor_hooks;
mod database;
mod dlp;
mod dlp_pattern_config;
mod pattern_utils;
mod proxy;
mod requestresponsemetadata;
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

// Proxy status enum
#[derive(Clone, Debug)]
pub enum ProxyStatus {
    Starting,
    Running(u16),        // port
    Failed(u16, String), // port, error message
}

// Global state for reverse proxy control
pub static PROXY_PORT: std::sync::LazyLock<Arc<Mutex<u16>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(DEFAULT_PORT)));
pub static RESTART_SENDER: std::sync::LazyLock<Arc<Mutex<Option<watch::Sender<bool>>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));
pub static PROXY_STATUS: std::sync::LazyLock<Arc<Mutex<ProxyStatus>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(ProxyStatus::Starting)));

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize reverse proxy port from environment variable or database
    {
        let port = std::env::var("QPORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or_else(get_port_from_db);

        if std::env::var("QPORT").is_ok() {
            println!("[PROXY] Using port {} from QPORT environment variable", port);
        }

        let mut current_port = PROXY_PORT.lock().unwrap();
        *current_port = port;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Spawn reverse proxy server with app handle for events
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[PROXY] Failed to create tokio runtime: {}", e);
                        return;
                    }
                };
                rt.block_on(proxy::start_proxy_server(app_handle));
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
            commands::get_proxy_status,
            commands::save_port_setting,
            commands::restart_proxy,
            commands::get_dlp_settings,
            commands::add_dlp_pattern,
            commands::update_dlp_pattern,
            commands::toggle_dlp_pattern,
            commands::delete_dlp_pattern,
            commands::get_dlp_detection_stats,
            commands::get_dlp_detections_for_request,
            commands::get_dlp_action_setting,
            commands::save_dlp_action_setting,
            commands::test_dlp_pattern,
            // Tool call commands
            commands::get_tool_calls_for_request,
            commands::get_tool_call_stats,
            commands::get_tool_call_insights,
            commands::set_shell_env,
            commands::check_shell_env,
            commands::remove_shell_env,
            commands::install_cursor_hooks,
            commands::uninstall_cursor_hooks,
            commands::check_cursor_hooks_installed,
            // Claude Code settings commands
            commands::check_claude_code_settings,
            commands::set_claude_code_settings,
            commands::remove_claude_code_settings,
            // Custom backends commands
            commands::get_custom_backends,
            commands::add_custom_backend,
            commands::update_custom_backend,
            commands::toggle_custom_backend,
            commands::delete_custom_backend,
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
            commands::install_compression_hook_codex,
            commands::uninstall_compression_hook_codex,
            commands::check_compression_hook_codex,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
