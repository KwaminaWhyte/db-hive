//! DB Hive's Slint shell.
//!
//! UI lives in `ui/*.slint`; this crate is the thin Rust layer that feeds it.
//! Anything that is not glue belongs in `dbhive-core` (or the shared UI model
//! crate once it exists), where it is testable without a window.

slint::include_modules!();

pub mod app;
pub mod connection_list;
pub mod diagram_model;
pub mod grid_model;
pub mod runtime;
pub mod sql_highlight;
