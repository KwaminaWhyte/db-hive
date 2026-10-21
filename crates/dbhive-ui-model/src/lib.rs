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
pub mod backup;
pub mod command_palette;
pub mod connection_editor;
pub mod er_diagram;
pub mod format;
pub mod graph_layout;
pub mod history;
pub mod history_panel;
pub mod import_export;
pub mod migrations;
pub mod migrations_panel;
pub mod palette_session;
pub mod plugins;
pub mod procedures;
pub mod query_builder;
pub mod results_grid;
pub mod schema_designer;
pub mod settings;
pub mod settings_editor;
pub mod sql_completion;
pub mod sql_guard;
pub mod table_editor;
pub mod table_inspector;
pub mod table_view;
pub mod tokens;
pub mod workspace_tabs;
