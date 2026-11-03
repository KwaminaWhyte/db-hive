//! Shell-agnostic operations.
//!
//! The Tauri command handlers were almost entirely portable already: their
//! bodies talk to `AppState` and the drivers, and only their signatures were
//! tied to Tauri (`State<'_, Mutex<AppState>>`, an often-unused `AppHandle`).
//!
//! This module holds those bodies, taking a plain `&Mutex<AppState>`. Both
//! shells call in here, so connecting from the native UI and connecting from
//! the Tauri UI cannot drift apart.

use std::sync::{Arc, Mutex};

use crate::credentials::CredentialManager;
use crate::drivers::{
    mongodb::MongoDbDriver, mysql::MysqlDriver, postgres::PostgresDriver, redis::RedisDriver,
    sqlite::SqliteDriver, sqlserver::SqlServerDriver, turso::TursoDriver, ConnectionOptions,
    DatabaseDriver,
};
use crate::models::connection::SshAuthMethod;
use crate::models::{DbDriver, DbError, DdlResult, SslMode};
use crate::state::AppState;

/// Open a connection for a saved profile and register it in state.
///
/// Returns the connection id, which is the profile id: connections are keyed
/// by profile so reconnecting reuses the same slot rather than leaking a new
/// one each time.
pub async fn connect(
    state: &Mutex<AppState>,
    profile_id: String,
    password: String,
    ssh_password: Option<String>,
) -> Result<String, DbError> {
    let profile = {
        let guard = state.lock().unwrap();
        guard
            .get_profile(&profile_id)
            .ok_or_else(|| DbError::NotFound(format!("Profile with ID {} not found", profile_id)))?
            .clone()
    };

    // An empty password means the caller had nothing cached; fall back to the
    // keyring, then to this session's in-memory copy.
    let password = if password.is_empty() {
        match CredentialManager::get_password(&profile_id).ok().flatten() {
            Some(p) => p,
            None => state
                .lock()
                .unwrap()
                .connection_passwords
                .get(&profile_id)
                .cloned()
                .unwrap_or_default(),
        }
    } else {
        password
    };
    let had_no_password = password.is_empty();

    // With a tunnel configured the driver connects to the local forwarded
    // port instead of the real host.
    let (actual_host, actual_port) = if let Some(ssh_config) = &profile.ssh_tunnel {
        let tunnel_manager = state.lock().unwrap().ssh_tunnel_manager.clone();
        let ssh_auth_password = match ssh_config.auth_method {
            SshAuthMethod::Password => ssh_password.clone(),
            SshAuthMethod::PrivateKey => None,
        };
        let local_port = tunnel_manager
            .create_tunnel(
                profile_id.clone(),
                ssh_config,
                ssh_auth_password,
                profile.host.clone(),
                profile.port,
            )
            .await?;
        ("127.0.0.1".to_string(), local_port)
    } else {
        (profile.host.clone(), profile.port)
    };

    // PostgreSQL needs *some* database to connect to; "postgres" always exists.
    let database = if profile.driver.is_postgres_compatible() {
        match &profile.database {
            None => Some("postgres".to_string()),
            Some(d) if d.is_empty() => Some("postgres".to_string()),
            other => other.clone(),
        }
    } else {
        profile.database.clone()
    };

    let opts = ConnectionOptions {
        host: actual_host,
        port: actual_port,
        username: profile.username.clone(),
        password: Some(password.clone()),
        database,
        timeout: Some(30),
        require_tls: matches!(profile.driver, DbDriver::Supabase | DbDriver::Neon)
            || (profile.driver.is_postgres_compatible() && profile.ssl_mode == SslMode::Require),
    };

    let connected: Result<Arc<dyn DatabaseDriver>, DbError> = match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => PostgresDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
        DbDriver::Sqlite => SqliteDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
        DbDriver::MySql => MysqlDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
        DbDriver::MongoDb => MongoDbDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
        DbDriver::SqlServer => SqlServerDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
        DbDriver::Turso => TursoDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
        DbDriver::Redis => RedisDriver::connect(opts)
            .await
            .map(|d| Arc::new(d) as Arc<dyn DatabaseDriver>),
    };

    // A server that wants a password and got none produces a driver-level
    // message with no hint about the real problem: tokio-postgres, for one,
    // just says "invalid configuration". Say what actually happened, so the
    // caller can prompt instead of leaving the user guessing.
    let connection = match connected {
        Ok(c) => c,
        Err(e) if had_no_password => {
            return Err(DbError::AuthError(format!(
                "No password available for this connection, and the server refused the \
                 connection without one. Enter the password and try again. ({})",
                e
            )))
        }
        Err(e) => return Err(e),
    };

    {
        let mut guard = state.lock().unwrap();
        guard.add_connection(profile_id.clone(), connection);
        guard
            .connection_passwords
            .insert(profile_id.clone(), password.clone());
    }

    // Credential and metadata persistence are best-effort: a working
    // connection should not be thrown away because the keyring refused.
    if !password.is_empty() {
        if let Err(e) = CredentialManager::save_password(&profile_id, &password) {
            eprintln!("Warning: Failed to save password to keyring: {}", e);
        }
    }
    if let Some(ssh_pwd) = ssh_password.as_ref().filter(|p| !p.is_empty()) {
        if let Err(e) = CredentialManager::save_ssh_password(&profile_id, ssh_pwd) {
            eprintln!("Warning: Failed to save SSH password to keyring: {}", e);
        }
    }
    if let Err(e) = record_connection(state, &profile_id) {
        eprintln!("Warning: Failed to record connection metadata: {}", e);
    }

    Ok(profile_id)
}

/// Stamp a successful connection onto the profile and persist it.
pub fn record_connection(state: &Mutex<AppState>, profile_id: &str) -> Result<(), DbError> {
    let mut guard = state.lock().unwrap();
    let profile = guard
        .get_profile_mut(profile_id)
        .ok_or_else(|| DbError::NotFound(format!("Profile with ID {} not found", profile_id)))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    profile.last_connected_at = Some(now);
    profile.connection_count += 1;
    profile.updated_at = now;

    guard.save_profiles_to_store()
}

/// Close a connection and tear down its tunnel.
///
/// The cached password is deliberately kept so reconnecting does not prompt
/// again in the same session.
pub async fn disconnect(state: &Mutex<AppState>, connection_id: &str) -> Result<(), DbError> {
    let connection = {
        let mut guard = state.lock().unwrap();
        guard.remove_connection(connection_id).ok_or_else(|| {
            DbError::NotFound(format!("Connection with ID {} not found", connection_id))
        })?
    };

    connection.close().await?;

    let tunnel_manager = state.lock().unwrap().ssh_tunnel_manager.clone();
    if tunnel_manager.has_tunnel(connection_id).await {
        tunnel_manager.close_tunnel(connection_id).await?;
    }
    Ok(())
}

/// Load everything persisted on disk into a fresh `AppState`.
pub fn load_persisted(state: &Mutex<AppState>) -> Result<(), DbError> {
    let mut guard = state.lock().unwrap();
    guard.load_profiles_from_store()?;
    guard.load_history_from_store()?;
    guard.load_snippets_from_store()?;
    Ok(())
}

/// Switch an open connection to a different database.
///
/// This reconnects rather than issuing a USE statement, because the drivers
/// bind a database at connect time: `MysqlDriver::get_tables` ignores its
/// schema argument entirely and reports `self.current_database`
/// (`drivers/mysql.rs:168`), so nothing short of a new connection changes
/// what the sidebar lists.
///
/// The connection id is preserved, so anything holding it keeps working.
pub async fn switch_database(
    state: &Mutex<AppState>,
    connection_id: &str,
    new_database: &str,
) -> Result<String, DbError> {
    let (profile, password) = {
        let guard = state.lock().unwrap();
        guard.get_connection(connection_id).ok_or_else(|| {
            DbError::NotFound(format!("Connection with ID {} not found", connection_id))
        })?;
        let profile = guard
            .get_profile(connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!(
                    "Profile for connection {} not found",
                    connection_id
                ))
            })?
            .clone();
        let password = guard
            .connection_passwords
            .get(connection_id)
            .cloned()
            .ok_or_else(|| DbError::AuthError("Password not found for connection".to_string()))?;
        (profile, password)
    };

    // Reuse the live tunnel if there is one rather than opening a second.
    let (actual_host, actual_port) = if profile.ssh_tunnel.is_some() {
        let tunnel_manager = state.lock().unwrap().ssh_tunnel_manager.clone();
        match tunnel_manager.get_local_port(connection_id).await {
            Some(local_port) => ("127.0.0.1".to_string(), local_port),
            None => (profile.host.clone(), profile.port),
        }
    } else {
        (profile.host.clone(), profile.port)
    };

    let opts = ConnectionOptions {
        host: actual_host,
        port: actual_port,
        username: profile.username.clone(),
        password: Some(password),
        database: Some(new_database.to_string()),
        timeout: Some(30),
        require_tls: matches!(profile.driver, DbDriver::Supabase | DbDriver::Neon)
            || (profile.driver.is_postgres_compatible() && profile.ssl_mode == SslMode::Require),
    };

    let new_connection: Arc<dyn DatabaseDriver> = match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            Arc::new(PostgresDriver::connect(opts).await?)
        }
        DbDriver::Sqlite => Arc::new(SqliteDriver::connect(opts).await?),
        DbDriver::MySql => Arc::new(MysqlDriver::connect(opts).await?),
        DbDriver::MongoDb => Arc::new(MongoDbDriver::connect(opts).await?),
        DbDriver::Turso => Arc::new(TursoDriver::connect(opts).await?),
        DbDriver::Redis => Arc::new(RedisDriver::connect(opts).await?),
        DbDriver::SqlServer => {
            return Err(DbError::InternalError(
                "Database switching not supported for this driver".to_string(),
            ))
        }
    };

    // Only drop the old connection once the new one is established, so a
    // failed switch leaves the user still connected.
    let old = state.lock().unwrap().remove_connection(connection_id);
    if let Some(old) = old {
        let _ = old.close().await;
    }
    state
        .lock()
        .unwrap()
        .add_connection(connection_id.to_string(), new_connection);

    Ok(connection_id.to_string())
}

// ---------------------------------------------------------------------------
// Databases
// ---------------------------------------------------------------------------

/// Longest database name any supported server accepts.
///
/// Postgres and MySQL both cut off at 64 bytes, Postgres counting the
/// terminator, which leaves 63. Rejecting here says so plainly instead of
/// letting the server truncate silently or fail with a byte offset.
pub const MAX_DATABASE_NAME_LEN: usize = 63;

/// Reject a database name that no server would accept, before any SQL is
/// built.
///
/// The name is quoted before it reaches SQL, so this is not what stops an
/// injection; it exists so the user gets a sentence instead of a syntax
/// error, and so a name that quoting would happily accept but the server
/// would refuse (empty, 200 characters long, leading digit) is caught here.
pub fn validate_database_name(name: &str) -> Result<(), DbError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbError::InvalidInput(
            "Database name cannot be empty".to_string(),
        ));
    }
    if name.chars().count() > MAX_DATABASE_NAME_LEN {
        return Err(DbError::InvalidInput(format!(
            "Database name too long (max {} characters)",
            MAX_DATABASE_NAME_LEN
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '$')
    {
        return Err(DbError::InvalidInput(
            "Database name may only contain letters, digits, underscores, hyphens, and $"
                .to_string(),
        ));
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(DbError::InvalidInput(
            "Database name cannot start with a digit".to_string(),
        ));
    }
    Ok(())
}

/// `CREATE DATABASE <name>` for drivers that have such a statement.
///
/// The name is quoted with the driver's own quoting rather than interpolated,
/// so a hyphenated or reserved name works. Drivers where a database is not a
/// thing you create with SQL say why: the message is the whole answer the
/// user gets, and "unsupported" alone would leave them guessing.
pub fn create_database_sql(
    driver: &DbDriver,
    name: &str,
    quote: &dyn Fn(&str) -> String,
) -> Result<String, DbError> {
    validate_database_name(name)?;
    match driver {
        DbDriver::Postgres
        | DbDriver::Supabase
        | DbDriver::Neon
        | DbDriver::MySql
        | DbDriver::SqlServer => Ok(format!("CREATE DATABASE {}", quote(name.trim()))),
        DbDriver::Sqlite => Err(DbError::InvalidInput(
            "SQLite databases are individual files and cannot be created from an active connection"
                .to_string(),
        )),
        DbDriver::MongoDb => Err(DbError::InvalidInput(
            "MongoDB databases are created implicitly when the first collection is written"
                .to_string(),
        )),
        DbDriver::Turso => Err(DbError::InvalidInput(
            "Turso databases are provisioned via the Turso platform, not via SQL".to_string(),
        )),
        DbDriver::Redis => Err(DbError::InvalidInput(
            "Redis exposes a fixed set of numbered logical databases (0..15); no CREATE DATABASE"
                .to_string(),
        )),
    }
}

/// Create a database on the server this connection is already talking to.
///
/// Returns the SQL that ran, so the caller can show it: DDL is not undoable
/// and the statement is the only record of what happened.
pub async fn create_database(
    state: &Mutex<AppState>,
    connection_id: &str,
    name: &str,
) -> Result<DdlResult, DbError> {
    let (connection, driver) = {
        let guard = state.lock().unwrap();
        let driver = guard
            .get_profile(connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!(
                    "Connection profile for '{}' not found",
                    connection_id
                ))
            })?
            .driver
            .clone();
        let connection = guard
            .get_connection(connection_id)
            .ok_or_else(|| DbError::NotFound(format!("Connection '{}' not found", connection_id)))?
            .clone();
        (connection, driver)
    };

    let quote = |s: &str| connection.quote_identifier(s);
    let sql = create_database_sql(&driver, name, &quote)?;
    connection.execute_query(&sql).await?;

    Ok(DdlResult {
        message: format!("Database '{}' created", name.trim()),
        sql: vec![sql],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard SQL quoting, as Postgres uses.
    fn q(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
    /// MySQL backtick quoting, to prove nothing assumes double quotes.
    fn q_mysql(ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }
    /// T-SQL bracket quoting.
    fn q_mssql(ident: &str) -> String {
        format!("[{}]", ident.replace(']', "]]"))
    }

    #[test]
    fn a_nameless_database_is_rejected() {
        assert!(validate_database_name("").is_err());
        assert!(validate_database_name("   ").is_err());
    }

    #[test]
    fn a_database_name_is_length_limited() {
        let ok = "a".repeat(MAX_DATABASE_NAME_LEN);
        assert!(validate_database_name(&ok).is_ok());
        let too_long = "a".repeat(MAX_DATABASE_NAME_LEN + 1);
        assert!(validate_database_name(&too_long).is_err());
    }

    #[test]
    fn a_database_name_cannot_start_with_a_digit() {
        assert!(validate_database_name("1db").is_err());
        assert!(validate_database_name("db1").is_ok());
    }

    #[test]
    fn punctuation_that_would_change_the_statement_is_rejected() {
        for bad in ["my db", "db;DROP DATABASE x", "db\"", "db'", "db`", "db.x"] {
            assert!(validate_database_name(bad).is_err(), "accepted {:?}", bad);
        }
        // The characters servers do allow still pass.
        for good in ["my_db", "my-db", "db$1", "DB"] {
            assert!(validate_database_name(good).is_ok(), "rejected {:?}", good);
        }
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_rather_than_rejected() {
        assert!(validate_database_name("  analytics  ").is_ok());
        let sql = create_database_sql(&DbDriver::Postgres, "  analytics  ", &q).unwrap();
        assert_eq!(sql, "CREATE DATABASE \"analytics\"");
    }

    #[test]
    fn the_name_is_quoted_per_dialect() {
        assert_eq!(
            create_database_sql(&DbDriver::Postgres, "my-db", &q).unwrap(),
            "CREATE DATABASE \"my-db\""
        );
        assert_eq!(
            create_database_sql(&DbDriver::MySql, "my-db", &q_mysql).unwrap(),
            "CREATE DATABASE `my-db`"
        );
        assert_eq!(
            create_database_sql(&DbDriver::SqlServer, "my-db", &q_mssql).unwrap(),
            "CREATE DATABASE [my-db]"
        );
    }

    #[test]
    fn postgres_flavours_all_create_databases() {
        for driver in [DbDriver::Postgres, DbDriver::Supabase, DbDriver::Neon] {
            assert!(
                create_database_sql(&driver, "analytics", &q).is_ok(),
                "{:?}",
                driver
            );
        }
    }

    #[test]
    fn drivers_without_create_database_explain_themselves() {
        for driver in [
            DbDriver::Sqlite,
            DbDriver::Turso,
            DbDriver::MongoDb,
            DbDriver::Redis,
        ] {
            let err = create_database_sql(&driver, "analytics", &q).unwrap_err();
            let msg = err.to_string();
            // Not just "unsupported": the message has to say what to do
            // instead, since it is all the user sees.
            assert!(msg.len() > 40, "{:?}: {}", driver, msg);
        }
    }

    #[test]
    fn an_invalid_name_is_caught_before_any_sql_is_built() {
        assert!(create_database_sql(&DbDriver::Postgres, "1bad", &q).is_err());
        assert!(create_database_sql(&DbDriver::Postgres, "", &q).is_err());
    }
}
