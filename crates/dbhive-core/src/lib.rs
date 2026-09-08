//! DB Hive core.
//!
//! Every part of the application that is not tied to a particular UI shell:
//! the database drivers, the DDL builders, SSH tunnelling, credential
//! storage, schema migrations, the AI helpers and the activity log.
//!
//! This crate deliberately has no dependency on Tauri, egui, or any other
//! GUI framework. It was extracted from `src-tauri` so that the Tauri shell
//! and the native egui shell can share one implementation rather than
//! forking it, and so the shell can be replaced without touching the logic.
//!
//! Anything that needs an `AppHandle`, a window, or an event emitter belongs
//! in a shell crate, not here.

pub mod activity_logger;
pub mod ai;
pub mod credentials;
pub mod ddl;
pub mod drivers;
pub mod migrations;
pub mod models;
pub mod ssh;

pub use activity_logger::ActivityLogger;
