//! Gapura desktop app. This crate is an IPC bridge: every real decision
//! lives in gapura-core.

// Public for integration tests; the webview is the only production consumer.
pub mod commands;
pub mod error;
pub mod state;

use gapura_core::paths;
use state::AppState;
use tauri::Manager;

fn init_logging(data_dir: &std::path::Path) {
    let file_appender = tracing_appender::rolling::daily(data_dir, "gapura.log");
    let (writer, _guard) = tracing_appender::non_blocking(file_appender);
    // Keep the guard alive for the lifetime of the process.
    std::mem::forget(_guard);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

pub fn run() {
    let data_dir = paths::ensure_dir(&paths::data_dir().expect("data dir must resolve"))
        .expect("data dir must be creatable");
    let config_dir = paths::ensure_dir(&paths::config_dir().expect("config dir must resolve"))
        .expect("config dir must be creatable");
    init_logging(&data_dir);

    let app_state =
        AppState::new(data_dir.clone(), config_dir.clone()).expect("app state must initialize");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            // vault
            commands::vault::vault_status,
            commands::vault::vault_setup,
            commands::vault::vault_unlock,
            commands::vault::vault_lock,
            commands::vault::close_all_sessions,
            // connections & groups
            commands::connections::list_connections,
            commands::connections::save_connection,
            commands::connections::delete_connection,
            commands::connections::list_groups,
            commands::connections::save_group,
            commands::connections::delete_group,
            // sessions
            commands::sessions::list_sessions,
            commands::sessions::session_open,
            commands::sessions::session_reconnect,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_close,
            // keys
            commands::keys::list_keys,
            commands::keys::generate_key,
            commands::keys::register_key,
            commands::keys::delete_key,
            // import
            commands::import::import_ssh_config,
            commands::import::commit_import,
            commands::import::preview_import,
            // settings & misc
            commands::misc::get_settings,
            commands::misc::save_settings,
            commands::misc::list_known_hosts,
            commands::misc::remove_known_host,
            commands::misc::app_paths,
            commands::misc::open_data_dir,
            commands::misc::version_info,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Sessions get a chance to close cleanly.
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    let _ = commands::vault::close_all_sessions(state);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
