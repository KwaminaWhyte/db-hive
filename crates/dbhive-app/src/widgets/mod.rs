//! Shared widgets.
//!
//! Screens compose these rather than rolling their own. The results grid in
//! particular is used by the query view, the table inspector and the data
//! browser, and they must behave identically.

pub mod file_dialog;
pub mod graph_layout;
pub mod results_grid;
pub mod sql_editor;
pub mod toast;
