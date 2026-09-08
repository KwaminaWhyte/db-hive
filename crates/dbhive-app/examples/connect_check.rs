//! Exercises the exact path the Connect button triggers, without a UI.
//!
//! Loads the saved profiles from disk, connects to the named profile through
//! `dbhive_core::service::connect`, lists its databases, and disconnects.
//!
//!     cargo run -p dbhive-app --example connect_check -- "Local MySQL"

use std::sync::{Arc, Mutex};

use dbhive_core::state::AppState;

#[tokio::main]
async fn main() {
    let wanted = std::env::args().nth(1).unwrap_or_default();

    let state = Arc::new(Mutex::new(AppState::new()));
    dbhive_core::service::load_persisted(&state).expect("failed to load saved data");

    let profiles: Vec<(String, String)> = {
        let g = state.lock().unwrap();
        g.list_profiles()
            .into_iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect()
    };
    println!("loaded {} profiles", profiles.len());

    let Some((id, name)) = profiles.iter().find(|(_, n)| n == &wanted).cloned() else {
        println!("no profile named {:?}; available:", wanted);
        for (_, n) in &profiles {
            println!("  {}", n);
        }
        return;
    };

    println!("connecting to {:?} ...", name);
    match dbhive_core::service::connect(&state, id.clone(), String::new(), None).await {
        Ok(conn_id) => {
            println!("connected, id={}", conn_id);

            let driver = state.lock().unwrap().get_connection(&conn_id).cloned();
            if let Some(driver) = driver {
                match driver.get_databases().await {
                    Ok(dbs) => {
                        println!("databases ({}):", dbs.len());
                        for d in dbs.iter().take(10) {
                            println!("  {}", d.name);
                        }
                    }
                    Err(e) => println!("get_databases failed: {}", e),
                }
            }

            match dbhive_core::service::disconnect(&state, &conn_id).await {
                Ok(()) => println!("disconnected cleanly"),
                Err(e) => println!("disconnect failed: {}", e),
            }
        }
        Err(e) => println!("connect failed: {}", e),
    }
}
