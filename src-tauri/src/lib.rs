// Module declarations
mod ai;
mod commands;
mod credentials;
mod ddl;
mod drivers;
mod models;
mod plugins;
mod ssh;
mod state;

use std::sync::{Arc, Mutex};
use plugins::{loader::PluginLoader, PluginManager};
use state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

// Helper function to toggle window visibility
fn toggle_window_visibility(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            Ok(false) => {
                let _ = window.show();
                let _ = window.set_focus();
            }
            Err(e) => {
                eprintln!("Error checking window visibility: {}", e);
            }
        }
    }
}

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

            // Initialize AI state
            app.manage(commands::ai::AiState::default());

            // Initialize plugin manager
            let plugin_manager = PluginManager::new(app.handle().clone());

            // Initialize plugins asynchronously
            let plugin_manager_clone = Arc::new(tokio::sync::Mutex::new(plugin_manager));
            let manager = plugin_manager_clone.clone();
            tauri::async_runtime::spawn(async move {
                let manager = manager.lock().await;
                if let Err(e) = manager.initialize().await {
                    eprintln!("Failed to initialize plugin manager: {}", e);
                }
            });

            // Manage the plugin manager for commands
            app.manage(plugin_manager_clone);

            // Initialize plugin loader
            let plugin_loader = PluginLoader::new(app.handle().clone());
            let plugin_loader_arc = Arc::new(tokio::sync::Mutex::new(plugin_loader));
            app.manage(plugin_loader_arc);

            // Create system tray menu
            let show_hide_i = MenuItem::with_id(app, "show_hide", "Show/Hide", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_hide_i, &quit_i])?;

            // Create tray icon with menu and event handler
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show_hide" => {
                        toggle_window_visibility(app);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        toggle_window_visibility(app);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::connection::test_connection_command,
            commands::connection::create_connection_profile,
            commands::connection::update_connection_profile,
            commands::connection::delete_connection_profile,
            commands::connection::list_connection_profiles,
            commands::connection::get_saved_password,
            commands::connection::save_password,
            commands::connection::save_ssh_password,
            commands::connection::get_ssh_password,
            commands::connection::connect_to_database,
            commands::connection::disconnect_from_database,
            commands::connection::switch_database,
            commands::connection::record_connection,
            commands::connection::toggle_favorite,
            commands::connection::update_connection_folder,
            commands::connection::get_connection_stats,
            commands::connection::get_recent_connections,
            commands::connection::duplicate_connection,
            commands::query::execute_query,
            commands::schema::get_databases,
            commands::schema::get_schemas,
            commands::schema::get_tables,
            commands::schema::get_table_schema,
            commands::schema::get_foreign_keys,
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
            commands::activity::get_query_logs,
            commands::activity::get_activity_stats,
            commands::activity::clear_query_logs,
            commands::activity::clear_old_query_logs,
            commands::activity::export_query_logs,
            commands::activity::update_query_log_tags,
            commands::activity::get_query_logs_count,
            commands::ddl::preview_create_table,
            commands::ddl::create_table,
            commands::ddl::preview_alter_table,
            commands::ddl::alter_table,
            commands::ddl::preview_drop_table,
            commands::ddl::drop_table,
            commands::plugins::get_installed_plugins,
            commands::plugins::get_plugin,
            commands::plugins::install_plugin,
            commands::plugins::uninstall_plugin,
            commands::plugins::enable_plugin,
            commands::plugins::disable_plugin,
            commands::plugins::update_plugin_config,
            commands::plugins::get_marketplace_plugins,
            commands::plugins::load_plugin,
            commands::plugins::unload_plugin_runtime,
            commands::plugins::execute_plugin_function,
            commands::plugins::get_loaded_plugins,
            commands::plugins::is_plugin_loaded,
            commands::data_import::preview_import_file,
            commands::data_import::import_data_to_table,
            commands::data_import::get_tables_for_import,
            commands::data_import::get_table_columns_for_import,
            commands::ai::check_ollama_status,
            commands::ai::check_ai_provider_status,
            commands::ai::get_ai_config,
            commands::ai::set_ai_config,
            commands::ai::set_active_ai_provider,
            commands::ai::set_ai_api_key,
            commands::ai::list_ai_models,
            commands::ai::ai_generate_sql,
            commands::ai::ai_explain_query,
            commands::ai::ai_optimize_query,
            commands::ai::ai_fix_query,
            commands::ai::ai_chat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
