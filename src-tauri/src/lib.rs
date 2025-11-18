// Module declarations
mod commands;
mod drivers;
mod models;
mod state;

use std::sync::Mutex;
use state::AppState;
use tauri::Manager;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Initialize and manage application state
            app.manage(Mutex::new(AppState::default()));
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::connection::test_connection_command,
            commands::connection::create_connection_profile,
            commands::connection::update_connection_profile,
            commands::connection::delete_connection_profile,
            commands::connection::list_connection_profiles,
            commands::connection::connect_to_database,
            commands::connection::disconnect_from_database,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
