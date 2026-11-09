//! DB Hive core.
//!
//! Every part of the application that is not tied to a particular UI shell:
//! the database drivers, the DDL builders, SSH tunnelling, credential
//! storage, schema migrations, the AI helpers and the activity log.
//!
//! This crate has no dependency on Slint or any other GUI framework: the app
//! is a thin shell over the logic here, which keeps every operation testable
//! without a window.
//!
//! Anything that needs a window or an event emitter belongs in
//! `crates/dbhive-slint`, not here.

pub mod activity_logger;
pub mod ai;
pub mod credentials;
pub mod ddl;
pub mod drivers;
pub mod migrations;
pub mod models;
pub mod plugins;
pub mod service;
pub mod service_ai;
pub mod service_backup;
pub mod service_import_export;
pub mod service_migrations;
pub mod service_monitoring;
pub mod service_procedures;
pub mod service_schema;
pub mod service_updater;
pub mod settings;
pub mod ssh;
pub mod state;
pub mod storage;

pub use activity_logger::ActivityLogger;
pub use state::{AppState, MetadataCache, MAX_HISTORY_ENTRIES};
