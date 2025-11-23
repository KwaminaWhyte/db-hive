//! DDL (Data Definition Language) generators
//!
//! This module contains SQL generators for creating, altering, and dropping
//! database objects. Each database driver has its own generator to handle
//! database-specific syntax and features.

pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod sqlserver;

use crate::models::{
    ddl::{AlterTableDefinition, DdlResult, DropTableDefinition, TableDefinition},
    DbDriver, DbError,
};

/// DDL generator trait
///
/// Defines the interface for generating DDL SQL statements for different databases.
pub trait DdlGenerator {
    /// Generate CREATE TABLE statement
    fn generate_create_table(&self, table: &TableDefinition) -> Result<DdlResult, DbError>;

    /// Generate ALTER TABLE statement
    fn generate_alter_table(&self, alter: &AlterTableDefinition) -> Result<DdlResult, DbError>;

    /// Generate DROP TABLE statement
    fn generate_drop_table(&self, drop: &DropTableDefinition) -> Result<DdlResult, DbError>;
}

/// Get DDL generator for a specific database driver
pub fn get_ddl_generator(driver: &DbDriver) -> Result<Box<dyn DdlGenerator>, DbError> {
    match driver {
        DbDriver::Postgres => Ok(Box::new(postgres::PostgresDdlGenerator)),
        DbDriver::MySql => Ok(Box::new(mysql::MySqlDdlGenerator)),
        DbDriver::Sqlite => Ok(Box::new(sqlite::SqliteDdlGenerator)),
        DbDriver::SqlServer => Ok(Box::new(sqlserver::SqlServerDdlGenerator)),
        DbDriver::MongoDb => Err(DbError::InvalidInput(
            "DDL operations not supported for MongoDB (NoSQL database)".to_string(),
        )),
    }
}
