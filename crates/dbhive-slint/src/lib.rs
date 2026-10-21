//! DB Hive's Slint shell.
//!
//! UI lives in `ui/*.slint`; this crate is the thin Rust layer that feeds it.
//! Anything that is not glue belongs in `dbhive-core` (or the shared UI model
//! crate once it exists), where it is testable without a window.

slint::include_modules!();

pub mod activity_view;
pub mod app;
pub mod backup_view;
pub mod builder_view;
pub mod connection_editor;
pub mod connection_list;
pub mod designer_view;
pub mod diagram_model;
pub mod erd_view;
pub mod grid_model;
pub mod history_view;
pub mod import_export_view;
pub mod migrations_view;
pub mod palette_view;
pub mod runtime;
pub mod settings_view;
pub mod sql_highlight;
pub mod table_dialog_view;
pub mod workspace_model;
