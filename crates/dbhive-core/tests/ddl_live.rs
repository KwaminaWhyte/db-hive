//! The PostgreSQL DDL generator against a real PostgreSQL.
//!
//! Ignored by default: it needs a PostgreSQL on localhost:5432 that accepts the
//! current OS user without a password.
//!
//!     cargo test -p dbhive-core --test ddl_live -- --ignored
//!
//! It never touches an existing database: it creates one with a unique name,
//! works only inside it, and drops it at the end (also if an assertion fails,
//! because the drop runs from a guard).
//!
//! What it proves that the string-level unit tests cannot: every statement the
//! generator writes for create, alter and drop is accepted by a real server, for
//! every column type it offers, a foreign key with an action, a unique and a
//! check constraint, a table name that contains a double quote, and each alter
//! operation; and the catalogue then shows what was asked for.

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::drivers::postgres::PostgresDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};
use dbhive_core::models::ddl::*;
use dbhive_core::models::DbDriver;

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

async fn run(d: &PostgresDriver, what: &str, r: DdlResult) {
    for sql in &r.sql {
        d.execute_query(sql)
            .await
            .unwrap_or_else(|e| panic!("{what}: the server refused:\n{sql}\n{e}"));
    }
}

async fn scalar(d: &PostgresDriver, sql: &str) -> String {
    let r = d
        .execute_query(sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    match &r.rows[0][0] {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[tokio::test]
#[ignore = "needs a local PostgreSQL on localhost:5432 (trust auth)"]
async fn generated_postgres_ddl_is_accepted_and_does_what_it_says() {
    let name = format!("dbhive_ddl_{}", std::process::id());
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
    let d = PostgresDriver::connect(opts(&name))
        .await
        .expect("connect to the scratch database");
    let g = get_ddl_generator(&DbDriver::Postgres).unwrap();

    // -- a parent table ------------------------------------------------------
    let mut id = col("id", ColumnType::Integer);
    id.primary_key = true;
    id.auto_increment = true;
    id.nullable = false;
    let mut email = col("email", ColumnType::Varchar { length: 120 });
    email.nullable = false;
    let owners = TableDefinition {
        schema: Some("public".into()),
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

    // -- a table with every column type, a name with a quote in it -----------
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
        col("c_bytea", ColumnType::Bytea),
        col("c_bool", ColumnType::Boolean),
        col("c_date", ColumnType::Date),
        col("c_time", ColumnType::Time),
        col("c_ts", ColumnType::Timestamp),
        col("c_tstz", ColumnType::TimestampTz),
        col("c_json", ColumnType::Json),
        col("c_jsonb", ColumnType::JsonB),
        col("c_uuid", ColumnType::Uuid),
        col(
            "c_array",
            ColumnType::Array {
                element_type: Box::new(ColumnType::Text),
            },
        ),
        col(
            "c_custom",
            ColumnType::Custom {
                type_name: "inet".into(),
            },
        ),
    ];
    let mut columns = vec![pid, owner, label];
    columns.extend(types);
    let pets = TableDefinition {
        schema: Some("public".into()),
        name: "we\"ird pets".into(),
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
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'we\"ird pets'",
    )
    .await;
    assert_eq!(n_cols, "21", "every requested column should exist");
    let fk_action = scalar(
        &d,
        "SELECT confdeltype::text FROM pg_constraint WHERE conname = 'fk_pet_owner'",
    )
    .await;
    assert_eq!(fk_action, "c", "the foreign key should cascade on delete");
    let uq = scalar(
        &d,
        "SELECT count(*) FROM pg_constraint WHERE conname IN ('uq_pet_label', 'uq_owner_email')",
    )
    .await;
    assert_eq!(uq, "2");
    let ck = scalar(
        &d,
        "SELECT count(*) FROM pg_constraint WHERE conname = 'ck_small' AND contype = 'c'",
    )
    .await;
    assert_eq!(ck, "1");
    let dflt = scalar(
        &d,
        "SELECT column_default FROM information_schema.columns WHERE table_name = 'we\"ird pets' AND column_name = 'label'",
    )
    .await;
    assert!(dflt.contains("unnamed"), "default was {dflt:?}");
    let comment = scalar(&d, "SELECT obj_description('public.owners'::regclass)").await;
    assert_eq!(comment, "People who own things");

    // The constraints work: a missing owner is refused, a delete cascades.
    assert!(
        d.execute_query("INSERT INTO \"we\"\"ird pets\" (owner_id) VALUES (999)")
            .await
            .is_err(),
        "the foreign key should refuse a pet with no owner"
    );
    d.execute_query("INSERT INTO owners (email) VALUES ('a@example.com')")
        .await
        .unwrap();
    d.execute_query("INSERT INTO \"we\"\"ird pets\" (owner_id, c_small) VALUES (1, 3)")
        .await
        .unwrap();
    assert!(
        d.execute_query(
            "INSERT INTO \"we\"\"ird pets\" (owner_id, label, c_small) VALUES (1, 'x', -1)"
        )
        .await
        .is_err(),
        "the check constraint should refuse a negative c_small"
    );

    // -- each alter operation -------------------------------------------------
    let mut added = col("added_at", ColumnType::TimestampTz);
    added.nullable = false;
    added.default = Some("now()".into());
    let alter = AlterTableDefinition {
        schema: Some("public".into()),
        name: "we\"ird pets".into(),
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
            AlterColumnOperation::SetNotNull {
                column_name: "c_double".into(),
                not_null: false,
            },
            AlterColumnOperation::SetDefault {
                column_name: "c_bool".into(),
                default: Some("true".into()),
            },
            AlterColumnOperation::DropColumn {
                column_name: "c_custom".into(),
                cascade: false,
            },
        ],
    };
    run(&d, "alter pets", g.generate_alter_table(&alter).unwrap()).await;
    let cols = scalar(
        &d,
        "SELECT string_agg(column_name, ',' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_name = 'we\"ird pets'",
    )
    .await;
    assert!(
        cols.contains("added_at") && cols.contains("notes"),
        "columns: {cols}"
    );
    assert!(
        !cols.contains("c_custom") && !cols.contains("c_text"),
        "columns: {cols}"
    );
    let int_type = scalar(
        &d,
        "SELECT data_type FROM information_schema.columns WHERE table_name = 'we\"ird pets' AND column_name = 'c_int'",
    )
    .await;
    assert_eq!(int_type, "bigint");
    // The existing row took the new column's default.
    let n = scalar(
        &d,
        "SELECT count(*) FROM \"we\"\"ird pets\" WHERE added_at IS NOT NULL",
    )
    .await;
    assert_eq!(n, "1");

    // -- drop the child, then the parent ---------------------------------------
    let drop_pets = DropTableDefinition {
        schema: Some("public".into()),
        name: "we\"ird pets".into(),
        cascade: false,
        if_exists: true,
    };
    run(&d, "drop pets", g.generate_drop_table(&drop_pets).unwrap()).await;
    let drop_owners = DropTableDefinition {
        schema: Some("public".into()),
        name: "owners".into(),
        cascade: true,
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
        "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .await;
    assert_eq!(left, "0", "both tables should be gone");
}
