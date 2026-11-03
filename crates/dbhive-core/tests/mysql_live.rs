//! The MySQL driver's transactions and the MySQL DDL generator against a real
//! MySQL server.
//!
//! Ignored by default: it needs a MySQL on localhost:3306 that accepts `root`
//! without a password.
//!
//!     cargo test -p dbhive-core --test mysql_live -- --ignored
//!
//! It never touches an existing database: each test creates one with a unique
//! name, works only inside it, and drops it at the end (also if an assertion
//! fails, because the drop runs from a guard). Nothing else on the server is
//! read or written.
//!
//! What it proves that the unit tests cannot: `execute_transaction` on MySQL is
//! one unit (a failure rolls back the earlier statements and is reported by
//! index) and leaves the pool usable; and the statements the MySQL generator
//! writes for create, alter and drop are accepted by a real server.

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::drivers::mysql::MysqlDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};
use dbhive_core::models::ddl::*;
use dbhive_core::models::DbDriver;

fn opts(database: Option<&str>) -> ConnectionOptions {
    ConnectionOptions {
        host: "127.0.0.1".to_string(),
        port: 3306,
        username: "root".to_string(),
        password: None,
        database: database.map(|s| s.to_string()),
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
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                if let Ok(admin) = MysqlDriver::connect(opts(None)).await {
                    let _ = admin
                        .execute_query(&format!("DROP DATABASE IF EXISTS `{name}`"))
                        .await;
                }
            });
        })
        .join();
    }
}

async fn scratch(tag: &str) -> (MysqlDriver, Scratch) {
    let name = format!("dbhive_{tag}_{}", std::process::id());
    let admin = MysqlDriver::connect(opts(None))
        .await
        .expect("connect to mysql");
    admin
        .execute_query(&format!("DROP DATABASE IF EXISTS `{name}`"))
        .await
        .unwrap();
    admin
        .execute_query(&format!("CREATE DATABASE `{name}`"))
        .await
        .expect("create the scratch database");
    let guard = Scratch { name: name.clone() };
    let d = MysqlDriver::connect(opts(Some(&name)))
        .await
        .expect("connect to the scratch database");
    (d, guard)
}

async fn scalar(d: &MysqlDriver, sql: &str) -> String {
    let r = d
        .execute_query(sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    match &r.rows[0][0] {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

async fn count(d: &MysqlDriver) -> i64 {
    scalar(d, "SELECT COUNT(*) FROM t").await.parse().unwrap()
}

#[tokio::test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password)"]
async fn mysql_transactions_commit_roll_back_and_leave_the_pool_usable() {
    let (d, _guard) = scratch("tx").await;
    assert!(d.supports_transactions());
    d.execute_query("CREATE TABLE t (id INT PRIMARY KEY, name VARCHAR(20) NOT NULL) ENGINE=InnoDB")
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

    // The pool is not poisoned by repeated failures, and no connection is left
    // inside an open transaction (a leaked one would make the count see a stale
    // snapshot or hold locks): each round fails, then a plain query and a plain
    // write both work.
    for i in 0..24 {
        let e = d
            .execute_transaction(&["INSERT INTO t VALUES (100, NULL)".to_string()])
            .await
            .unwrap_err();
        assert_eq!(e.index, 0, "round {i}");
        assert_eq!(count(&d).await, 2, "round {i}");
    }
    d.execute_query("INSERT INTO t VALUES (4, 'd')")
        .await
        .unwrap();
    assert_eq!(count(&d).await, 3);
}

fn col(name: &str, t: ColumnType) -> ColumnDefinition {
    ColumnDefinition {
        name: name.to_string(),
        column_type: t,
        nullable: true,
        default: None,
        primary_key: false,
        auto_increment: false,
        comment: None,
    }
}

#[tokio::test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password)"]
async fn generated_mysql_ddl_is_accepted_and_does_what_it_says() {
    let (d, _guard) = scratch("ddl").await;
    let g = get_ddl_generator(&DbDriver::MySql).unwrap();

    async fn run(d: &MysqlDriver, what: &str, r: DdlResult) {
        for sql in &r.sql {
            d.execute_query(sql)
                .await
                .unwrap_or_else(|e| panic!("{what}: the server refused:\n{sql}\n{e}"));
        }
    }

    // -- a parent table ----------------------------------------------------------
    let mut id = col("id", ColumnType::Integer);
    id.primary_key = true;
    id.auto_increment = true;
    id.nullable = false;
    let mut email = col("email", ColumnType::Varchar { length: 120 });
    email.nullable = false;
    let owners = TableDefinition {
        schema: None,
        name: "owners".into(),
        columns: vec![id, email],
        primary_key: None,
        foreign_keys: vec![],
        unique_constraints: vec![UniqueConstraint {
            name: Some("uq_owner_email".into()),
            columns: vec!["email".into()],
        }],
        check_constraints: vec![],
        comment: Some("People who own things".into()),
        if_not_exists: false,
    };
    run(
        &d,
        "create owners",
        g.generate_create_table(&owners).unwrap(),
    )
    .await;

    // -- a child with the column types MySQL has, a name with a backtick -------------
    let mut pid = col("id", ColumnType::BigInt);
    pid.primary_key = true;
    pid.auto_increment = true;
    pid.nullable = false;
    let mut owner = col("owner_id", ColumnType::Integer);
    owner.nullable = false;
    let mut label = col("label", ColumnType::Varchar { length: 30 });
    label.default = Some("'unnamed'".into());
    let types: Vec<ColumnDefinition> = vec![
        col("c_small", ColumnType::SmallInt),
        col("c_int", ColumnType::Integer),
        col(
            "c_dec",
            ColumnType::Decimal {
                precision: 10,
                scale: 2,
            },
        ),
        col("c_real", ColumnType::Real),
        col("c_double", ColumnType::DoublePrecision),
        col("c_char", ColumnType::Char { length: 3 }),
        col("c_text", ColumnType::Text),
        col("c_bool", ColumnType::Boolean),
        col("c_date", ColumnType::Date),
        col("c_time", ColumnType::Time),
        col("c_ts", ColumnType::Timestamp),
        col("c_json", ColumnType::Json),
    ];
    let mut columns = vec![pid, owner, label];
    columns.extend(types);
    let n_expected = columns.len();
    let pets = TableDefinition {
        schema: None,
        name: "we`ird pets".into(),
        columns,
        primary_key: None,
        foreign_keys: vec![ForeignKeyConstraint {
            name: Some("fk_pet_owner".into()),
            columns: vec!["owner_id".into()],
            referenced_table: "owners".into(),
            referenced_columns: vec!["id".into()],
            on_delete: ForeignKeyAction::Cascade,
            on_update: ForeignKeyAction::NoAction,
        }],
        unique_constraints: vec![UniqueConstraint {
            name: Some("uq_pet_label".into()),
            columns: vec!["owner_id".into(), "label".into()],
        }],
        check_constraints: vec![CheckConstraint {
            name: Some("ck_small".into()),
            expression: "c_small IS NULL OR c_small >= 0".into(),
        }],
        comment: None,
        if_not_exists: false,
    };
    run(&d, "create pets", g.generate_create_table(&pets).unwrap()).await;

    let n_cols = scalar(
        &d,
        "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = 'we`ird pets'",
    )
    .await;
    assert_eq!(
        n_cols,
        n_expected.to_string(),
        "every requested column should exist"
    );
    let fk = scalar(
        &d,
        "SELECT delete_rule FROM information_schema.referential_constraints WHERE constraint_schema = DATABASE() AND constraint_name = 'fk_pet_owner'",
    )
    .await;
    assert_eq!(fk, "CASCADE");
    let uq = scalar(
        &d,
        "SELECT COUNT(DISTINCT index_name) FROM information_schema.statistics WHERE table_schema = DATABASE() AND index_name IN ('uq_pet_label', 'uq_owner_email')",
    )
    .await;
    assert_eq!(uq, "2");

    // The constraints work.
    assert!(
        d.execute_query("INSERT INTO `we``ird pets` (owner_id) VALUES (999)")
            .await
            .is_err(),
        "the foreign key should refuse a pet with no owner"
    );
    d.execute_query("INSERT INTO owners (email) VALUES ('a@example.com')")
        .await
        .unwrap();
    d.execute_query("INSERT INTO `we``ird pets` (owner_id, c_small) VALUES (1, 3)")
        .await
        .unwrap();
    assert!(
        d.execute_query(
            "INSERT INTO `we``ird pets` (owner_id, label, c_small) VALUES (1, 'x', -1)"
        )
        .await
        .is_err(),
        "the check constraint should refuse a negative c_small"
    );

    // -- each alter operation ------------------------------------------------------
    let mut added = col("added_at", ColumnType::Timestamp);
    added.nullable = false;
    added.default = Some("CURRENT_TIMESTAMP".into());
    let alter = AlterTableDefinition {
        schema: None,
        name: "we`ird pets".into(),
        operations: vec![
            AlterColumnOperation::AddColumn { column: added },
            AlterColumnOperation::RenameColumn {
                old_name: "c_text".into(),
                new_name: "notes".into(),
            },
            AlterColumnOperation::AlterType {
                column_name: "c_int".into(),
                new_type: ColumnType::BigInt,
            },
            AlterColumnOperation::DropColumn {
                column_name: "c_json".into(),
                cascade: false,
            },
        ],
    };
    run(&d, "alter pets", g.generate_alter_table(&alter).unwrap()).await;
    let cols = scalar(
        &d,
        "SELECT GROUP_CONCAT(column_name ORDER BY ordinal_position) FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = 'we`ird pets'",
    )
    .await;
    assert!(
        cols.contains("added_at") && cols.contains("notes"),
        "columns: {cols}"
    );
    assert!(
        !cols.contains("c_json") && !cols.contains("c_text"),
        "columns: {cols}"
    );
    let int_type = scalar(
        &d,
        "SELECT data_type FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = 'we`ird pets' AND column_name = 'c_int'",
    )
    .await;
    assert_eq!(int_type.to_lowercase(), "bigint");

    // -- drop the child, then the parent ------------------------------------------------
    let drop_pets = DropTableDefinition {
        schema: None,
        name: "we`ird pets".into(),
        cascade: false,
        if_exists: true,
    };
    run(&d, "drop pets", g.generate_drop_table(&drop_pets).unwrap()).await;
    let drop_owners = DropTableDefinition {
        schema: None,
        name: "owners".into(),
        cascade: false,
        if_exists: false,
    };
    run(
        &d,
        "drop owners",
        g.generate_drop_table(&drop_owners).unwrap(),
    )
    .await;
    let left = scalar(
        &d,
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE()",
    )
    .await;
    assert_eq!(left, "0", "both tables should be gone");
}
