//! `execute_transaction` against a real PostgreSQL.
//!
//! Ignored by default: it needs a PostgreSQL on localhost:5432 that accepts the
//! current OS user without a password.
//!
//!     cargo test -p dbhive-core --test transaction_live -- --ignored
//!
//! It never touches an existing database: it creates one with a unique name,
//! works only inside it, and drops it at the end (also if an assertion fails,
//! because the drop runs from a guard).
//!
//! What it proves that the SQLite tests cannot: the transaction really is one
//! unit on a pooled driver, a failure leaves nothing behind, and (the reason
//! this API exists) a failed transaction does not hand a connection stuck in an
//! aborted transaction back to the pool.

use dbhive_core::drivers::postgres::PostgresDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};

fn opts(database: &str) -> ConnectionOptions {
    ConnectionOptions {
        host: "localhost".to_string(),
        port: 5432,
        username: std::env::var("USER").unwrap_or_else(|_| "postgres".to_string()),
        password: None,
        database: Some(database.to_string()),
        timeout: Some(10),
        require_tls: false,
    }
}

/// Drops the scratch database when the test ends, however it ends.
struct Scratch {
    name: String,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let name = self.name.clone();
        // A fresh runtime on its own thread: this runs in a destructor,
        // possibly during a panic, inside another runtime.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                if let Ok(admin) = PostgresDriver::connect(opts("postgres")).await {
                    let _ = admin
                        .execute_query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                        .await;
                }
            });
        })
        .join();
    }
}

async fn count(d: &PostgresDriver) -> i64 {
    let r = d.execute_query("SELECT COUNT(*) FROM t").await.unwrap();
    r.rows[0][0].as_i64().unwrap()
}

#[tokio::test]
#[ignore = "needs a local PostgreSQL on localhost:5432 (trust auth)"]
async fn postgres_transactions_commit_roll_back_and_leave_the_pool_usable() {
    let name = format!("dbhive_tx_{}", std::process::id());
    let admin = PostgresDriver::connect(opts("postgres"))
        .await
        .expect("connect to postgres");
    admin
        .execute_query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
        .await
        .unwrap();
    admin
        .execute_query(&format!("CREATE DATABASE \"{name}\""))
        .await
        .expect("create the scratch database");
    let _guard = Scratch { name: name.clone() };

    let d = PostgresDriver::connect(opts(&name)).await.unwrap();
    assert!(d.supports_transactions());
    d.execute_query("CREATE TABLE t (id integer PRIMARY KEY, name text NOT NULL)")
        .await
        .unwrap();
    d.execute_query("INSERT INTO t VALUES (1, 'a'), (2, 'b')")
        .await
        .unwrap();

    // Commit: every statement applies, with a count each.
    let affected = d
        .execute_transaction(&[
            "UPDATE t SET name = 'z' WHERE id = 1".to_string(),
            "INSERT INTO t VALUES (3, 'c')".to_string(),
            "DELETE FROM t WHERE id = 2".to_string(),
        ])
        .await
        .unwrap();
    assert_eq!(affected, vec![1, 1, 1]);
    let r = d
        .execute_query("SELECT id, name FROM t ORDER BY id")
        .await
        .unwrap();
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0][1], serde_json::json!("z"));

    // Roll back: the second statement violates NOT NULL, nothing survives.
    let err = d
        .execute_transaction(&[
            "UPDATE t SET name = 'changed' WHERE id = 1".to_string(),
            "INSERT INTO t VALUES (9, NULL)".to_string(),
            "DELETE FROM t".to_string(),
        ])
        .await
        .unwrap_err();
    assert_eq!(err.index, 1);
    let r = d
        .execute_query("SELECT name FROM t WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(r.rows[0][0], serde_json::json!("z"));
    assert_eq!(count(&d).await, 2);

    // The pool is not poisoned: many failed transactions in a row, each
    // followed by a plain query, all behave. A connection returned in an
    // aborted-transaction state would fail the plain query with "current
    // transaction is aborted".
    for i in 0..24 {
        let e = d
            .execute_transaction(&["INSERT INTO t VALUES (100, NULL)".to_string()])
            .await
            .unwrap_err();
        assert_eq!(e.index, 0, "round {i}");
        assert_eq!(count(&d).await, 2, "round {i}");
    }
}
