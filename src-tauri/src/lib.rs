// Module declarations
mod commands;
mod credentials;
mod drivers;
mod models;
mod ssh;
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

            // Load saved passwords from persistent storage
            match state.load_passwords_from_store(&app.handle()) {
                Ok(count) => {
                    if count > 0 {
                        println!("Loaded {} saved password(s) from storage", count);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load passwords from storage: {}", e);
                }
            }

            // Load query history from persistent storage
            match state.load_history_from_store(&app.handle()) {
                Ok(count) => {
                    if count > 0 {
                        println!("Loaded {} query history record(s) from storage", count);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load query history from storage: {}", e);
                }
            }

            // Load query snippets from persistent storage
            match state.load_snippets_from_store(&app.handle()) {
                Ok(count) => {
                    if count > 0 {
                        println!("Loaded {} query snippet(s) from storage", count);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load query snippets from storage: {}", e);
                }
            }

            // Manage the state
            app.manage(Mutex::new(state));
            Ok(())
        })
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::connection::test_connection_command,
            commands::connection::create_connection_profile,
            commands::connection::update_connection_profile,
            commands::connection::delete_connection_profile,
            commands::connection::list_connection_profiles,
            commands::connection::get_saved_password,
            commands::connection::save_password,
            commands::connection::connect_to_database,
            commands::connection::disconnect_from_database,
            commands::connection::switch_database,
            commands::query::execute_query,
            commands::schema::get_databases,
            commands::schema::get_schemas,
            commands::schema::get_tables,
            commands::schema::get_table_schema,
            commands::schema::get_autocomplete_metadata,
            commands::history::save_to_history,
            commands::history::get_query_history,
            commands::history::clear_history,
            commands::history::save_snippet,
            commands::history::list_snippets,
            commands::history::delete_snippet,
            commands::history::get_snippet,
            commands::export::export_to_csv,
            commands::export::export_to_json,
            commands::export::export_to_sql,
            commands::export::import_from_sql,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::reset_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
