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
mod dep_protection;
mod dlp;
mod dlp_pattern_config;
mod pattern_utils;
mod predefined_backend_settings;
mod requestresponsemetadata;
mod server;
mod shell_compression;
pub mod symbols;
mod token_saving;

use database::get_port_from_db;
use dlp_pattern_config::DEFAULT_PORT;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_opener::OpenerExt;
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

const TRAY_OPEN_APP_ID: &str = "tray_open_app";
const TRAY_STAR_GITHUB_ID: &str = "tray_star_github";
const TRAY_REPORT_ISSUE_ID: &str = "tray_report_issue";
const TRAY_UPDATE_ID: &str = "tray_update";
const TRAY_QUIT_ID: &str = "tray_quit";

pub struct UpdateTrayItem(pub std::sync::Mutex<Option<MenuItem<tauri::Wry>>>);

#[tauri::command]
fn set_update_tray_label(
    state: tauri::State<'_, UpdateTrayItem>,
    version: Option<String>,
) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(item) = guard.as_ref() {
        let text = match version {
            Some(v) => format!("Install Update — v{}", v),
            None => "Check for Updates...".to_string(),
        };
        item.set_text(text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn format_compact_number(value: i64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn truncate_label(text: &str, max_len: usize) -> String {
    let char_count = text.chars().count();
    if char_count > max_len && max_len > 3 {
        let mut truncated: String = text.chars().take(max_len - 3).collect();
        truncated.push_str("...");
        truncated
    } else {
        text.to_string()
    }
}

fn format_tray_totals(backends: &[commands::BackendStats]) -> String {
    let total_requests: i64 = backends.iter().map(|backend| backend.request_count).sum();
    let total_input: i64 = backends.iter().map(|backend| backend.input_tokens).sum();
    let total_output: i64 = backends.iter().map(|backend| backend.output_tokens).sum();
    let total_cache: i64 = backends.iter().map(|backend| backend.cache_tokens).sum();

    let mut summary = format!(
        "{} req | {} in | {} out",
        format_compact_number(total_requests),
        format_compact_number(total_input),
        format_compact_number(total_output)
    );

    if total_cache > 0 {
        summary.push_str(&format!(" | {} cache", format_compact_number(total_cache)));
    }

    summary
}

fn format_backend_summary(backend: &commands::BackendStats) -> String {
    let mut summary = format!(
        "{} | {} req | {} in",
        truncate_label(&backend.backend, 18),
        format_compact_number(backend.request_count),
        format_compact_number(backend.input_tokens)
    );

    if backend.output_tokens > 0 {
        summary.push_str(&format!(
            " | {} out",
            format_compact_number(backend.output_tokens)
        ));
    }

    summary
}

fn refresh_tray_stats<R: Runtime>(summary_item: &MenuItem<R>, backend_items: &[MenuItem<R>]) {
    match commands::get_tray_stats() {
        Ok(stats) if stats.backends.is_empty() => {
            let _ = summary_item.set_text("No activity in last 24h");
            for item in backend_items {
                let _ = item.set_text(" ");
            }
        }
        Ok(stats) => {
            let _ = summary_item.set_text(format_tray_totals(&stats.backends));

            for (item, backend) in backend_items.iter().zip(stats.backends.iter()) {
                let _ = item.set_text(format_backend_summary(backend));
            }

            for item in backend_items.iter().skip(stats.backends.len()) {
                let _ = item.set_text(" ");
            }
        }
        Err(err) => {
            eprintln!("[TRAY] Failed to load tray stats: {err}");
            let _ = summary_item.set_text("Stats unavailable");

            if let Some(first_item) = backend_items.first() {
                let _ = first_item.set_text("Check app logs for details");
            }

            for item in backend_items.iter().skip(1) {
                let _ = item.set_text(" ");
            }
        }
    }
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
            println!(
                "[SERVER] Using port {} from QPORT environment variable",
                port
            );
        }

        let mut current_port = SERVER_PORT.lock().unwrap();
        *current_port = port;
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            if let Err(err) = commands::migrate_installed_compression_hooks() {
                eprintln!("[SHELL_COMPRESSION] Hook migration skipped/failed: {err}");
            }

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
            let Some(icon) = app.default_window_icon().cloned() else {
                eprintln!("[TRAY] No default window icon found; skipping tray icon setup.");
                return Ok(());
            };

            let stats_header_item = MenuItem::new(app, "Last 24 Hours", false, None::<&str>)?;
            let stats_summary_item = MenuItem::new(app, "Loading usage...", false, None::<&str>)?;
            let backend_item_1 = MenuItem::new(app, " ", false, None::<&str>)?;
            let backend_item_2 = MenuItem::new(app, " ", false, None::<&str>)?;
            let backend_item_3 = MenuItem::new(app, " ", false, None::<&str>)?;
            let backend_items = [
                backend_item_1.clone(),
                backend_item_2.clone(),
                backend_item_3.clone(),
            ];

            refresh_tray_stats(&stats_summary_item, &backend_items);

            let separator_1 = PredefinedMenuItem::separator(app)?;
            let separator_2 = PredefinedMenuItem::separator(app)?;
            let separator_3 = PredefinedMenuItem::separator(app)?;
            let open_app_item =
                MenuItem::with_id(app, TRAY_OPEN_APP_ID, "Open App", true, None::<&str>)?;
            let star_github_item = MenuItem::with_id(
                app,
                TRAY_STAR_GITHUB_ID,
                "Star on GitHub",
                true,
                None::<&str>,
            )?;
            let report_issue_item = MenuItem::with_id(
                app,
                TRAY_REPORT_ISSUE_ID,
                "Report Issue",
                true,
                None::<&str>,
            )?;
            let update_item = MenuItem::with_id(
                app,
                TRAY_UPDATE_ID,
                "Check for Updates...",
                true,
                None::<&str>,
            )?;
            app.manage(UpdateTrayItem(std::sync::Mutex::new(Some(
                update_item.clone(),
            ))));
            let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;

            let tray_menu = Menu::with_items(
                app,
                &[
                    &stats_header_item,
                    &stats_summary_item,
                    &backend_item_1,
                    &backend_item_2,
                    &backend_item_3,
                    &separator_1,
                    &open_app_item,
                    &star_github_item,
                    &report_issue_item,
                    &separator_2,
                    &update_item,
                    &separator_3,
                    &quit_item,
                ],
            )?;

            let stats_summary_item_for_click = stats_summary_item.clone();
            let backend_items_for_click = backend_items.clone();

            if let Err(err) = TrayIconBuilder::new()
                .icon(icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    TRAY_OPEN_APP_ID => show_window(app),
                    TRAY_STAR_GITHUB_ID => {
                        if let Err(err) = app
                            .opener()
                            .open_url("https://github.com/quilrai/AgentGuard", None::<&str>)
                        {
                            eprintln!("[TRAY] Failed to open GitHub page: {err}");
                        }
                    }
                    TRAY_REPORT_ISSUE_ID => {
                        if let Err(err) = app
                            .opener()
                            .open_url("https://github.com/quilrai/AgentGuard/issues", None::<&str>)
                        {
                            eprintln!("[TRAY] Failed to open issue tracker: {err}");
                        }
                    }
                    TRAY_UPDATE_ID => {
                        show_window(app);
                        let _ = app.emit("tray-update-clicked", ());
                    }
                    TRAY_QUIT_ID => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::Click {
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        refresh_tray_stats(&stats_summary_item_for_click, &backend_items_for_click);
                    }
                })
                .build(app)
            {
                eprintln!("[TRAY] Failed to build tray icon: {err}");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Prevent the window from closing, hide it instead
                api.prevent_close();
                let app = window.app_handle();
                hide_window(&app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_update_tray_label,
            // Main app commands
            commands::greet,
            commands::get_dashboard_stats,
            commands::get_backends,
            commands::get_models,
            commands::get_message_logs,
            commands::get_message_log_detail,
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
            commands::get_token_savings_stats,
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
            // Garden symbols (tree-sitter)
            commands::get_file_symbols,
            commands::get_import_graph,
            // Garden disk browsing
            commands::browse_directory,
            // Agent behaviour
            commands::get_agent_behaviour,
        ]);

    if let Err(err) = app.run(tauri::generate_context!()) {
        eprintln!("[TAURI] Error while running tauri application: {err}");
        std::process::exit(1);
    }
}
