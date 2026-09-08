//! Application screens.
//!
//! Each module owns one feature area from
//! docs/rust-ui-migration/01-frontend-inventory.md and keeps its own state
//! struct. State lives in `Screens` below so that adding a field to one
//! screen never touches another.

pub mod activity_monitor;
pub mod ai_assistant;
pub mod backup;
pub mod command_palette;
pub mod connection_editor;
pub mod connections;
pub mod er_diagram;
pub mod history;
pub mod import_export;
pub mod migrations;
pub mod plugins;
pub mod procedures;
pub mod query_builder;
pub mod schema_designer;
pub mod settings;
pub mod table_editor;
pub mod table_inspector;
pub mod workspace;

/// The full-window view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// A new window starts on the connection list, as the first one does.
    #[default]
    Connections,
    Workspace,
}

/// State for every screen and dialog, owned by the app.
#[derive(Default)]
pub struct Screens {
    pub activity_monitor: activity_monitor::ActivityMonitor,
    pub ai_assistant: ai_assistant::AiAssistant,
    pub backup: backup::Backup,
    pub command_palette: command_palette::CommandPalette,
    pub connection_editor: connection_editor::ConnectionEditor,
    pub er_diagram: er_diagram::ErDiagram,
    pub history: history::History,
    pub import_export: import_export::ImportExport,
    pub migrations: migrations::Migrations,
    pub plugins: plugins::Plugins,
    pub procedures: procedures::Procedures,
    pub query_builder: query_builder::QueryBuilder,
    pub schema_designer: schema_designer::SchemaDesigner,
    pub settings: settings::Settings,
    pub table_editor: table_editor::TableEditor,
    pub table_inspector: table_inspector::TableInspector,
}
