//! Schema browser operations: the overview a sidebar tree is built from, SQL
//! the browser generates for a table, and dropping a table safely.
//!
//! Nothing here knows about a UI. Pure functions take a `quote` closure (the
//! driver's own `quote_identifier`) so every identifier is quoted by the
//! dialect that will read it, and none of them concatenate user text into SQL
//! unquoted. The async functions take a driver by reference so a caller can
//! clone its `Arc<dyn DatabaseDriver>` out of `AppState`, drop the lock, and
//! await without ever holding it.

use crate::ddl::get_ddl_generator;
use crate::drivers::DatabaseDriver;
use crate::models::{
    ColumnInfo, DbDriver, DbError, DdlResult, DropTableDefinition, ForeignKeyInfo,
};
use crate::service_procedures::qualify;

/// Most schemas whose foreign keys are read when looking for what depends on
/// a table. A server with more than this has bigger problems than a slow
/// drop dialog, and the answer would be a wall of text anyway.
pub const MAX_DEPENDENCY_SCAN_SCHEMAS: usize = 50;

/// What the tree needs before it can draw anything: the server's databases,
/// the one the connection is on, and that database's schemas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub databases: Vec<String>,
    /// The database the connection is on.
    pub database: String,
    /// Schemas of `database`. Empty for a driver with no schema level
    /// (MySQL, SQLite), where the database itself is what `get_tables` takes.
    pub schemas: Vec<String>,
}

/// A table named with its schema, for lists of "what depends on this".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
}

impl TableRef {
    /// Display only. Never used in SQL, so it is not quoted.
    pub fn display(&self) -> String {
        if self.schema.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.schema, self.name)
        }
    }
}

/// Whether a schema is the server's own bookkeeping rather than the user's
/// data. The tree hides these.
pub fn is_system_schema(name: &str) -> bool {
    name == "information_schema" || name.starts_with("pg_")
}

/// Read the databases and schemas. A failing schema list does not cost the
/// user their tables: it degrades to "no schema level" and says nothing,
/// which matches how the flat list always behaved.
///
/// `wanted_database` is the database the connection was opened on; when it is
/// absent the first one the server lists is used.
pub async fn load_overview(
    conn: &dyn DatabaseDriver,
    driver: &DbDriver,
    wanted_database: Option<&str>,
) -> Overview {
    let mut databases: Vec<String> = conn
        .get_databases()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.name)
        .collect();
    databases.sort_by_key(|d| d.to_lowercase());

    let database = wanted_database
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .or_else(|| databases.first().cloned())
        .unwrap_or_default();

    let mut schemas: Vec<String> = if driver.is_postgres_compatible() {
        conn.get_schemas(&database)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.name)
            .filter(|s| !is_system_schema(s))
            .collect()
    } else {
        Vec::new()
    };
    schemas.sort_by_key(|s| (s != "public", s.to_lowercase()));

    Overview {
        databases,
        database,
        schemas,
    }
}

// ---------------------------------------------------------------------------
// Generated SQL
// ---------------------------------------------------------------------------

/// Rows a generated SELECT asks for. Enough to look at a table, few enough
/// that running it unread by accident costs nothing.
pub const SELECT_LIMIT: u32 = 100;

/// `SELECT * FROM <table> LIMIT 100;`, in the dialect's own row-limit syntax.
///
/// `qualifier` is the schema (or database) put in front of the table, empty
/// when the dialect addresses tables bare. Both parts go through `quote`.
pub fn generate_select(
    driver: &DbDriver,
    quote: &dyn Fn(&str) -> String,
    qualifier: &str,
    table: &str,
) -> String {
    let target = qualify(quote, qualifier, table);
    match driver {
        DbDriver::SqlServer => format!("SELECT TOP {SELECT_LIMIT} *\nFROM {target};"),
        _ => format!("SELECT *\nFROM {target}\nLIMIT {SELECT_LIMIT};"),
    }
}

/// An INSERT listing every column the database fills in itself last.
///
/// Auto-increment columns are left out (the server numbers them), and every
/// remaining column gets `NULL` as a placeholder to overwrite. A table whose
/// every column is generated gets `DEFAULT VALUES`, which Postgres, SQLite
/// and SQL Server accept; MySQL's spelling for the same thing is the empty
/// column list, so it gets that.
pub fn generate_insert(
    driver: &DbDriver,
    quote: &dyn Fn(&str) -> String,
    qualifier: &str,
    table: &str,
    columns: &[ColumnInfo],
) -> String {
    let target = qualify(quote, qualifier, table);
    let writable: Vec<&ColumnInfo> = columns.iter().filter(|c| !c.is_auto_increment).collect();
    if writable.is_empty() {
        return match driver {
            DbDriver::MySql => format!("INSERT INTO {target} () VALUES ();"),
            _ => format!("INSERT INTO {target} DEFAULT VALUES;"),
        };
    }
    let names: Vec<String> = writable.iter().map(|c| quote(&c.name)).collect();
    let values: Vec<&str> = writable.iter().map(|_| "NULL").collect();
    format!(
        "INSERT INTO {target}\n  ({})\nVALUES\n  ({});",
        names.join(", "),
        values.join(", ")
    )
}

// ---------------------------------------------------------------------------
// Dropping a table
// ---------------------------------------------------------------------------

/// Tables with a foreign key that points at `schema.table`, itself excluded
/// (a self-reference does not block dropping the table).
///
/// A driver that reports an empty referenced schema (SQLite, MySQL) is
/// matched on the table name alone, because there is only one schema to be in.
pub fn referencing_tables(
    foreign_keys: &[ForeignKeyInfo],
    schema: &str,
    table: &str,
) -> Vec<TableRef> {
    let mut out: Vec<TableRef> = Vec::new();
    for fk in foreign_keys {
        let schema_matches =
            fk.referenced_schema.is_empty() || schema.is_empty() || fk.referenced_schema == schema;
        if fk.referenced_table != table || !schema_matches {
            continue;
        }
        if fk.table == table && fk.schema == schema {
            continue;
        }
        let r = TableRef {
            schema: fk.schema.clone(),
            name: fk.table.clone(),
        };
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out.sort_by(|a, b| a.display().to_lowercase().cmp(&b.display().to_lowercase()));
    out
}

/// Every table that has a foreign key to `schema.table`.
///
/// Foreign keys are read one schema at a time, and a Postgres table can be
/// referenced from another schema, so `scan_schemas` lists the schemas to
/// read. A schema whose keys cannot be read is skipped rather than failing
/// the whole question, but if none could be read at all the error is
/// returned: "nothing depends on it" must never mean "we could not tell".
pub async fn find_dependents(
    conn: &dyn DatabaseDriver,
    scan_schemas: &[String],
    schema: &str,
    table: &str,
) -> Result<Vec<TableRef>, DbError> {
    let mut all: Vec<ForeignKeyInfo> = Vec::new();
    let mut last_error: Option<DbError> = None;
    let mut read_any = false;
    for s in scan_schemas.iter().take(MAX_DEPENDENCY_SCAN_SCHEMAS) {
        match conn.get_foreign_keys(s).await {
            Ok(fks) => {
                read_any = true;
                all.extend(fks);
            }
            Err(e) => last_error = Some(e),
        }
    }
    if !read_any {
        if let Some(e) = last_error {
            return Err(e);
        }
    }
    Ok(referencing_tables(&all, schema, table))
}

/// The DROP TABLE statement, generated (and its identifiers escaped) by the
/// dialect's DDL generator.
///
/// `IF EXISTS` is off on purpose: a drop that silently does nothing because
/// the name was wrong is worse than the error saying so.
pub fn drop_table_sql(
    driver: &DbDriver,
    qualifier: &str,
    table: &str,
    cascade: bool,
) -> Result<Vec<String>, DbError> {
    let generator = get_ddl_generator(driver)?;
    let def = DropTableDefinition {
        schema: (!qualifier.is_empty()).then(|| qualifier.to_string()),
        name: table.trim().to_string(),
        cascade,
        if_exists: false,
    };
    Ok(generator.generate_drop_table(&def)?.sql)
}

/// Drop the table. `cascade` is honoured only where the dialect has it
/// (Postgres); elsewhere it is ignored and the server's own foreign key
/// rules apply, so a refusal comes back as the server's error.
pub async fn drop_table(
    conn: &dyn DatabaseDriver,
    driver: &DbDriver,
    qualifier: &str,
    table: &str,
    cascade: bool,
) -> Result<DdlResult, DbError> {
    let cascade = cascade && driver.is_postgres_compatible();
    let statements = drop_table_sql(driver, qualifier, table, cascade)?;
    for sql in &statements {
        conn.execute_query(sql).await?;
    }
    Ok(DdlResult {
        message: format!("Table '{}' dropped", table.trim()),
        sql: statements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
    fn q_mysql(s: &str) -> String {
        format!("`{}`", s.replace('`', "``"))
    }

    fn col(name: &str, auto: bool) -> ColumnInfo {
        let mut c = ColumnInfo::new(name.to_string(), "text".to_string(), true);
        c.is_auto_increment = auto;
        c
    }

    fn fk(from_schema: &str, from: &str, to_schema: &str, to: &str) -> ForeignKeyInfo {
        ForeignKeyInfo::new(
            format!("{from}_{to}_fk"),
            from.to_string(),
            from_schema.to_string(),
            vec!["x".into()],
            to.to_string(),
            to_schema.to_string(),
            vec!["id".into()],
        )
    }

    #[test]
    fn select_is_quoted_and_qualified() {
        let sql = generate_select(&DbDriver::Postgres, &q, "public", "orders");
        assert_eq!(sql, "SELECT *\nFROM \"public\".\"orders\"\nLIMIT 100;");
    }

    #[test]
    fn select_without_a_qualifier_is_bare() {
        let sql = generate_select(&DbDriver::Sqlite, &q, "", "orders");
        assert_eq!(sql, "SELECT *\nFROM \"orders\"\nLIMIT 100;");
    }

    #[test]
    fn select_uses_the_dialects_row_limit() {
        let sql = generate_select(&DbDriver::SqlServer, &q, "dbo", "orders");
        assert!(sql.starts_with("SELECT TOP 100 *"), "{sql}");
        assert!(!sql.contains("LIMIT"));
    }

    #[test]
    fn a_hostile_table_name_stays_inside_the_quotes() {
        let name = "t\"; DROP TABLE users; --";
        let sql = generate_select(&DbDriver::Postgres, &q, "public", name);
        assert_eq!(
            sql,
            "SELECT *\nFROM \"public\".\"t\"\"; DROP TABLE users; --\"\nLIMIT 100;"
        );
        let mysql = generate_select(
            &DbDriver::MySql,
            &q_mysql,
            "app",
            "t`; DROP TABLE users; --",
        );
        assert!(mysql.contains("`t``; DROP TABLE users; --`"), "{mysql}");
    }

    #[test]
    fn insert_skips_auto_increment_and_lists_the_rest() {
        let cols = vec![col("id", true), col("name", false), col("email", false)];
        let sql = generate_insert(&DbDriver::Postgres, &q, "public", "users", &cols);
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\"\n  (\"name\", \"email\")\nVALUES\n  (NULL, NULL);"
        );
    }

    #[test]
    fn insert_quotes_column_names() {
        let cols = vec![col("we\"ird", false)];
        let sql = generate_insert(&DbDriver::Sqlite, &q, "", "t", &cols);
        assert!(sql.contains("(\"we\"\"ird\")"), "{sql}");
    }

    #[test]
    fn insert_into_an_all_generated_table_uses_default_values() {
        let cols = vec![col("id", true)];
        assert_eq!(
            generate_insert(&DbDriver::Postgres, &q, "public", "t", &cols),
            "INSERT INTO \"public\".\"t\" DEFAULT VALUES;"
        );
        assert_eq!(
            generate_insert(&DbDriver::MySql, &q_mysql, "app", "t", &cols),
            "INSERT INTO `app`.`t` () VALUES ();"
        );
    }

    #[test]
    fn dependents_exclude_the_table_itself() {
        let fks = vec![
            fk("public", "orders", "public", "customers"),
            fk("public", "customers", "public", "customers"),
            fk("public", "orders", "public", "products"),
        ];
        let deps = referencing_tables(&fks, "public", "customers");
        assert_eq!(
            deps,
            vec![TableRef {
                schema: "public".into(),
                name: "orders".into()
            }]
        );
    }

    #[test]
    fn dependents_are_found_across_schemas_and_deduplicated() {
        let fks = vec![
            fk("billing", "invoices", "public", "customers"),
            fk("billing", "invoices", "public", "customers"),
            fk("public", "orders", "public", "customers"),
            // Same table name, different schema: not a dependent.
            fk("billing", "refunds", "billing", "customers"),
        ];
        let deps: Vec<String> = referencing_tables(&fks, "public", "customers")
            .iter()
            .map(TableRef::display)
            .collect();
        assert_eq!(deps, vec!["billing.invoices", "public.orders"]);
    }

    #[test]
    fn a_driver_without_schemas_matches_on_the_table_name() {
        let fks = vec![fk("", "orders", "", "customers")];
        assert_eq!(referencing_tables(&fks, "", "customers").len(), 1);
        // Reported schema empty, asked-for schema a path: still the same table.
        assert_eq!(
            referencing_tables(&fks, "/tmp/shop.sqlite", "customers").len(),
            1
        );
    }

    #[test]
    fn drop_sql_quotes_and_cascades_only_when_asked() {
        let plain = drop_table_sql(&DbDriver::Postgres, "public", "t", false).unwrap();
        assert!(
            plain[0].contains("DROP TABLE \"public\".\"t\""),
            "{plain:?}"
        );
        assert!(!plain[0].contains("CASCADE"));
        let cascade = drop_table_sql(&DbDriver::Postgres, "public", "t", true).unwrap();
        assert!(cascade[0].contains("CASCADE"));
        assert!(!cascade[0].contains("IF EXISTS"));
    }

    #[test]
    fn drop_sql_keeps_a_hostile_name_inside_the_quotes() {
        let name = "t\"; DROP TABLE users; --";
        let sql = drop_table_sql(&DbDriver::Postgres, "public", name, false).unwrap();
        assert!(
            sql[0].contains("\"t\"\"; DROP TABLE users; --\""),
            "{sql:?}"
        );
    }

    #[test]
    fn system_schemas_are_recognised() {
        assert!(is_system_schema("pg_catalog"));
        assert!(is_system_schema("pg_toast"));
        assert!(is_system_schema("information_schema"));
        assert!(!is_system_schema("public"));
        assert!(!is_system_schema("pgbouncer"));
    }

    // -- Against a real SQLite file ---------------------------------------------

    use crate::drivers::sqlite::SqliteDriver;
    use crate::drivers::ConnectionOptions;

    async fn sqlite(path: &std::path::Path) -> SqliteDriver {
        SqliteDriver::connect(ConnectionOptions {
            host: String::new(),
            port: 0,
            username: String::new(),
            password: None,
            database: Some(path.to_string_lossy().to_string()),
            timeout: Some(5),
            require_tls: false,
        })
        .await
        .unwrap()
    }

    fn temp_db(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dbhive-service-schema-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("t.sqlite")
    }

    #[tokio::test]
    async fn finds_dependents_and_drops_in_a_real_database() {
        let path = temp_db("drop");
        let db = sqlite(&path).await;
        for sql in [
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT)",
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id))",
            "CREATE TABLE scratch (id INTEGER PRIMARY KEY)",
        ] {
            db.execute_query(sql).await.unwrap();
        }
        let schemas = vec!["main".to_string()];

        let deps = find_dependents(&db, &schemas, "main", "customers")
            .await
            .unwrap();
        assert_eq!(
            deps.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["orders"]
        );
        assert!(find_dependents(&db, &schemas, "main", "scratch")
            .await
            .unwrap()
            .is_empty());

        let res = drop_table(&db, &DbDriver::Sqlite, "", "scratch", false)
            .await
            .unwrap();
        assert_eq!(res.sql.len(), 1);
        let tables = db.get_tables("main").await.unwrap();
        assert!(tables.iter().all(|t| t.name != "scratch"));
        assert!(tables.iter().any(|t| t.name == "customers"));

        // Dropping a table that is gone is an error, not a silent no-op.
        assert!(drop_table(&db, &DbDriver::Sqlite, "", "scratch", false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn overview_of_a_sqlite_file_has_one_database_and_no_schema_level() {
        let path = temp_db("overview");
        let db = sqlite(&path).await;
        let o = load_overview(&db, &DbDriver::Sqlite, Some(&path.to_string_lossy())).await;
        assert_eq!(o.databases, vec!["t.sqlite".to_string()]);
        assert!(o.schemas.is_empty());
        assert_eq!(o.database, path.to_string_lossy());
    }
}
