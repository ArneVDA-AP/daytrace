//! Daytrace: an unofficial Windows port of Dayflow (https://github.com/JerryZLiu/Dayflow, MIT).

pub mod analysis;
pub mod batching;
pub mod capture;
mod commands;
pub mod config;
pub mod db;
mod export;
pub mod features;
pub mod imaging;
pub mod llm;
pub mod platform;
pub mod state;
pub mod timeutil;
mod tray;
pub mod types;

use std::sync::Arc;
use tauri::Manager;

fn init_logging(dir: &std::path::Path) {
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("daytrace.log"));
    let mut b = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,tao=warn,wry=warn"));
    b.format_timestamp_secs();
    if let Ok(f) = file {
        b.target(env_logger::Target::Pipe(Box::new(f)));
    }
    let _ = b.try_init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let root = std::env::var_os("DAYTRACE_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&root)?;
            init_logging(&root);
            log::info!("Daytrace {} starting; data dir {}", env!("CARGO_PKG_VERSION"), root.display());
            let paths = config::Paths::new(root)?;
            let state = Arc::new(state::AppState::new(paths)?);
            app.manage(state.clone());
            tray::create(app.handle())?;
            capture::spawn(state.clone());
            features::insights::spawn(state.clone());
            features::storage::spawn(state.clone());
            analysis::spawn(state);
            if !start_minimized {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::provider_defaults,
            commands::get_status,
            commands::set_recording,
            commands::get_day,
            commands::get_card_screenshots,
            commands::get_screenshot,
            commands::retry_card,
            commands::analyze_now,
            commands::list_models,
            commands::test_provider,
            commands::export_markdown,
            commands::open_data_dir,
            // insights commands
            features::insights::insights_day,
            features::insights::insights_week,
            features::insights::insights_recap,
            features::insights::insights_generate_recap,
            features::insights::insights_save_recap,
            features::insights::insights_export_recap,
            // chat commands
            features::chat::chat_info,
            features::chat::chat_set_model,
            features::chat::chat_list_conversations,
            features::chat::chat_get_conversation,
            features::chat::chat_send,
            features::chat::chat_cancel,
            features::chat::chat_delete_conversation,
            features::chat::chat_get_memory,
            features::chat::chat_save_memory,
            features::chat::chat_open_link,
            // storage commands
            features::storage::storage_usage,
            features::storage::storage_delete_older_than,
            features::storage::storage_delete_day,
            features::storage::storage_backup_now,
            features::storage::storage_health,
            features::storage::storage_agent_setup,
            // journal commands
            // platform_extras commands
            // provider commands
        ])
        .build(tauri::generate_context!())
        .expect("error while building Daytrace")
        .run(|_app, event| {
            // Closing the window destroys it; keep running in the tray. Quit from the tray menu
            // calls exit(0), which carries an exit code and is allowed through.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
