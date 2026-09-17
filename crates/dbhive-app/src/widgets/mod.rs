//! Shared widgets.
//!
//! Screens compose these rather than rolling their own. The results grid in
//! particular is used by the query view, the table inspector and the data
//! browser, and they must behave identically.

pub mod file_dialog;
// Moved to dbhive-ui-model; re-exported so existing paths keep working.
pub use dbhive_ui_model::graph_layout;
pub mod results_grid;
pub mod sql_editor;
pub mod toast;
pub mod ui_kit;
