//! Schema migration tools
//!
//! Provides schema diff computation and migration SQL generation. Compares two
//! sets of `TableSchema` values (source vs. target) and emits structured diffs
//! plus ALTER/CREATE/DROP SQL statements that bring the target schema into
//! alignment with the source.

pub mod diff;
pub mod sql_gen;

pub use diff::{
    compute_diff, ColumnChange, ForeignKeyDiff, IndexDiff, SchemaDiff, TableDiff,
};
pub use sql_gen::generate_migration_sql;
