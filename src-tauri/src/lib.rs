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
            // Initialize application state
            let mut state = AppState::default();

            // Load saved profiles from persistent storage
            match state.load_profiles_from_store(&app.handle()) {
                Ok(count) => {
                    if count > 0 {
                        println!("Loaded {} connection profile(s) from storage", count);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load profiles from storage: {}", e);
                }
            }

            // Manage the state
            app.manage(Mutex::new(state));
            Ok(())
        })
        .plugin(tauri_plugin_store::Builder::new().build())
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
            commands::connection::switch_database,
            commands::query::execute_query,
            commands::schema::get_databases,
            commands::schema::get_schemas,
            commands::schema::get_tables,
            commands::schema::get_table_schema,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
