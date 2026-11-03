//! A value or a name that is quoted by a driver's own helpers comes back from
//! the server exactly as it went in, and cannot break out of its quotes.
//!
//! Every place the app writes SQL from user text (the row editor, data import,
//! the inspector's filters, the DDL generators) goes through
//! `DatabaseDriver::escape_string_literal` and `quote_identifier`. This sends
//! the nasty cases through them for real: embedded quotes, backslashes (which
//! MySQL treats as an escape), a backslash-quote pair, newlines, a statement
//! terminator and comment marker, and non-ASCII; and names with quotes,
//! backticks, brackets, spaces and a semicolon.
//!
//! The SQLite test always runs. PostgreSQL and MySQL need a server on
//! localhost, so they are ignored by default:
//!
//!     cargo test -p dbhive-core --test quoting_roundtrip -- --ignored
//!
//! Those create a database with a unique name, work only inside it and drop it
//! at the end, also when an assertion fails.

use dbhive_core::drivers::mysql::MysqlDriver;
use dbhive_core::drivers::postgres::PostgresDriver;
use dbhive_core::drivers::sqlite::SqliteDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};

const VALUES: &[&str] = &[
    "plain",
    "it's",
    "two''quotes",
    "back\\slash",
    "ends with backslash\\",
    "\\'",
    "\\\\'",
    "a'; DROP TABLE victim; --",
    "line\nbreak\r\nand\ttab",
    "100% _wild_ [x]",
    "\"double\" `tick`",
    "emoji 😀 and ünïcödé 漢字",
    "",
];

const NAMES: &[&str] = &[
    "plain",
    "we\"ird",
    "we`ird",
    "br]acket",
    "sp ace",
    "semi;colon",
    "--dash",
    "x'quote",
];

/// Every value, as a literal in a SELECT, must come back identical, and a
/// table named with each name must be creatable, writable and readable, with a
/// column of the same name.
async fn roundtrip<D: DatabaseDriver>(d: &D) {
    for v in VALUES {
        let sql = format!("SELECT '{}' AS v", d.escape_string_literal(v));
        let r = d
            .execute_query(&sql)
            .await
            .unwrap_or_else(|e| panic!("{v:?}: {e}\n{sql}"));
        assert_eq!(
            r.rows[0][0],
            serde_json::Value::String(v.to_string()),
            "value {v:?} did not survive its own quoting ({sql})"
        );
    }

    d.execute_query("CREATE TABLE victim (id INTEGER)")
        .await
        .unwrap();
    for (i, n) in NAMES.iter().enumerate() {
        let q = d.quote_identifier(n);
        let sql = format!("CREATE TABLE {q} ({q} VARCHAR(50), n INTEGER)");
        d.execute_query(&sql)
            .await
            .unwrap_or_else(|e| panic!("{n:?}: {e}\n{sql}"));
        let v = VALUES[i % VALUES.len()];
        let insert = format!(
            "INSERT INTO {q} ({q}, n) VALUES ('{}', {i})",
            d.escape_string_literal(v)
        );
        d.execute_query(&insert)
            .await
            .unwrap_or_else(|e| panic!("{n:?}: {e}\n{insert}"));
        let r = d
            .execute_query(&format!("SELECT {q} FROM {q} WHERE n = {i}"))
            .await
            .unwrap_or_else(|e| panic!("{n:?}: {e}"));
        assert_eq!(
            r.rows[0][0],
            serde_json::Value::String(v.to_string()),
            "name {n:?}"
        );
    }
    // None of the hostile values dropped the canary table.
    d.execute_query("SELECT id FROM victim")
        .await
        .expect("the canary table must still exist");
}

#[tokio::test]
async fn sqlite_quoting_round_trips() {
    let path = std::env::temp_dir().join(format!("dbhive-quote-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let d = SqliteDriver::connect(ConnectionOptions {
        host: String::new(),
        port: 0,
        username: String::new(),
        password: None,
        database: Some(path.to_string_lossy().to_string()),
        timeout: None,
        require_tls: false,
    })
    .await
    .unwrap();
    roundtrip(&d).await;
    let _ = std::fs::remove_file(path);
}

fn server_opts(host: &str, port: u16, user: &str, database: Option<&str>) -> ConnectionOptions {
    ConnectionOptions {
        host: host.to_string(),
        port,
        username: user.to_string(),
        password: None,
        database: database.map(|s| s.to_string()),
        timeout: Some(10),
        require_tls: false,
    }
}

fn pg_opts(database: &str) -> ConnectionOptions {
    let user = std::env::var("USER").unwrap_or_else(|_| "postgres".to_string());
    server_opts("localhost", 5432, &user, Some(database))
}

fn my_opts(database: Option<&str>) -> ConnectionOptions {
    server_opts("127.0.0.1", 3306, "root", database)
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

#[tokio::test]
#[ignore = "needs a local PostgreSQL on localhost:5432 (trust auth)"]
async fn postgres_quoting_round_trips() {
    let name = format!("dbhive_quote_{}", std::process::id());
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
    let d = PostgresDriver::connect(pg_opts(&name)).await.unwrap();
    roundtrip(&d).await;
}

#[tokio::test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password)"]
async fn mysql_quoting_round_trips() {
    let name = format!("dbhive_quote_{}", std::process::id());
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
    let d = MysqlDriver::connect(my_opts(Some(&name))).await.unwrap();
    roundtrip(&d).await;
}
