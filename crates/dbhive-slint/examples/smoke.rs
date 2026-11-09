//! End-to-end smoke test against a real database, with no UI.
//!
//! Exercises the paths the screens depend on, so a regression in the shared
//! core is caught without needing to click through the app.
//!
//!     cargo run -p dbhive-slint --example smoke -- "Local MySQL"

use std::sync::{Arc, Mutex};

use dbhive_core::state::AppState;

macro_rules! step {
    ($ok:expr, $label:expr) => {{
        let ok = $ok;
        println!("  [{}] {}", if ok { "pass" } else { "FAIL" }, $label);
        ok
    }};
}

#[tokio::main]
async fn main() {
    let wanted = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Local MySQL".into());
    let mut failures = 0;

    let state = Arc::new(Mutex::new(AppState::new()));

    println!("persistence");
    failures += !step!(
        dbhive_core::service::load_persisted(&state).is_ok(),
        "load saved profiles, history and snippets"
    ) as i32;

    let profiles: Vec<(String, String)> = {
        let g = state.lock().unwrap();
        g.list_profiles()
            .into_iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect()
    };
    failures += !step!(!profiles.is_empty(), "at least one saved profile") as i32;

    println!("settings");
    failures += !step!(
        dbhive_core::settings::load().is_ok(),
        "settings load (or default) without error"
    ) as i32;

    println!("updater");
    failures += !step!(
        dbhive_core::service_updater::is_newer("0.22.0", "0.21.0-beta"),
        "version comparison"
    ) as i32;

    let Some((id, name)) = profiles.iter().find(|(_, n)| n == &wanted).cloned() else {
        println!(
            "\nno profile named {:?}; skipping the database steps",
            wanted
        );
        std::process::exit(if failures == 0 { 0 } else { 1 });
    };

    println!("connection: {}", name);
    let connected = dbhive_core::service::connect(&state, id.clone(), String::new(), None).await;
    failures += !step!(connected.is_ok(), "connect") as i32;
    if let Err(e) = &connected {
        println!("      {}", e);
        std::process::exit(1);
    }

    let driver = state.lock().unwrap().get_connection(&id).cloned().unwrap();

    let dbs = driver.get_databases().await;
    failures += !step!(
        dbs.as_ref().map(|d| !d.is_empty()).unwrap_or(false),
        "list databases"
    ) as i32;

    let q = driver.execute_query("SELECT 1 AS one").await;
    failures += !step!(
        q.as_ref().map(|r| r.rows.len() == 1).unwrap_or(false),
        "execute a query"
    ) as i32;

    let bad = driver
        .execute_query("SELECT * FROM a_table_that_does_not_exist")
        .await;
    failures += !step!(
        bad.is_err(),
        "a bad query reports an error rather than empty success"
    ) as i32;

    println!("metadata");
    let first_db = dbs.map(|d| d[0].name.clone()).unwrap_or_default();
    let tables = driver.get_tables(&first_db).await;
    failures += !step!(tables.is_ok(), "list tables") as i32;

    println!("teardown");
    failures += !step!(
        dbhive_core::service::disconnect(&state, &id).await.is_ok(),
        "disconnect"
    ) as i32;
    failures += !step!(
        state.lock().unwrap().get_connection(&id).is_none(),
        "connection removed from state"
    ) as i32;

    println!();
    if failures == 0 {
        println!("all steps passed");
    } else {
        println!("{} step(s) FAILED", failures);
        std::process::exit(1);
    }
}
