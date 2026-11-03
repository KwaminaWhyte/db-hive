//! The data import (CSV, spreadsheet rows into a table) against a real
//! PostgreSQL and a real MySQL.
//!
//! Ignored by default: it needs a PostgreSQL on localhost:5432 (the current OS
//! user, no password) and a MySQL on localhost:3306 (`root`, no password).
//!
//!     cargo test -p dbhive-core --test import_live -- --ignored --test-threads=1
//!
//! Each test creates a database with a unique name, works only inside it and
//! drops it at the end, also when an assertion fails. Nothing else on the
//! server is read or written.
//!
//! What it proves that the SQLite scenario and the plan-building unit tests
//! cannot: rows made of hostile text (quotes, backslashes, newlines, unicode, a
//! statement terminator) are written as data, in several batches, into columns
//! of integer, text, numeric, boolean and date types; empty cells become NULL
//! or the mapped default; truncate-before empties the table first; a skipped
//! column is left alone; and a table created from the file's columns is
//! accepted by the server and holds every row.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use dbhive_core::drivers::mysql::MysqlDriver;
use dbhive_core::drivers::postgres::PostgresDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};
use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::service;
use dbhive_core::service_import_export::{
    run_import, ColumnMapping, DataImportOptions, ImportSource,
};
use dbhive_core::state::AppState;

fn user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "postgres".to_string())
}

fn pg_opts(database: &str) -> ConnectionOptions {
    ConnectionOptions {
        host: "localhost".into(),
        port: 5432,
        username: user(),
        password: None,
        database: Some(database.to_string()),
        timeout: Some(10),
        require_tls: false,
    }
}

fn my_opts(database: Option<&str>) -> ConnectionOptions {
    ConnectionOptions {
        host: "127.0.0.1".into(),
        port: 3306,
        username: "root".into(),
        password: None,
        database: database.map(|s| s.to_string()),
        timeout: Some(10),
        require_tls: false,
    }
}

struct Scratch {
    postgres: bool,
    name: String,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let (postgres, name) = (self.postgres, self.name.clone());
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                if postgres {
                    if let Ok(a) = PostgresDriver::connect(pg_opts("postgres")).await {
                        let _ = a
                            .execute_query(&format!(
                                "DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"
                            ))
                            .await;
                    }
                } else if let Ok(a) = MysqlDriver::connect(my_opts(None)).await {
                    let _ = a
                        .execute_query(&format!("DROP DATABASE IF EXISTS `{name}`"))
                        .await;
                }
            });
        })
        .join();
    }
}

fn map(source: &str, target: &str, ty: &str) -> ColumnMapping {
    ColumnMapping {
        source_column: source.to_string(),
        target_column: target.to_string(),
        target_type: Some(ty.to_string()),
        default_value: None,
        skip: false,
    }
}

fn source() -> ImportSource {
    let rows: Vec<Vec<&str>> = vec![
        vec!["1", "plain", "12.50", "true", "2026-01-02", "ignored"],
        vec!["2", "it's", "0.5", "false", "2026-02-03", "ignored"],
        vec!["3", "back\\slash\\", "", "true", "", "ignored"],
        vec!["4", "\\'", "7", "false", "2026-03-04", "ignored"],
        vec![
            "5",
            "line1\nline2 😀",
            "99999.99",
            "true",
            "2026-04-05",
            "ignored",
        ],
        vec![
            "6",
            "a'; DROP TABLE items; --",
            "1",
            "false",
            "2026-05-06",
            "ignored",
        ],
        vec!["7", "", "2", "true", "2026-06-07", "ignored"],
    ];
    ImportSource {
        columns: ["id", "name", "amount", "flag", "day", "junk"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        rows: rows
            .into_iter()
            .map(|r| r.into_iter().map(|s| s.to_string()).collect())
            .collect(),
    }
}

async fn scalar<D: DatabaseDriver>(d: &D, sql: &str) -> String {
    let r = d
        .execute_query(sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    match &r.rows[0][0] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "NULL".to_string(),
        other => other.to_string(),
    }
}

async fn scenario<D: DatabaseDriver>(
    d: &D,
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: Option<&str>,
    create_items: &str,
) {
    d.execute_query(create_items)
        .await
        .unwrap_or_else(|e| panic!("create: {e}"));
    // A row that truncate-before must remove.
    d.execute_query("INSERT INTO items (id, name) VALUES (100, 'leftover')")
        .await
        .unwrap();

    // -- into an existing table, several batches, one column skipped ---------------------
    let mut junk = map("junk", "", "TEXT");
    junk.skip = true;
    let options = DataImportOptions {
        table_name: "items".to_string(),
        schema: schema.map(|s| s.to_string()),
        column_mappings: vec![
            map("id", "id", "INTEGER"),
            map("name", "name", "TEXT"),
            map("amount", "amount", "DECIMAL"),
            map("flag", "flag", "BOOLEAN"),
            map("day", "day", "DATE"),
            junk,
        ],
        truncate_before: true,
        batch_size: 3,
        ..DataImportOptions::default()
    };
    let r = run_import(
        state,
        connection_id,
        Arc::new(source()),
        options,
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap_or_else(|e| panic!("import: {e}"));
    assert!(r.success && r.rows_failed == 0, "import result: {r:?}");
    assert_eq!(r.rows_imported, 7);

    assert_eq!(
        scalar(d, "SELECT COUNT(*) FROM items").await,
        "7",
        "truncate-before should drop the leftover row"
    );
    assert_eq!(
        scalar(d, "SELECT name FROM items WHERE id = 2").await,
        "it's"
    );
    assert_eq!(
        scalar(d, "SELECT name FROM items WHERE id = 3").await,
        "back\\slash\\"
    );
    assert_eq!(
        scalar(d, "SELECT name FROM items WHERE id = 4").await,
        "\\'"
    );
    assert_eq!(
        scalar(d, "SELECT name FROM items WHERE id = 5").await,
        "line1\nline2 😀"
    );
    assert_eq!(
        scalar(d, "SELECT name FROM items WHERE id = 6").await,
        "a'; DROP TABLE items; --"
    );
    // An empty cell is NULL for a number and a date, '' stays '' for text.
    assert_eq!(
        scalar(d, "SELECT amount FROM items WHERE id = 3").await,
        "NULL"
    );
    assert_eq!(
        scalar(d, "SELECT day FROM items WHERE id = 3").await,
        "NULL"
    );
    // (The importer may treat an empty text cell as NULL or as ''; both are data, not an error.)
    let empty = scalar(d, "SELECT name FROM items WHERE id = 7").await;
    assert!(
        empty.is_empty() || empty == "NULL",
        "empty text cell became {empty:?}"
    );
    assert_eq!(
        scalar(d, "SELECT COUNT(*) FROM items WHERE flag").await,
        "4"
    );

    // -- a new table made from the file's columns -------------------------------------------
    let options = DataImportOptions {
        table_name: "made_from_file".to_string(),
        schema: schema.map(|s| s.to_string()),
        column_mappings: vec![
            map("id", "id", "INTEGER"),
            map("name", "name", "TEXT"),
            map("amount", "amount", "DECIMAL"),
            map("flag", "flag", "BOOLEAN"),
            map("day", "day", "DATE"),
        ],
        create_table: true,
        batch_size: 1,
        ..DataImportOptions::default()
    };
    let r = run_import(
        state,
        connection_id,
        Arc::new(source()),
        options,
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap_or_else(|e| panic!("import into a new table: {e}"));
    assert!(r.success && r.rows_failed == 0, "import result: {r:?}");
    assert_eq!(scalar(d, "SELECT COUNT(*) FROM made_from_file").await, "7");
    assert_eq!(
        scalar(d, "SELECT name FROM made_from_file WHERE id = 4").await,
        "\\'"
    );

    // The canary: the hostile text did not drop the table.
    assert_eq!(scalar(d, "SELECT COUNT(*) FROM items").await, "7");
}

async fn state_with(profile: ConnectionProfile) -> (Mutex<AppState>, String) {
    let id = profile.id.clone();
    let mut s = AppState::new();
    s.add_profile(profile);
    let state = Mutex::new(s);
    service::connect(&state, id.clone(), String::new(), None)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    (state, id)
}

#[tokio::test]
#[ignore = "needs a local PostgreSQL on localhost:5432 (trust auth)"]
async fn postgres_import_writes_hostile_text_as_data() {
    let dir = std::env::temp_dir().join(format!("dbhive-import-pg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_imp_{}", std::process::id());
    let admin = PostgresDriver::connect(pg_opts("postgres"))
        .await
        .expect("connect to postgres");
    admin
        .execute_query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
        .await
        .unwrap();
    admin
        .execute_query(&format!("CREATE DATABASE \"{name}\""))
        .await
        .unwrap();
    let _guard = Scratch {
        postgres: true,
        name: name.clone(),
    };

    let mut p = ConnectionProfile::new(
        "pg".into(),
        "PG".into(),
        DbDriver::Postgres,
        "localhost".into(),
        5432,
        user(),
    );
    p.database = Some(name.clone());
    let (state, id) = state_with(p).await;
    let d = PostgresDriver::connect(pg_opts(&name)).await.unwrap();
    scenario(
        &d,
        &state,
        &id,
        Some("public"),
        "CREATE TABLE items (id integer PRIMARY KEY, name text, amount numeric(10,2), flag boolean, day date)",
    )
    .await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password)"]
async fn mysql_import_writes_hostile_text_as_data() {
    let dir = std::env::temp_dir().join(format!("dbhive-import-my-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_imp_{}", std::process::id());
    let admin = MysqlDriver::connect(my_opts(None))
        .await
        .expect("connect to mysql");
    admin
        .execute_query(&format!("DROP DATABASE IF EXISTS `{name}`"))
        .await
        .unwrap();
    admin
        .execute_query(&format!("CREATE DATABASE `{name}`"))
        .await
        .unwrap();
    let _guard = Scratch {
        postgres: false,
        name: name.clone(),
    };

    let mut p = ConnectionProfile::new(
        "my".into(),
        "MY".into(),
        DbDriver::MySql,
        "127.0.0.1".into(),
        3306,
        "root".into(),
    );
    p.database = Some(name.clone());
    let (state, id) = state_with(p).await;
    let d = MysqlDriver::connect(my_opts(Some(&name))).await.unwrap();
    scenario(
        &d,
        &state,
        &id,
        Some(&name),
        "CREATE TABLE items (id INT PRIMARY KEY, name VARCHAR(100), amount DECIMAL(10,2), flag TINYINT(1), day DATE)",
    )
    .await;
    let _ = std::fs::remove_dir_all(dir);
}
