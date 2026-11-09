//! UI-independent view logic.
//!
//! Everything a screen needs that is not drawing: SQL generation, validation,
//! filtering and sorting, formatting, layout, the command registry. It was
//! written inside the egui screens and lifted here so the Slint shell reuses
//! it, tests and all, instead of porting it a second time.
//!
//! The rule for this crate: no UI toolkit types, ever. If a function needs
//! `egui::` or `slint::` it belongs in a shell, not here.

pub mod activity_monitor;
pub mod activity_panel;
pub mod ai_assistant;
pub mod ai_panel;
pub mod backup;
pub mod backup_manager;
pub mod builder_editor;
pub mod command_palette;
pub mod connect_flow;
pub mod connection_editor;
pub mod connection_folders;
pub mod data_forms;
pub mod designer_session;
pub mod editor_prefs;
pub mod er_diagram;
pub mod erd_scene;
pub mod file_pick;
pub mod format;
pub mod graph_layout;
pub mod history;
pub mod history_panel;
pub mod import_export;
pub mod import_wizard;
pub mod keymap;
pub mod layout;
pub mod migrations;
pub mod migrations_panel;
pub mod palette_session;
pub mod plugins;
pub mod plugins_panel;
pub mod procedures;
pub mod query_builder;
pub mod query_tabs;
pub mod result_chart;
pub mod result_export;
pub mod results_grid;
pub mod routine_list;
pub mod routine_run;
pub mod row_edits;
pub mod saved_tabs;
pub mod schema_designer;
pub mod schema_tree;
pub mod settings;
pub mod settings_editor;
pub mod shell;
pub mod sql_completion;
pub mod sql_guard;
pub mod sql_suggest;
pub mod table_dialog;
pub mod table_editor;
pub mod table_inspector;
pub mod table_view;
pub mod tokens;
pub mod typeahead;
pub mod update_state;
pub mod window_state;
pub mod workspace_tabs;
