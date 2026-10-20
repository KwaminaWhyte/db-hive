//! Scratch: verify the connection-string parse/apply path against a real
//! string, and optionally dial it. Not part of the app; delete after use.

use dbhive_core::models::DbDriver;
use dbhive_ui_model::connection_editor::{apply_connection_string, parse_connection_string, ConnectionForm};

#[tokio::main]
async fn main() {
    let raw = "postgresql://user_32666:dBBqh9Q7SHk0PDLNHhk2DySU@187.124.50.183:5433/user_32666";

    let parsed = parse_connection_string(&DbDriver::Postgres, raw).expect("should parse");
    println!("parsed: {:#?}", parsed);

    let mut form = ConnectionForm::new_for(DbDriver::Postgres);
    let password = apply_connection_string(&mut form, &parsed);
    println!("host={} port={} username={} database={:?}", form.host, form.port, form.username, form.database);
    println!("password captured: {}", password.is_some());

    let opts = dbhive_core::drivers::ConnectionOptions {
        host: form.host.clone(),
        port: form.port.parse().unwrap_or(5432),
        username: form.username.clone(),
        password: password.clone(),
        database: if form.database.is_empty() { None } else { Some(form.database.clone()) },
        timeout: None,
        require_tls: false,
    };
    match dbhive_ui_model::connection_editor::dial(&DbDriver::Postgres, opts).await {
        Ok(()) => println!("dial: OK, connection succeeded"),
        Err(e) => println!("dial: FAILED: {}", e),
    }
}
