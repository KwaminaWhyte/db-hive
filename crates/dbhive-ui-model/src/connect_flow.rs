//! Connecting to a saved profile and noticing when the connection dies.
//!
//! Two decisions the app needs, kept out of any screen:
//!
//! * **Ask for a password, or not.** A saved profile with no secret in the
//!   keyring (and none cached this session) should get a password prompt
//!   instead of a connection attempt with an empty password, which the
//!   drivers turn into opaque errors. [`needs_password`] says which profiles
//!   want one at all; [`PasswordPrompt`] is the dialog's state.
//! * **Is this error the connection, or the query?** A statement that fails
//!   because the socket closed should offer a reconnect, not a "fix your
//!   SQL" error. [`is_connection_lost`] classifies driver messages;
//!   [`ConnectionLost`] is the overlay's state.
//!
//! The password itself only ever lives in [`PasswordPrompt::password`] until
//! it is handed to the connect call. It is never part of a message this
//! module builds.

use dbhive_core::models::{ConnectionProfile, DbDriver};

use crate::connection_editor::driver_fields;

/// Whether connecting `profile` without a stored secret should ask for one.
///
/// * SQLite has no password at all.
/// * MongoDB and Redis run unauthenticated more often than not; they only
///   ask when the profile names a user, which is the sign auth is on.
/// * Everything else (Postgres and friends, MySQL, SQL Server, Turso's auth
///   token) asks. The prompt accepts an empty answer for a server that
///   really has no password.
pub fn needs_password(profile: &ConnectionProfile) -> bool {
    if !driver_fields(&profile.driver).password {
        return false;
    }
    match profile.driver {
        DbDriver::MongoDb | DbDriver::Redis => !profile.username.trim().is_empty(),
        _ => true,
    }
}

/// The "enter the password" dialog for one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordPrompt {
    pub profile_id: String,
    pub profile_name: String,
    /// What has been typed. Never logged, never shown in a message.
    pub password: String,
    /// Keep the password in the OS keychain after a successful connect.
    pub remember: bool,
    /// Show the typed text instead of dots.
    pub show_text: bool,
}

impl PasswordPrompt {
    pub fn new(profile: &ConnectionProfile) -> Self {
        Self {
            profile_id: profile.id.clone(),
            profile_name: profile.name.clone(),
            password: String::new(),
            remember: true,
            show_text: false,
        }
    }

    pub fn title(&self) -> String {
        format!("Password for {}", self.profile_name)
    }

    /// The line under the title. Names the user when there is one, so the
    /// person knows which account the password is for.
    pub fn message(&self, username: &str) -> String {
        let user = username.trim();
        if user.is_empty() {
            "No password is saved for this connection.".to_string()
        } else {
            format!("No password is saved for this connection. Enter the password for {user}.")
        }
    }
}

/// Fragments that only a dead or unreachable connection produces, lower
/// case. Kept specific: a query error that merely mentions a column called
/// `connection` must not trip it.
const LOST_PATTERNS: &[&str] = &[
    // Sockets, every driver.
    "broken pipe",
    "connection reset",
    "connection aborted",
    "connection refused",
    "connection closed",
    "connection was closed",
    "connection is closed",
    "connection has been closed",
    "connection to server was lost",
    "not connected",
    "unexpected eof",
    "unexpected end of file",
    "os error 32",  // EPIPE
    "os error 54",  // ECONNRESET (macOS)
    "os error 104", // ECONNRESET (Linux)
    "os error 10054",
    "network is unreachable",
    "no route to host",
    // PostgreSQL / tokio-postgres.
    "server closed the connection unexpectedly",
    "terminating connection due to administrator command",
    "connection closed by server",
    // MySQL.
    "server has gone away",
    "lost connection to mysql server",
    "lost connection to server",
    // sqlx pools.
    "pool closed",
    "pool timed out",
    "attempted to acquire a connection on a closed pool",
    // SQL Server.
    "connection is not open",
    // MongoDB.
    "connection pool cleared",
    // The shell's own message when the driver handle is gone.
    "connection is no longer open",
];

/// Whether `error` (a driver error's text) means the connection itself is
/// gone, rather than the statement being wrong.
///
/// A timeout set in Settings ("timed out after") is the query, not the
/// connection, and does not count.
pub fn is_connection_lost(error: &str) -> bool {
    let e = error.to_lowercase();
    if e.contains("query timed out") || e.contains("statement timeout") {
        return false;
    }
    LOST_PATTERNS.iter().any(|p| e.contains(p))
}

/// The "connection lost" overlay over a workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionLost {
    pub connection_id: String,
    pub connection_name: String,
    /// The driver's message for the failure that revealed the loss.
    pub error: String,
    pub reconnecting: bool,
    /// Why the last reconnect attempt failed, if it did.
    pub reconnect_error: String,
}

impl ConnectionLost {
    pub fn new(connection_id: &str, connection_name: &str, error: &str) -> Self {
        Self {
            connection_id: connection_id.to_string(),
            connection_name: connection_name.to_string(),
            error: error.trim().to_string(),
            reconnecting: false,
            reconnect_error: String::new(),
        }
    }

    pub fn title(&self) -> String {
        if self.connection_name.is_empty() {
            "Connection lost".to_string()
        } else {
            format!("Connection to {} lost", self.connection_name)
        }
    }

    /// The error to show: the reconnect failure once there is one, since it
    /// is the newer and more useful of the two.
    pub fn shown_error(&self) -> &str {
        if self.reconnect_error.is_empty() {
            &self.error
        } else {
            &self.reconnect_error
        }
    }

    pub fn begin_reconnect(&mut self) -> bool {
        if self.reconnecting {
            return false;
        }
        self.reconnecting = true;
        self.reconnect_error.clear();
        true
    }

    pub fn reconnect_failed(&mut self, error: &str) {
        self.reconnecting = false;
        self.reconnect_error = format!("Reconnect failed: {}", error.trim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(driver: DbDriver, user: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            "id".into(),
            "Prod".into(),
            driver,
            "h".into(),
            1,
            user.into(),
        )
    }

    #[test]
    fn sqlite_never_needs_a_password() {
        assert!(!needs_password(&profile(DbDriver::Sqlite, "")));
        assert!(!needs_password(&profile(DbDriver::Sqlite, "someone")));
    }

    #[test]
    fn server_drivers_need_one() {
        for d in [
            DbDriver::Postgres,
            DbDriver::MySql,
            DbDriver::SqlServer,
            DbDriver::Supabase,
            DbDriver::Neon,
            DbDriver::Turso,
        ] {
            assert!(needs_password(&profile(d.clone(), "")), "{d:?}");
        }
    }

    #[test]
    fn mongo_and_redis_only_with_a_user() {
        for d in [DbDriver::MongoDb, DbDriver::Redis] {
            assert!(!needs_password(&profile(d.clone(), "")), "{d:?}");
            assert!(!needs_password(&profile(d.clone(), "  ")), "{d:?}");
            assert!(needs_password(&profile(d.clone(), "admin")), "{d:?}");
        }
    }

    #[test]
    fn prompt_defaults_to_remembering_and_never_echoes_the_password() {
        let mut p = PasswordPrompt::new(&profile(DbDriver::Postgres, "app"));
        assert!(p.remember);
        assert!(!p.show_text);
        p.password = "hunter2".into();
        assert_eq!(p.title(), "Password for Prod");
        assert!(!p.message("app").contains("hunter2"));
        assert!(p.message("app").contains("for app"));
        assert_eq!(p.message(""), "No password is saved for this connection.");
    }

    #[test]
    fn socket_failures_are_connection_lost() {
        for e in [
            "Query execution failed: error communicating with database: Broken pipe (os error 32)",
            "Query failed: connection closed",
            "Connection reset by peer (os error 54)",
            "db error: FATAL: terminating connection due to administrator command",
            "server closed the connection unexpectedly",
            "Query failed: MySQL server has gone away",
            "Lost connection to MySQL server during query",
            "error returned from database: pool timed out while waiting for an open connection",
            "attempted to acquire a connection on a closed pool",
            "io error: unexpected end of file",
            "This connection is no longer open.",
            "Connection refused (os error 61)",
        ] {
            assert!(is_connection_lost(e), "{e}");
        }
    }

    #[test]
    fn query_errors_are_not() {
        for e in [
            "relation \"missing_table\" does not exist",
            "syntax error at or near \"SELEC\"",
            "column \"connection_closed\" does not exist",
            "duplicate key value violates unique constraint \"users_pkey\"",
            "Query timed out after 30 seconds (Settings > Query timeout)",
            "canceling statement due to statement timeout",
            "",
        ] {
            assert!(!is_connection_lost(e), "{e}");
        }
    }

    #[test]
    fn lost_overlay_tracks_one_reconnect_at_a_time() {
        let mut lost = ConnectionLost::new("c1", "Prod", "  broken pipe ");
        assert_eq!(lost.title(), "Connection to Prod lost");
        assert_eq!(lost.shown_error(), "broken pipe");
        assert!(lost.begin_reconnect());
        assert!(!lost.begin_reconnect(), "a second click does nothing");
        lost.reconnect_failed("connection refused");
        assert!(!lost.reconnecting);
        assert_eq!(lost.shown_error(), "Reconnect failed: connection refused");
        assert!(lost.begin_reconnect());
        assert_eq!(lost.shown_error(), "broken pipe");
    }

    #[test]
    fn untitled_overlay() {
        assert_eq!(ConnectionLost::new("c", "", "x").title(), "Connection lost");
    }
}
