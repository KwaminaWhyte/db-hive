//! The plan path against a real SQLite file: the dialect statement goes to
//! the driver, comes back as rows, and parses into the tree the pane draws.

use std::sync::{Arc, Mutex};

use dbhive_core::models::DbDriver;
use dbhive_core::state::AppState;
use dbhive_ui_model::query_plan;

#[tokio::test]
async fn sqlite_explain_round_trips_through_the_driver_and_parser() {
    let dir = std::env::temp_dir().join(format!("dbhive-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("plan.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT, category TEXT)",
            [],
        )
        .unwrap();
        conn.execute("CREATE INDEX idx_widgets_category ON widgets(category)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO widgets (name, category) VALUES ('a','x'),('b','y')",
            [],
        )
        .unwrap();
    }

    let state = Arc::new(Mutex::new(AppState::new()));
    let profile = dbhive_core::models::ConnectionProfile {
        id: "plan".into(),
        name: "Plan".into(),
        driver: DbDriver::Sqlite,
        host: String::new(),
        port: 0,
        username: String::new(),
        password_keyring_key: None,
        database: Some(db.to_string_lossy().to_string()),
        ssl_mode: Default::default(),
        ssh_tunnel: None,
        folder: None,
        environment: None,
        last_connected_at: None,
        connection_count: 0,
        is_favorite: false,
        color: None,
        description: None,
        created_at: 0,
        updated_at: 0,
    };
    {
        let mut s = state.lock().unwrap();
        s.add_profile(profile);
    }
    dbhive_core::service::connect(&state, "plan".to_string(), String::new(), None)
        .await
        .expect("connect");
    let driver = state
        .lock()
        .unwrap()
        .get_connection("plan")
        .cloned()
        .expect("driver");

    let statement = query_plan::explain_sql(
        DbDriver::Sqlite,
        "SELECT * FROM widgets WHERE category = 'x'",
    )
    .unwrap();
    assert!(statement.starts_with("EXPLAIN QUERY PLAN"));
    let result = driver.execute_query(&statement).await.expect("explain");
    let rows: Vec<Vec<String>> = result
        .rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .collect();
    let plan = query_plan::parse(DbDriver::Sqlite, &result.columns, &rows).expect("parse");
    assert!(!plan.rows.is_empty());
    assert!(plan.summary.contains("steps"));
    assert!(
        plan.rows
            .iter()
            .any(|r| r.title.contains("idx_widgets_category")),
        "{:?}",
        plan.rows.iter().map(|r| &r.title).collect::<Vec<_>>()
    );

    dbhive_core::service::disconnect(&state, "plan").await.ok();
}
