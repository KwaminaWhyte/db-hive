//! A SQL dump written by the app and replayed by the app restores a table
//! exactly, on PostgreSQL and on MySQL.
//!
//! Ignored by default: it needs a PostgreSQL on localhost:5432 (the current OS
//! user, no password) and a MySQL on localhost:3306 (`root`, no password).
//!
//!     cargo test -p dbhive-core --test dump_roundtrip_live -- --ignored --test-threads=1
//!
//! Each test creates a database with a unique name, works only inside it and
//! drops it at the end, also when an assertion fails. Nothing else on the
//! server is read or written.
//!
//! What it proves that the SQLite scenario and the rendering unit tests
//! cannot: `export_to_sql` writes DROP, CREATE and INSERT statements, in each
//! dialect, that `import_from_sql` can replay through the real driver, for
//! values with quotes, backslashes, newlines, NULLs, unicode, numerics,
//! booleans, timestamps and JSON, so that the table afterwards holds exactly
//! the rows it held before.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use dbhive_core::drivers::mysql::MysqlDriver;
use dbhive_core::drivers::postgres::PostgresDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};
use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::service;
use dbhive_core::service_import_export::{
    export_to_sql, import_from_sql, SqlExportOptions, SqlImportOptions,
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

const NOTE: &str = "line1\nline2 \\ back'slash \"dq\" 😀 100% _x_";

/// Fill the table, dump it, damage it, replay the dump, compare.
async fn roundtrip<D: DatabaseDriver>(
    d: &D,
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: &str,
    create: &str,
    tag: &str,
) {
    d.execute_query(create)
        .await
        .unwrap_or_else(|e| panic!("create: {e}"));
    let note = d.escape_string_literal(NOTE);
    // PostgreSQL has a real boolean; MySQL takes 1 and 0.
    let (yes, no) = if tag == "pg" {
        ("true", "false")
    } else {
        ("1", "0")
    };
    for sql in [
        format!(
            "INSERT INTO items (id, name, note, amount, flag, at, meta) VALUES \
             (1, 'plain', 'simple', 12.50, {yes}, '2026-01-02 03:04:05', '{{\"a\": [1, 2]}}')"
        ),
        format!(
            "INSERT INTO items (id, name, note, amount, flag, at, meta) VALUES \
             (2, 'it''s', '{note}', NULL, {no}, NULL, NULL)"
        ),
        "INSERT INTO items (id, name) VALUES (3, '')".to_string(),
    ] {
        d.execute_query(&sql)
            .await
            .unwrap_or_else(|e| panic!("insert: {e}\n{sql}"));
    }
    let before = d
        .execute_query("SELECT * FROM items ORDER BY id")
        .await
        .unwrap();
    assert_eq!(before.rows.len(), 3);

    // Dump it.
    let path = std::env::temp_dir().join(format!("dbhive-dump-{tag}-{}.sql", std::process::id()));
    let n = export_to_sql(
        state,
        connection_id,
        path.to_str().unwrap(),
        SqlExportOptions {
            include_drop: true,
            include_create: true,
            include_data: true,
            tables: vec!["items".to_string()],
            schema: Some(schema.to_string()),
        },
    )
    .await
    .unwrap_or_else(|e| panic!("export: {e}"));
    assert_eq!(n, 1, "one table should have been written");
    let dump = std::fs::read_to_string(&path).unwrap();
    assert!(
        dump.contains("DROP TABLE")
            && dump.contains("CREATE TABLE")
            && dump.contains("INSERT INTO"),
        "dump was:\n{dump}"
    );

    // Damage it: the replay must restore, not merely add.
    d.execute_query("DELETE FROM items WHERE id = 2")
        .await
        .unwrap();
    d.execute_query("UPDATE items SET name = 'changed' WHERE id = 1")
        .await
        .unwrap();

    let result = import_from_sql(
        state,
        connection_id,
        path.to_str().unwrap(),
        SqlImportOptions {
            continue_on_error: false,
            use_transaction: true,
        },
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap_or_else(|e| panic!("import: {e}\ndump was:\n{dump}"));
    assert_eq!(
        result.errors_count, 0,
        "replay errors: {:?}\ndump was:\n{dump}",
        result.first_error
    );

    let after = d
        .execute_query("SELECT * FROM items ORDER BY id")
        .await
        .unwrap();
    assert_eq!(after.columns, before.columns);
    assert_eq!(
        after.rows, before.rows,
        "the table after the replay differs\ndump was:\n{dump}"
    );
    let _ = std::fs::remove_file(path);
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
async fn postgres_dump_replays_to_the_same_rows() {
    // Keep the test's own store writes out of the developer's data directory.
    let dir = std::env::temp_dir().join(format!("dbhive-dump-pg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_dump_{}", std::process::id());
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
    roundtrip(
        &d,
        &state,
        &id,
        "public",
        "CREATE TABLE items (id integer PRIMARY KEY, name text, note text, amount numeric(10,2), flag boolean, at timestamp, meta jsonb)",
        "pg",
    )
    .await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password)"]
async fn mysql_dump_replays_to_the_same_rows() {
    let dir = std::env::temp_dir().join(format!("dbhive-dump-my-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_dump_{}", std::process::id());
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
    roundtrip(
        &d,
        &state,
        &id,
        &name,
        "CREATE TABLE items (id INT PRIMARY KEY, name VARCHAR(100), note TEXT, amount DECIMAL(10,2), flag TINYINT(1), at DATETIME, meta JSON)",
        "my",
    )
    .await;
    let _ = std::fs::remove_dir_all(dir);
}
