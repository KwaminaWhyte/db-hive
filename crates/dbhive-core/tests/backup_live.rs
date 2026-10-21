//! Backup and restore against a real PostgreSQL and a real MySQL.
//!
//! Ignored by default: it needs a PostgreSQL on localhost:5432 (the current OS
//! user, no password) with `pg_dump`, `psql`, `dropdb` and `createdb` on the
//! PATH, and a MySQL on localhost:3306 (`root`, no password) with `mysqldump`
//! and `mysql`.
//!
//!     cargo test -p dbhive-core --test backup_live -- --ignored --test-threads=1
//!
//! Each test creates a database with a unique name, works only inside it and
//! drops it at the end, also when an assertion fails. Nothing else on the
//! server is read or written.
//!
//! What it proves that the option-validation unit tests cannot: a backup file
//! written by the real dump tool restores the rows exactly, including when
//! the restore drops and recreates the database while the app itself still has
//! the connection open (which is how the Backup screen is used), and a restore
//! that hits errors is reported as a failure, not as success.

use dbhive_core::drivers::mysql::MysqlDriver;
use dbhive_core::drivers::postgres::PostgresDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};
use dbhive_core::models::backup::{BackupOptions, RestoreOptions};
use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::service;
use dbhive_core::service_backup::{create_backup, restore_backup, ProgressSink};
use dbhive_core::state::AppState;
use std::sync::Mutex;

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
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                if postgres {
                    if let Ok(a) = PostgresDriver::connect(pg_opts("postgres")).await {
                        let _ = a
                            .execute_query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                            .await;
                    }
                } else if let Ok(a) = MysqlDriver::connect(my_opts(None)).await {
                    let _ = a.execute_query(&format!("DROP DATABASE IF EXISTS `{name}`")).await;
                }
            });
        })
        .join();
    }
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

/// The rows of the test table, read through the app's own connection (the
/// one a restore has to get out from under).
async fn rows_via_app(state: &Mutex<AppState>, id: &str) -> Vec<Vec<serde_json::Value>> {
    let d = state.lock().unwrap().get_connection(id).cloned().expect("connected");
    d.execute_query("SELECT id, name, note FROM items ORDER BY id")
        .await
        .unwrap_or_else(|e| panic!("read through the app's connection: {e}"))
        .rows
}

async fn scenario<D: DatabaseDriver>(d: &D, state: &Mutex<AppState>, id: &str, create: &str, tag: &str) {
    d.execute_query(create).await.unwrap();
    let note = d.escape_string_literal("tab\there 'quoted' \\ back \"dq\" 😀\nnewline");
    d.execute_query(&format!(
        "INSERT INTO items (id, name, note) VALUES (1, 'one', '{note}'), (2, 'two', NULL), (3, 'three', '')"
    ))
    .await
    .unwrap();
    let before = rows_via_app(state, id).await;
    assert_eq!(before.len(), 3);

    let out_dir = std::env::temp_dir().join(format!("dbhive-backup-{tag}-{}", std::process::id()));
    let entry = create_backup(
        state,
        id,
        BackupOptions {
            include_data: true,
            include_schema: true,
            tables: vec![],
            note: Some("live test".to_string()),
            output_dir: Some(out_dir.to_string_lossy().to_string()),
        },
        ProgressSink::default(),
    )
    .await
    .unwrap_or_else(|e| panic!("backup: {e}"));
    assert!(entry.size_bytes > 0, "the backup file is empty: {entry:?}");

    // Damage the database, then restore over it while the app is still connected.
    d.execute_query("DELETE FROM items WHERE id = 2").await.unwrap();
    d.execute_query("UPDATE items SET name = 'changed' WHERE id = 1").await.unwrap();

    restore_backup(
        state,
        id,
        RestoreOptions { file_path: entry.file_path.clone(), drop_existing: true },
        ProgressSink::default(),
    )
    .await
    .unwrap_or_else(|e| panic!("restore with drop_existing while connected: {e}"));

    // The app's connection must still work afterwards (or reconnect), and hold the original rows.
    let mut after = None;
    for _ in 0..3 {
        let d = state.lock().unwrap().get_connection(id).cloned().unwrap();
        if let Ok(r) = d.execute_query("SELECT id, name, note FROM items ORDER BY id").await {
            after = Some(r.rows);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    let after = after.expect("the app's own connection should still work after a restore");
    assert_eq!(after, before, "the restored table differs from the backed-up one");

    // A restore that hits errors must say so, not report success. A dump with a
    // statement the server rejects, after statements that would apply, must
    // fail (the client tools can exit 0 after a failed statement), and must not
    // leave the earlier statements half-applied where it can be avoided.
    let broken = out_dir.join("broken.sql");
    std::fs::write(
        &broken,
        "UPDATE items SET name = 'half applied' WHERE id = 1;\nTHIS IS NOT SQL AT ALL;\n",
    )
    .unwrap();
    let again = restore_backup(
        state,
        id,
        RestoreOptions { file_path: broken.to_string_lossy().to_string(), drop_existing: false },
        ProgressSink::default(),
    )
    .await;
    assert!(
        again.is_err(),
        "a restore file with a rejected statement must be reported as a failure, not success"
    );
    let _ = std::fs::remove_dir_all(out_dir);
}

#[tokio::test]
#[ignore = "needs a local PostgreSQL on localhost:5432 (trust auth) and the pg tools"]
async fn postgres_backup_restores_the_rows_and_failures_are_reported() {
    let dir = std::env::temp_dir().join(format!("dbhive-backup-pgdata-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_bak_{}", std::process::id());
    let admin = PostgresDriver::connect(pg_opts("postgres")).await.expect("connect to postgres");
    admin.execute_query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)")).await.unwrap();
    admin.execute_query(&format!("CREATE DATABASE \"{name}\"")).await.unwrap();
    let _guard = Scratch { postgres: true, name: name.clone() };

    let mut p = ConnectionProfile::new("pg".into(), "PG".into(), DbDriver::Postgres, "localhost".into(), 5432, user());
    p.database = Some(name.clone());
    let (state, id) = state_with(p).await;
    let d = PostgresDriver::connect(pg_opts(&name)).await.unwrap();
    scenario(&d, &state, &id, "CREATE TABLE items (id integer PRIMARY KEY, name text, note text)", "pg").await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password) and mysqldump"]
async fn mysql_backup_restores_the_rows_and_failures_are_reported() {
    let dir = std::env::temp_dir().join(format!("dbhive-backup-mydata-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_bak_{}", std::process::id());
    let admin = MysqlDriver::connect(my_opts(None)).await.expect("connect to mysql");
    admin.execute_query(&format!("DROP DATABASE IF EXISTS `{name}`")).await.unwrap();
    admin.execute_query(&format!("CREATE DATABASE `{name}`")).await.unwrap();
    let _guard = Scratch { postgres: false, name: name.clone() };

    let mut p = ConnectionProfile::new("my".into(), "MY".into(), DbDriver::MySql, "127.0.0.1".into(), 3306, "root".into());
    p.database = Some(name.clone());
    let (state, id) = state_with(p).await;
    let d = MysqlDriver::connect(my_opts(Some(&name))).await.unwrap();
    scenario(&d, &state, &id, "CREATE TABLE items (id INT PRIMARY KEY, name VARCHAR(50), note TEXT)", "my").await;
    let _ = std::fs::remove_dir_all(dir);
}
