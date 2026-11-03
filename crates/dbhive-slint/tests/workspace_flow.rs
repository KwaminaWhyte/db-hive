//! Exercises the real path behind "add a connection, then view its tables":
//! `service::connect` against a live SQLite file, `get_tables`,
//! `execute_query`, and `workspace_model::to_grid_rows` on the result.
//!
//! This is the one place that proves the driver calls `app.rs` makes
//! actually return what the workspace screen expects; everything else about
//! that flow is unit-tested against fakes or fixed data.

use std::sync::Mutex;

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::workspace_model;

fn temp_db_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "dbhive-slint-test-{}-{}.sqlite",
        std::process::id(),
        name
    ))
}

#[tokio::test]
async fn connect_list_tables_and_read_rows_from_a_real_sqlite_file() {
    let path = temp_db_path("workspace-flow");
    let _ = std::fs::remove_file(&path);

    // `service::connect` saves the profile store. Without this the test
    // overwrites the developer's real profiles.json with its one profile.
    let data_dir = std::env::temp_dir().join(format!("dbhive-slint-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &data_dir);

    // Seed a database the way a user's own file would look: one table, one
    // NULL to prove it survives the round trip distinct from the word NULL.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT, note TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO widgets (name, note) VALUES ('left-handed sprocket', 'in stock'), ('gizmo', NULL)",
            [],
        )
        .unwrap();
    }

    let mut profile = ConnectionProfile::new(
        "widgets-db".to_string(),
        "Widgets".to_string(),
        DbDriver::Sqlite,
        String::new(),
        0,
        String::new(),
    );
    profile.database = Some(path.to_string_lossy().to_string());

    let state = Mutex::new(AppState::new());
    state.lock().unwrap().add_profile(profile.clone());

    let conn_id = dbhive_core::service::connect(&state, profile.id.clone(), String::new(), None)
        .await
        .expect("connect should succeed against a file it can create");
    assert_eq!(conn_id, profile.id);

    let driver = state
        .lock()
        .unwrap()
        .get_connection(&conn_id)
        .unwrap()
        .clone();

    let database = workspace_model::connected_database(&profile).unwrap();
    let schema_arg = workspace_model::tables_schema_arg(&DbDriver::Sqlite, &database, None, &[]);
    let tables = driver.get_tables(&schema_arg).await.unwrap();
    assert_eq!(
        tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec!["widgets"],
        "sidebar should list exactly the one table in the file"
    );

    let sql = format!(
        "SELECT * FROM {} ORDER BY id",
        driver.quote_identifier("widgets")
    );
    let result = driver.execute_query(&sql).await.unwrap();
    assert_eq!(result.columns, vec!["id", "name", "note"]);

    let grid_rows = workspace_model::to_grid_rows(&result);
    assert_eq!(
        grid_rows,
        vec![
            vec![
                Some("1".to_string()),
                Some("left-handed sprocket".to_string()),
                Some("in stock".to_string())
            ],
            vec![Some("2".to_string()), Some("gizmo".to_string()), None],
        ],
        "a database NULL must come through as None, not the text \"NULL\""
    );

    dbhive_core::service::disconnect(&state, &conn_id)
        .await
        .unwrap();
    assert!(
        data_dir.join("profiles.json").exists(),
        "connect saved its profile store under DBHIVE_DATA_DIR, not the real data dir"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&data_dir);
}
