//! Connection profile form logic, lifted from the egui `connection_editor`
//! screen. See lib.rs.
//!
//! Everything that decides *what* the form means: which fields a driver
//! wants, whether the form is valid, what profile it produces, how a pasted
//! connection string maps onto fields, and the test / save operations. The
//! shells only draw.

use std::sync::{Arc, Mutex};

use dbhive_core::credentials::CredentialManager;
use dbhive_core::drivers::{
    mongodb::MongoDbDriver, mysql::MysqlDriver, postgres::PostgresDriver, redis::RedisDriver,
    sqlite::SqliteDriver, sqlserver::SqlServerDriver, turso::TursoDriver, ConnectionOptions,
    DatabaseDriver,
};
use dbhive_core::models::connection::{Environment, SshAuthMethod, SshConfig};
use dbhive_core::models::{ConnectionProfile, DbDriver, DbError, SslMode};
use dbhive_core::state::AppState;

/// Every driver the profile model knows about, in the order the picker shows
/// them. Kept as a list rather than derived, because the display order is a
/// UI decision and `DbDriver` is a core type.
pub const ALL_DRIVERS: [DbDriver; 9] = [
    DbDriver::Postgres,
    DbDriver::MySql,
    DbDriver::Sqlite,
    DbDriver::MongoDb,
    DbDriver::SqlServer,
    DbDriver::Supabase,
    DbDriver::Neon,
    DbDriver::Turso,
    DbDriver::Redis,
];

// ===========================================================================
// Form state
// ===========================================================================

/// The dialog's editable state.
///
/// Numeric fields are held as strings so a half-typed port ("") is
/// representable and can be reported as a validation error rather than
/// silently snapping to 0, which is what a bound `u16` would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionForm {
    /// `None` for a new profile, `Some(id)` when editing an existing one.
    pub id: Option<String>,
    pub name: String,
    pub driver: DbDriver,
    /// Host, or the libSQL URL for Turso.
    pub host: String,
    pub port: String,
    pub username: String,
    /// Database name, the SQLite file path, or the Redis database index.
    pub database: String,
    pub ssl_mode: SslMode,
    pub folder: String,
    pub environment: Option<Environment>,
    /// Hex colour tag, e.g. `#3b82f6`. Empty means none.
    pub color: String,
    pub description: String,
    pub is_favorite: bool,

    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_username: String,
    pub ssh_auth_method: SshAuthMethod,
    pub ssh_private_key_path: String,
    /// 0 (or empty) means "pick a free local port".
    pub ssh_local_port: String,

    /// Carried through an edit so saving does not reset the profile's history.
    pub created_at: Option<i64>,
    pub last_connected_at: Option<i64>,
    pub connection_count: u32,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            driver: DbDriver::Postgres,
            host: "localhost".to_string(),
            port: "5432".to_string(),
            username: String::new(),
            database: String::new(),
            ssl_mode: SslMode::Prefer,
            folder: String::new(),
            environment: None,
            color: String::new(),
            description: String::new(),
            is_favorite: false,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_auth_method: SshAuthMethod::Password,
            ssh_private_key_path: String::new(),
            ssh_local_port: "0".to_string(),
            created_at: None,
            last_connected_at: None,
            connection_count: 0,
        }
    }
}

impl ConnectionForm {
    /// A blank form for a driver, with that driver's default port filled in.
    pub fn new_for(driver: DbDriver) -> Self {
        let mut form = Self {
            ssl_mode: default_ssl_mode(&driver),
            ..Self::default()
        };
        form.set_driver(driver);
        form
    }

    /// Switch drivers, moving the port to the new driver's default when the
    /// old value was still the previous default. A port the user actually
    /// typed is left alone.
    pub fn set_driver(&mut self, driver: DbDriver) {
        let was_default = self.port.trim().is_empty()
            || self.port.trim() == default_port(&self.driver).to_string();
        if was_default {
            let port = default_port(&driver);
            self.port = if port == 0 {
                String::new()
            } else {
                port.to_string()
            };
        }
        if self.ssl_mode == default_ssl_mode(&self.driver) {
            self.ssl_mode = default_ssl_mode(&driver);
        }
        self.driver = driver;
    }
}

/// Populate a form from a saved profile, for the edit path.
pub fn form_from_profile(profile: &ConnectionProfile) -> ConnectionForm {
    let ssh = profile.ssh_tunnel.clone();
    ConnectionForm {
        id: Some(profile.id.clone()),
        name: profile.name.clone(),
        driver: profile.driver.clone(),
        host: profile.host.clone(),
        port: profile.port.to_string(),
        username: profile.username.clone(),
        database: profile.database.clone().unwrap_or_default(),
        ssl_mode: profile.ssl_mode.clone(),
        folder: profile.folder.clone().unwrap_or_default(),
        environment: profile.environment.clone(),
        color: profile.color.clone().unwrap_or_default(),
        description: profile.description.clone().unwrap_or_default(),
        is_favorite: profile.is_favorite,
        ssh_enabled: ssh.is_some(),
        ssh_host: ssh.as_ref().map(|s| s.host.clone()).unwrap_or_default(),
        ssh_port: ssh
            .as_ref()
            .map(|s| s.port.to_string())
            .unwrap_or_else(|| "22".to_string()),
        ssh_username: ssh.as_ref().map(|s| s.username.clone()).unwrap_or_default(),
        ssh_auth_method: ssh
            .as_ref()
            .map(|s| s.auth_method.clone())
            .unwrap_or(SshAuthMethod::Password),
        ssh_private_key_path: ssh
            .as_ref()
            .and_then(|s| s.private_key_path.clone())
            .unwrap_or_default(),
        ssh_local_port: ssh
            .as_ref()
            .map(|s| s.local_port.to_string())
            .unwrap_or_else(|| "0".to_string()),
        created_at: Some(profile.created_at),
        last_connected_at: profile.last_connected_at,
        connection_count: profile.connection_count,
    }
}

// ===========================================================================
// Driver-specific field logic
// ===========================================================================

/// Which inputs a driver actually wants.
///
/// SQLite is a file, Turso is a URL, and the rest are host/port servers; the
/// React form branched on the driver in six separate places for this, which
/// is exactly the sort of thing that drifts. It is one function here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverFields {
    /// SQLite: the "database" field holds a filesystem path.
    pub file_path: bool,
    /// Turso: the "host" field holds a libSQL URL, and there is no port.
    pub url: bool,
    pub host_port: bool,
    pub username: bool,
    pub username_required: bool,
    pub password: bool,
    pub database: bool,
    pub ssl: bool,
    pub ssh: bool,
}

pub fn driver_fields(driver: &DbDriver) -> DriverFields {
    match driver {
        DbDriver::Sqlite => DriverFields {
            file_path: true,
            url: false,
            host_port: false,
            username: false,
            username_required: false,
            password: false,
            database: true,
            ssl: false,
            ssh: false,
        },
        DbDriver::Turso => DriverFields {
            file_path: false,
            url: true,
            host_port: false,
            // Turso authenticates with an auth token in the password slot,
            // so there is nothing to put in a username.
            username: false,
            username_required: false,
            password: true,
            database: false,
            ssl: false,
            ssh: false,
        },
        // MongoDB and Redis both allow unauthenticated local servers.
        DbDriver::MongoDb | DbDriver::Redis => DriverFields {
            file_path: false,
            url: false,
            host_port: true,
            username: true,
            username_required: false,
            password: true,
            database: true,
            ssl: false,
            ssh: true,
        },
        DbDriver::Postgres
        | DbDriver::MySql
        | DbDriver::SqlServer
        | DbDriver::Supabase
        | DbDriver::Neon => DriverFields {
            file_path: false,
            url: false,
            host_port: true,
            username: true,
            username_required: true,
            password: true,
            database: true,
            ssl: true,
            ssh: true,
        },
    }
}

/// Default port for a driver. Delegates to the core model so the native shell
/// and the web shell cannot disagree about what "default" means.
pub fn default_port(driver: &DbDriver) -> u16 {
    ConnectionProfile::default_port_for_driver(driver)
}

/// Supabase and Neon terminate TLS at the edge and reject plaintext, so they
/// start at Require; everything else starts at the model default.
pub fn default_ssl_mode(driver: &DbDriver) -> SslMode {
    match driver {
        DbDriver::Supabase | DbDriver::Neon => SslMode::Require,
        _ => SslMode::default(),
    }
}

pub fn database_label(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::Sqlite => "Database file",
        DbDriver::Redis => "Database index",
        _ => "Database name",
    }
}

pub fn password_label(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::Turso => "Auth token",
        _ => "Password",
    }
}

pub fn connection_string_placeholder(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::Postgres => "postgresql://user:password@host:5432/database",
        DbDriver::Supabase => {
            "postgresql://postgres:password@db.<project>.supabase.co:5432/postgres"
        }
        DbDriver::Neon => "postgresql://user:password@ep-<id>.neon.tech/neondb",
        DbDriver::MySql => "mysql://user:password@host:3306/database",
        DbDriver::MongoDb => "mongodb://user:password@host:27017/database",
        DbDriver::SqlServer => "mssql://user:password@host:1433/database",
        DbDriver::Turso => "libsql://my-db-org.turso.io",
        DbDriver::Redis => "redis://:password@host:6379/0",
        DbDriver::Sqlite => "",
    }
}

pub fn ssl_mode_label(mode: &SslMode) -> &'static str {
    match mode {
        SslMode::Disable => "Disabled",
        SslMode::Prefer => "Preferred",
        SslMode::Require => "Required",
    }
}

pub fn environment_label(env: Option<&Environment>) -> &'static str {
    match env {
        None => "None",
        Some(Environment::Local) => "Local",
        Some(Environment::Staging) => "Staging",
        Some(Environment::Production) => "Production",
    }
}

// ===========================================================================
// Validation
// ===========================================================================

/// Per-field validation messages. Empty means the form can be saved.
///
/// Errors are per-field rather than a single banner so each message can sit
/// under the input that caused it; a banner saying "Host is required" above a
/// form with three host-ish fields is not actionable.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Validation {
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
    pub username: Option<String>,
    pub database: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<String>,
    pub ssh_username: Option<String>,
    pub ssh_private_key_path: Option<String>,
}

impl Validation {
    pub fn is_valid(&self) -> bool {
        *self == Validation::default()
    }

    /// Every message, for callers that want a summary rather than inline text.
    pub fn messages(&self) -> Vec<&str> {
        [
            &self.name,
            &self.host,
            &self.port,
            &self.username,
            &self.database,
            &self.ssh_host,
            &self.ssh_port,
            &self.ssh_username,
            &self.ssh_private_key_path,
        ]
        .into_iter()
        .filter_map(|m| m.as_deref())
        .collect()
    }
}

/// Parse a port field. `None` on anything that is not a usable TCP port.
///
/// 0 is rejected: it is "let the OS choose" for a listener, which is
/// meaningless for the port you are dialling out to.
pub fn parse_port(raw: &str) -> Option<u16> {
    match raw.trim().parse::<u32>() {
        Ok(p) if (1..=65535).contains(&p) => Some(p as u16),
        _ => None,
    }
}

pub fn validate(form: &ConnectionForm) -> Validation {
    let fields = driver_fields(&form.driver);
    let mut v = Validation::default();

    if form.name.trim().is_empty() {
        v.name = Some("Connection name is required".to_string());
    }

    if fields.file_path {
        if form.database.trim().is_empty() {
            v.database = Some("Database file path is required for SQLite".to_string());
        }
    } else if fields.url {
        if form.host.trim().is_empty() {
            v.host = Some("libSQL URL is required".to_string());
        }
    } else {
        if form.host.trim().is_empty() {
            v.host = Some("Host is required".to_string());
        }
        if parse_port(&form.port).is_none() {
            v.port = Some("Port must be between 1 and 65535".to_string());
        }
        if fields.username_required && form.username.trim().is_empty() {
            v.username = Some("Username is required".to_string());
        }
    }

    if fields.ssh && form.ssh_enabled {
        if form.ssh_host.trim().is_empty() {
            v.ssh_host = Some("SSH host is required".to_string());
        }
        if form.ssh_username.trim().is_empty() {
            v.ssh_username = Some("SSH username is required".to_string());
        }
        if parse_port(&form.ssh_port).is_none() {
            v.ssh_port = Some("SSH port must be between 1 and 65535".to_string());
        }
        if form.ssh_auth_method == SshAuthMethod::PrivateKey
            && form.ssh_private_key_path.trim().is_empty()
        {
            v.ssh_private_key_path = Some("Private key path is required".to_string());
        }
    }

    v
}

// ===========================================================================
// Profile construction
// ===========================================================================

fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build the SSH config the form describes, or `None` when tunnelling is off
/// or the driver does not support it.
pub fn build_ssh_config(form: &ConnectionForm) -> Option<SshConfig> {
    if !form.ssh_enabled || !driver_fields(&form.driver).ssh {
        return None;
    }
    Some(SshConfig {
        host: form.ssh_host.trim().to_string(),
        port: parse_port(&form.ssh_port).unwrap_or(22),
        username: form.ssh_username.trim().to_string(),
        auth_method: form.ssh_auth_method.clone(),
        private_key_path: match form.ssh_auth_method {
            SshAuthMethod::PrivateKey => opt(&form.ssh_private_key_path),
            SshAuthMethod::Password => None,
        },
        key_passphrase_keyring_key: None,
        // A blank local port means "auto-assign", which the tunnel manager
        // already spells 0.
        local_port: form.ssh_local_port.trim().parse::<u16>().unwrap_or(0),
    })
}

/// Turn form state into the profile that gets persisted.
///
/// `id` is the profile id to use; the caller supplies a fresh UUID for a new
/// profile so this stays a pure function.
pub fn build_profile_with(form: &ConnectionForm, id: String, now: i64) -> ConnectionProfile {
    let fields = driver_fields(&form.driver);
    ConnectionProfile {
        id,
        name: form.name.trim().to_string(),
        driver: form.driver.clone(),
        host: form.host.trim().to_string(),
        // SQLite has no port; storing the stale default would only confuse
        // whoever reads the JSON later.
        port: if fields.host_port || fields.url {
            parse_port(&form.port).unwrap_or_else(|| default_port(&form.driver))
        } else {
            0
        },
        username: form.username.trim().to_string(),
        // The password lives in the OS keyring, never in the profile.
        password_keyring_key: None,
        database: opt(&form.database),
        ssl_mode: form.ssl_mode.clone(),
        ssh_tunnel: build_ssh_config(form),
        folder: opt(&form.folder),
        environment: form.environment.clone(),
        last_connected_at: form.last_connected_at,
        connection_count: form.connection_count,
        is_favorite: form.is_favorite,
        color: opt(&form.color),
        description: opt(&form.description),
        created_at: form.created_at.unwrap_or(now),
        updated_at: now,
    }
}

/// Convenience wrapper that allocates an id for a new profile and stamps the
/// current time.
pub fn build_profile(form: &ConnectionForm) -> ConnectionProfile {
    let id = form
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    build_profile_with(form, id, now_secs())
}

// ===========================================================================
// Connection string parsing
// ===========================================================================

/// The parts a pasted connection string can contribute.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedConnectionString {
    pub username: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
}

fn schemes_for(driver: &DbDriver) -> &'static [&'static str] {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => &["postgres", "postgresql"],
        DbDriver::MySql => &["mysql", "mariadb"],
        DbDriver::MongoDb => &["mongodb", "mongodb+srv"],
        DbDriver::SqlServer => &["mssql", "sqlserver"],
        DbDriver::Redis => &["redis", "rediss"],
        DbDriver::Turso => &["libsql", "wss", "https"],
        DbDriver::Sqlite => &[],
    }
}

/// Parse a `scheme://user:pass@host:port/database?params` URI.
///
/// Hand-rolled rather than pulled from a URL crate: the shell has no `url`
/// dependency, the grammar needed here is one line of splitting, and the
/// failure mode we care about (a string for the wrong driver) is a scheme
/// check the URL crates would not do for us anyway.
pub fn parse_connection_string(driver: &DbDriver, raw: &str) -> Option<ParsedConnectionString> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (scheme, rest) = raw.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !schemes_for(driver).contains(&scheme.as_str()) {
        return None;
    }

    // Drop the query string: none of the fields on this form come from it.
    let rest = rest.split('?').next().unwrap_or("");
    let (authority, database) = match rest.split_once('/') {
        Some((a, d)) => (a, opt(d)),
        None => (rest, None),
    };

    // rsplit: a password may itself contain '@'.
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };

    let (username, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((user, pass)) => (opt(user), opt(pass)),
            None => (opt(u), None),
        },
        None => (None, None),
    };

    // Only treat a trailing `:digits` as a port; `:` also shows up in IPv6
    // literals, which we leave as part of the host.
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            (h, parse_port(p))
        }
        _ => (hostport, None),
    };

    let host = opt(host)?;

    Some(ParsedConnectionString {
        username,
        password,
        host: Some(host),
        port,
        database,
    })
}

/// Apply a parsed connection string over the form, leaving fields the string
/// did not mention untouched. Returns the password, if the string carried one,
/// so the caller can put it in the (separately held) password field.
pub fn apply_connection_string(
    form: &mut ConnectionForm,
    parsed: &ParsedConnectionString,
) -> Option<String> {
    if let Some(h) = &parsed.host {
        form.host = h.clone();
    }
    if let Some(p) = parsed.port {
        form.port = p.to_string();
    }
    if let Some(u) = &parsed.username {
        form.username = u.clone();
    }
    if let Some(d) = &parsed.database {
        form.database = d.clone();
    }
    parsed.password.clone()
}

/// Result of pasting a connection string into the editor.
pub struct PastedConnection {
    /// The password the string carried, for the separately held password field.
    pub password: Option<String>,
}

/// Fill the form from a pasted connection string.
///
/// `Err` carries the message to show: the string is not a URI, or its scheme
/// belongs to another driver (e.g. `mysql://` pasted into a Postgres form).
pub fn paste_connection_string(
    form: &mut ConnectionForm,
    raw: &str,
) -> Result<PastedConnection, String> {
    let Some(parsed) = parse_connection_string(&form.driver, raw) else {
        let hint = connection_string_placeholder(&form.driver);
        return Err(format!("Not a connection string for this driver. Expected e.g. {hint}"));
    };
    let password = apply_connection_string(form, &parsed);
    Ok(PastedConnection { password })
}

/// Colour tags offered in the editor. A tag is user data stored on the profile
/// as hex, so these are not theme tokens.
pub const COLOR_PRESETS: [&str; 6] = [
    "#ef4444", "#f59e0b", "#22c55e", "#3b82f6", "#a855f7", "#ec4899",
];

/// Which preset a profile's colour is, or `None` for no tag or a custom hex.
pub fn color_preset_index(color: &str) -> Option<usize> {
    let c = color.trim();
    COLOR_PRESETS.iter().position(|p| p.eq_ignore_ascii_case(c))
}

/// Parse a `#rrggbb` / `#rgb` colour tag into `(r, g, b)`. Profile colour
/// tags are user data, not theme colours, so they are read from the profile
/// rather than the palette.
pub fn parse_hex_color(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim().trim_start_matches('#');
    let expand = |c: char| -> Option<u8> { c.to_digit(16).map(|d| (d * 17) as u8) };
    match s.len() {
        3 => {
            let mut it = s.chars();
            let r = expand(it.next()?)?;
            let g = expand(it.next()?)?;
            let b = expand(it.next()?)?;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

// ===========================================================================
// Async operations
// ===========================================================================

/// Outcome of a save, plus the keyring warning if the profile stored fine but
/// the secret did not.
pub type SaveOutcome = Result<(String, Option<String>), DbError>;

/// Dial the profile without registering it as a live connection.
///
/// Mirrors the Tauri `test_connection_command`: bring up a throwaway tunnel if
/// one is configured, connect, ping, then tear the tunnel down again whatever
/// the outcome.
pub async fn test_profile(
    state: Arc<Mutex<AppState>>,
    profile: ConnectionProfile,
    password: String,
    ssh_password: Option<String>,
) -> Result<(), DbError> {
    let (host, port, temp_tunnel) = if let Some(ssh) = &profile.ssh_tunnel {
        let temp_id = format!("test-{}", uuid::Uuid::new_v4());
        let manager = {
            let guard = state.lock().unwrap();
            guard.ssh_tunnel_manager.clone()
        };
        let auth_password = match ssh.auth_method {
            SshAuthMethod::Password => ssh_password.clone(),
            SshAuthMethod::PrivateKey => None,
        };
        let local_port = manager
            .create_tunnel(
                temp_id.clone(),
                ssh,
                auth_password,
                profile.host.clone(),
                profile.port,
            )
            .await?;
        ("127.0.0.1".to_string(), local_port, Some(temp_id))
    } else {
        (profile.host.clone(), profile.port, None)
    };

    let opts = ConnectionOptions {
        host,
        port,
        username: profile.username.clone(),
        password: Some(password),
        database: profile.database.clone(),
        timeout: Some(30),
        require_tls: matches!(profile.driver, DbDriver::Supabase | DbDriver::Neon)
            || (profile.driver.is_postgres_compatible() && profile.ssl_mode == SslMode::Require),
    };

    let result = dial(&profile.driver, opts).await;

    if let Some(tunnel_id) = temp_tunnel {
        let manager = {
            let guard = state.lock().unwrap();
            guard.ssh_tunnel_manager.clone()
        };
        // Best effort: a leaked test tunnel is not worth failing a successful
        // test over, but it should not be silent either.
        if let Err(e) = manager.close_tunnel(&tunnel_id).await {
            eprintln!("Warning: failed to close test SSH tunnel: {}", e);
        }
    }

    result
}

pub async fn dial(driver: &DbDriver, opts: ConnectionOptions) -> Result<(), DbError> {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            PostgresDriver::connect(opts).await?.test_connection().await
        }
        DbDriver::Sqlite => SqliteDriver::connect(opts).await?.test_connection().await,
        DbDriver::MySql => MysqlDriver::connect(opts).await?.test_connection().await,
        DbDriver::MongoDb => MongoDbDriver::connect(opts).await?.test_connection().await,
        DbDriver::SqlServer => {
            SqlServerDriver::connect(opts)
                .await?
                .test_connection()
                .await
        }
        DbDriver::Turso => TursoDriver::connect(opts).await?.test_connection().await,
        DbDriver::Redis => RedisDriver::connect(opts).await?.test_connection().await,
    }
}

/// Persist a profile, then store its secrets in the OS keyring.
///
/// `id` is the profile's id; `ssh_password` is `Some` only when the tunnel
/// uses password auth and one was typed.
pub async fn save_profile(
    state: Arc<Mutex<AppState>>,
    profile: ConnectionProfile,
    id: String,
    password: String,
    ssh_password: Option<String>,
) -> SaveOutcome {
    // No await inside the guard's scope, so the lock is never held across
    // a suspension point.
    {
        let mut guard = state.lock().unwrap();
        guard.add_profile(profile);
        guard.save_profiles_to_store()?;
    }

    // The keyring can refuse (locked keychain, unsigned binary). That must
    // not throw away a profile that saved fine, so it is reported as a
    // warning alongside a successful save.
    let mut warning = None;
    if !password.is_empty() {
        if let Err(e) = CredentialManager::save_password(&id, &password) {
            warning = Some(format!(
                "Profile saved, but the password could not be stored: {}",
                e
            ));
        }
    }
    if let Some(ssh) = ssh_password {
        if let Err(e) = CredentialManager::save_ssh_password(&id, &ssh) {
            warning = Some(format!(
                "Profile saved, but the SSH password could not be stored: {}",
                e
            ));
        }
    }

    Ok((id, warning))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_pg_form() -> ConnectionForm {
        ConnectionForm {
            name: "Prod".to_string(),
            host: "db.example.com".to_string(),
            port: "5432".to_string(),
            username: "app".to_string(),
            ..ConnectionForm::new_for(DbDriver::Postgres)
        }
    }

    // --- default ports ---

    #[test]
    fn default_ports_match_the_documented_values() {
        assert_eq!(default_port(&DbDriver::Postgres), 5432);
        assert_eq!(default_port(&DbDriver::MySql), 3306);
        assert_eq!(default_port(&DbDriver::SqlServer), 1433);
        assert_eq!(default_port(&DbDriver::MongoDb), 27017);
        assert_eq!(default_port(&DbDriver::Redis), 6379);
        assert_eq!(default_port(&DbDriver::Supabase), 5432);
        assert_eq!(default_port(&DbDriver::Neon), 5432);
        assert_eq!(default_port(&DbDriver::Turso), 443);
        assert_eq!(default_port(&DbDriver::Sqlite), 0);
    }

    #[test]
    fn new_form_prefills_the_driver_default_port() {
        assert_eq!(ConnectionForm::new_for(DbDriver::MySql).port, "3306");
        assert_eq!(ConnectionForm::new_for(DbDriver::Redis).port, "6379");
        // SQLite has no port, so the box is left blank rather than showing 0.
        assert_eq!(ConnectionForm::new_for(DbDriver::Sqlite).port, "");
    }

    #[test]
    fn switching_driver_moves_an_untouched_port_but_keeps_a_typed_one() {
        let mut form = ConnectionForm::new_for(DbDriver::Postgres);
        form.set_driver(DbDriver::MySql);
        assert_eq!(form.port, "3306");

        let mut form = ConnectionForm::new_for(DbDriver::Postgres);
        form.port = "6543".to_string(); // a pgbouncer port the user typed
        form.set_driver(DbDriver::MySql);
        assert_eq!(form.port, "6543");
    }

    #[test]
    fn managed_postgres_defaults_to_require_ssl() {
        assert_eq!(default_ssl_mode(&DbDriver::Supabase), SslMode::Require);
        assert_eq!(default_ssl_mode(&DbDriver::Neon), SslMode::Require);
        assert_eq!(default_ssl_mode(&DbDriver::Postgres), SslMode::Prefer);
        assert_eq!(
            ConnectionForm::new_for(DbDriver::Neon).ssl_mode,
            SslMode::Require
        );
    }

    // --- driver-specific fields ---

    #[test]
    fn sqlite_asks_for_a_file_not_a_server() {
        let f = driver_fields(&DbDriver::Sqlite);
        assert!(f.file_path);
        assert!(!f.host_port);
        assert!(!f.url);
        assert!(!f.username);
        assert!(!f.password);
        assert!(!f.ssl);
        assert!(!f.ssh);
        assert_eq!(database_label(&DbDriver::Sqlite), "Database file");
    }

    #[test]
    fn turso_asks_for_a_url_and_an_auth_token() {
        let f = driver_fields(&DbDriver::Turso);
        assert!(f.url);
        assert!(!f.host_port);
        assert!(!f.username, "Turso authenticates with a token, not a user");
        assert!(f.password);
        assert_eq!(password_label(&DbDriver::Turso), "Auth token");
    }

    #[test]
    fn mongo_and_redis_make_the_username_optional() {
        for driver in [DbDriver::MongoDb, DbDriver::Redis] {
            let f = driver_fields(&driver);
            assert!(f.username, "{:?} shows a username box", driver);
            assert!(
                !f.username_required,
                "{:?} allows an empty username",
                driver
            );
        }
        assert_eq!(database_label(&DbDriver::Redis), "Database index");
    }

    #[test]
    fn server_drivers_require_a_username() {
        for driver in [
            DbDriver::Postgres,
            DbDriver::MySql,
            DbDriver::SqlServer,
            DbDriver::Supabase,
            DbDriver::Neon,
        ] {
            let f = driver_fields(&driver);
            assert!(f.host_port, "{:?} uses host/port", driver);
            assert!(f.username_required, "{:?} requires a username", driver);
            assert!(f.ssh, "{:?} can tunnel", driver);
        }
    }

    #[test]
    fn every_driver_variant_has_field_rules() {
        for driver in ALL_DRIVERS {
            let f = driver_fields(&driver);
            // Exactly one way of naming the target.
            let ways = [f.file_path, f.url, f.host_port]
                .iter()
                .filter(|b| **b)
                .count();
            assert_eq!(ways, 1, "{:?} must have exactly one address style", driver);
        }
    }

    // --- validation ---

    #[test]
    fn a_complete_postgres_form_is_valid() {
        assert!(validate(&valid_pg_form()).is_valid());
    }

    #[test]
    fn name_is_always_required() {
        let mut form = valid_pg_form();
        form.name = "   ".to_string();
        let v = validate(&form);
        assert_eq!(v.name.as_deref(), Some("Connection name is required"));
        assert!(!v.is_valid());
    }

    #[test]
    fn host_is_required_for_server_drivers() {
        let mut form = valid_pg_form();
        form.host = String::new();
        assert_eq!(validate(&form).host.as_deref(), Some("Host is required"));
    }

    #[test]
    fn username_is_required_for_postgres_but_not_mongo() {
        let mut form = valid_pg_form();
        form.username = String::new();
        assert!(validate(&form).username.is_some());

        form.set_driver(DbDriver::MongoDb);
        assert!(validate(&form).username.is_none());
        assert!(validate(&form).is_valid());
    }

    #[test]
    fn port_must_be_a_real_tcp_port() {
        assert_eq!(parse_port("5432"), Some(5432));
        assert_eq!(parse_port(" 1 "), Some(1));
        assert_eq!(parse_port("65535"), Some(65535));
        assert_eq!(parse_port("0"), None, "0 is not a port to dial");
        assert_eq!(parse_port("65536"), None);
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("abc"), None);
        assert_eq!(parse_port("-1"), None);

        let mut form = valid_pg_form();
        form.port = "70000".to_string();
        assert_eq!(
            validate(&form).port.as_deref(),
            Some("Port must be between 1 and 65535")
        );
    }

    #[test]
    fn sqlite_needs_a_path_and_ignores_host_and_port() {
        let mut form = ConnectionForm::new_for(DbDriver::Sqlite);
        form.name = "Local file".to_string();
        form.host = String::new();
        let v = validate(&form);
        assert!(v.host.is_none(), "SQLite has no host to validate");
        assert!(v.port.is_none(), "SQLite has no port to validate");
        assert_eq!(
            v.database.as_deref(),
            Some("Database file path is required for SQLite")
        );

        form.database = "/tmp/app.sqlite".to_string();
        assert!(validate(&form).is_valid());
    }

    #[test]
    fn turso_needs_a_url() {
        let mut form = ConnectionForm::new_for(DbDriver::Turso);
        form.name = "Edge".to_string();
        form.host = String::new();
        assert_eq!(
            validate(&form).host.as_deref(),
            Some("libSQL URL is required")
        );

        form.host = "libsql://db-org.turso.io".to_string();
        assert!(validate(&form).is_valid());
    }

    #[test]
    fn ssh_fields_are_only_validated_when_the_tunnel_is_on() {
        let mut form = valid_pg_form();
        assert!(validate(&form).is_valid());

        form.ssh_enabled = true;
        let v = validate(&form);
        assert!(v.ssh_host.is_some());
        assert!(v.ssh_username.is_some());

        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        assert!(validate(&form).is_valid());
    }

    #[test]
    fn ssh_private_key_auth_requires_a_key_path() {
        let mut form = valid_pg_form();
        form.ssh_enabled = true;
        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        form.ssh_auth_method = SshAuthMethod::PrivateKey;
        assert_eq!(
            validate(&form).ssh_private_key_path.as_deref(),
            Some("Private key path is required")
        );

        form.ssh_private_key_path = "~/.ssh/id_ed25519".to_string();
        assert!(validate(&form).is_valid());
    }

    #[test]
    fn ssh_port_is_range_checked() {
        let mut form = valid_pg_form();
        form.ssh_enabled = true;
        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        form.ssh_port = "0".to_string();
        assert!(validate(&form).ssh_port.is_some());
    }

    #[test]
    fn validation_messages_lists_every_problem() {
        let form = ConnectionForm::new_for(DbDriver::Postgres);
        let v = validate(&form);
        // Blank name, blank username; host defaults to localhost.
        assert_eq!(v.messages().len(), 2);
    }

    // --- profile construction ---

    #[test]
    fn build_profile_maps_every_field() {
        let mut form = valid_pg_form();
        form.database = "app_production".to_string();
        form.folder = "Work".to_string();
        form.environment = Some(Environment::Production);
        form.color = "#3b82f6".to_string();
        form.description = "The real one".to_string();
        form.is_favorite = true;
        form.ssl_mode = SslMode::Require;

        let p = build_profile_with(&form, "id-1".to_string(), 1000);
        assert_eq!(p.id, "id-1");
        assert_eq!(p.name, "Prod");
        assert_eq!(p.driver, DbDriver::Postgres);
        assert_eq!(p.host, "db.example.com");
        assert_eq!(p.port, 5432);
        assert_eq!(p.username, "app");
        assert_eq!(p.database.as_deref(), Some("app_production"));
        assert_eq!(p.ssl_mode, SslMode::Require);
        assert_eq!(p.folder.as_deref(), Some("Work"));
        assert_eq!(p.environment, Some(Environment::Production));
        assert_eq!(p.color.as_deref(), Some("#3b82f6"));
        assert_eq!(p.description.as_deref(), Some("The real one"));
        assert!(p.is_favorite);
        assert_eq!(p.created_at, 1000);
        assert_eq!(p.updated_at, 1000);
    }

    #[test]
    fn build_profile_never_puts_a_secret_in_the_profile() {
        let p = build_profile_with(&valid_pg_form(), "id-1".to_string(), 0);
        assert!(p.password_keyring_key.is_none());
    }

    #[test]
    fn build_profile_blanks_optional_fields_that_are_empty() {
        let p = build_profile_with(&valid_pg_form(), "id-1".to_string(), 0);
        assert!(p.database.is_none());
        assert!(p.folder.is_none());
        assert!(p.color.is_none());
        assert!(p.description.is_none());
        assert!(p.environment.is_none());
        assert!(p.ssh_tunnel.is_none());
    }

    #[test]
    fn build_profile_trims_whitespace() {
        let mut form = valid_pg_form();
        form.name = "  Spaced  ".to_string();
        form.host = " host ".to_string();
        form.username = " me ".to_string();
        let p = build_profile_with(&form, "id".to_string(), 0);
        assert_eq!(p.name, "Spaced");
        assert_eq!(p.host, "host");
        assert_eq!(p.username, "me");
    }

    #[test]
    fn sqlite_profile_gets_no_port() {
        let mut form = ConnectionForm::new_for(DbDriver::Sqlite);
        form.name = "File".to_string();
        form.database = "/tmp/a.db".to_string();
        let p = build_profile_with(&form, "id".to_string(), 0);
        assert_eq!(p.port, 0);
        assert_eq!(p.database.as_deref(), Some("/tmp/a.db"));
    }

    #[test]
    fn build_profile_preserves_history_on_edit() {
        let mut form = valid_pg_form();
        form.created_at = Some(111);
        form.last_connected_at = Some(222);
        form.connection_count = 9;
        let p = build_profile_with(&form, "id".to_string(), 999);
        assert_eq!(p.created_at, 111, "created_at survives an edit");
        assert_eq!(p.updated_at, 999);
        assert_eq!(p.last_connected_at, Some(222));
        assert_eq!(p.connection_count, 9);
    }

    #[test]
    fn ssh_config_is_built_only_when_enabled() {
        let mut form = valid_pg_form();
        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        assert!(build_ssh_config(&form).is_none());

        form.ssh_enabled = true;
        let ssh = build_ssh_config(&form).expect("tunnel configured");
        assert_eq!(ssh.host, "bastion");
        assert_eq!(ssh.port, 22);
        assert_eq!(ssh.username, "ubuntu");
        assert_eq!(ssh.auth_method, SshAuthMethod::Password);
        assert!(
            ssh.private_key_path.is_none(),
            "password auth stores no key path"
        );
        assert_eq!(ssh.local_port, 0);
    }

    #[test]
    fn ssh_key_path_is_dropped_when_switching_back_to_password_auth() {
        let mut form = valid_pg_form();
        form.ssh_enabled = true;
        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        form.ssh_private_key_path = "/keys/id".to_string();
        form.ssh_auth_method = SshAuthMethod::PrivateKey;
        assert_eq!(
            build_ssh_config(&form).unwrap().private_key_path.as_deref(),
            Some("/keys/id")
        );

        form.ssh_auth_method = SshAuthMethod::Password;
        assert!(build_ssh_config(&form).unwrap().private_key_path.is_none());
    }

    #[test]
    fn drivers_without_tunnel_support_never_build_one() {
        let mut form = ConnectionForm::new_for(DbDriver::Sqlite);
        form.ssh_enabled = true;
        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        assert!(build_ssh_config(&form).is_none());
    }

    #[test]
    fn form_round_trips_through_a_profile() {
        let mut form = valid_pg_form();
        form.ssh_enabled = true;
        form.ssh_host = "bastion".to_string();
        form.ssh_username = "ubuntu".to_string();
        form.ssh_port = "2222".to_string();
        form.database = "app".to_string();
        form.folder = "Work".to_string();
        form.environment = Some(Environment::Staging);
        form.color = "#ff0000".to_string();
        form.description = "notes".to_string();
        form.is_favorite = true;

        let profile = build_profile_with(&form, "id-42".to_string(), 500);
        let back = form_from_profile(&profile);

        assert_eq!(back.id.as_deref(), Some("id-42"));
        assert_eq!(back.name, form.name);
        assert_eq!(back.driver, form.driver);
        assert_eq!(back.host, form.host);
        assert_eq!(back.port, form.port);
        assert_eq!(back.username, form.username);
        assert_eq!(back.database, form.database);
        assert_eq!(back.folder, form.folder);
        assert_eq!(back.environment, form.environment);
        assert_eq!(back.color, form.color);
        assert_eq!(back.description, form.description);
        assert_eq!(back.is_favorite, form.is_favorite);
        assert!(back.ssh_enabled);
        assert_eq!(back.ssh_host, "bastion");
        assert_eq!(back.ssh_port, "2222");
        assert_eq!(back.ssh_username, "ubuntu");
        assert_eq!(back.created_at, Some(500));
    }

    // --- connection strings ---

    #[test]
    fn parses_a_full_postgres_uri() {
        let p = parse_connection_string(
            &DbDriver::Postgres,
            "postgresql://me:s3cret@db.host:6543/app",
        )
        .expect("parsed");
        assert_eq!(p.username.as_deref(), Some("me"));
        assert_eq!(p.password.as_deref(), Some("s3cret"));
        assert_eq!(p.host.as_deref(), Some("db.host"));
        assert_eq!(p.port, Some(6543));
        assert_eq!(p.database.as_deref(), Some("app"));
    }

    #[test]
    fn parses_a_uri_without_credentials_or_port() {
        let p = parse_connection_string(&DbDriver::MySql, "mysql://db.host/app").expect("parsed");
        assert!(p.username.is_none());
        assert!(p.password.is_none());
        assert_eq!(p.host.as_deref(), Some("db.host"));
        assert_eq!(p.port, None);
        assert_eq!(p.database.as_deref(), Some("app"));
    }

    #[test]
    fn drops_the_query_string() {
        let p = parse_connection_string(
            &DbDriver::Neon,
            "postgresql://u:p@ep-x.neon.tech/neondb?sslmode=require",
        )
        .expect("parsed");
        assert_eq!(p.database.as_deref(), Some("neondb"));
    }

    #[test]
    fn rejects_a_uri_for_a_different_driver() {
        assert!(parse_connection_string(&DbDriver::MySql, "postgresql://h/db").is_none());
        assert!(parse_connection_string(&DbDriver::Postgres, "not a url").is_none());
        assert!(parse_connection_string(&DbDriver::Sqlite, "sqlite:///tmp/a.db").is_none());
    }

    #[test]
    fn handles_an_at_sign_inside_the_password() {
        let p = parse_connection_string(&DbDriver::Postgres, "postgres://me:p@ss@db.host:5432/app")
            .expect("parsed");
        assert_eq!(p.password.as_deref(), Some("p@ss"));
        assert_eq!(p.host.as_deref(), Some("db.host"));
    }

    #[test]
    fn redis_uri_with_only_a_password() {
        let p = parse_connection_string(&DbDriver::Redis, "redis://:hunter2@cache:6379/2")
            .expect("parsed");
        assert!(p.username.is_none());
        assert_eq!(p.password.as_deref(), Some("hunter2"));
        assert_eq!(p.port, Some(6379));
        assert_eq!(p.database.as_deref(), Some("2"));
    }

    #[test]
    fn mongodb_srv_scheme_is_accepted() {
        let p = parse_connection_string(
            &DbDriver::MongoDb,
            "mongodb+srv://u:p@cluster.mongodb.net/app",
        )
        .expect("parsed");
        assert_eq!(p.host.as_deref(), Some("cluster.mongodb.net"));
        assert_eq!(p.port, None);
    }

    #[test]
    fn applying_a_connection_string_leaves_unmentioned_fields_alone() {
        let mut form = valid_pg_form();
        form.name = "Keep me".to_string();
        form.database = "existing".to_string();

        let parsed =
            parse_connection_string(&DbDriver::Postgres, "postgres://newuser@newhost:1234")
                .unwrap();
        let password = apply_connection_string(&mut form, &parsed);

        assert_eq!(form.name, "Keep me", "the name is not part of a URI");
        assert_eq!(
            form.database, "existing",
            "no database in the URI, so no change"
        );
        assert_eq!(form.host, "newhost");
        assert_eq!(form.port, "1234");
        assert_eq!(form.username, "newuser");
        assert!(password.is_none());
    }

    #[test]
    fn applying_a_connection_string_returns_its_password() {
        let mut form = valid_pg_form();
        let parsed =
            parse_connection_string(&DbDriver::Postgres, "postgres://u:pw@h:5432/d").unwrap();
        assert_eq!(
            apply_connection_string(&mut form, &parsed).as_deref(),
            Some("pw")
        );
    }

    // --- colour tags ---

    #[test]
    fn parses_hex_colour_tags() {
        assert_eq!(parse_hex_color("#3b82f6"), Some((0x3b, 0x82, 0xf6)));
        assert_eq!(parse_hex_color("3b82f6"), Some((0x3b, 0x82, 0xf6)));
        assert_eq!(parse_hex_color("#f00"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#zzzzzz"), None);
        assert_eq!(parse_hex_color("#12345"), None);
    }

    #[test]
    fn every_colour_preset_parses_and_round_trips_its_index() {
        for (i, p) in COLOR_PRESETS.iter().enumerate() {
            assert!(parse_hex_color(p).is_some(), "{p}");
            assert_eq!(color_preset_index(p), Some(i));
        }
        assert_eq!(color_preset_index(&COLOR_PRESETS[2].to_uppercase()), Some(2));
        assert_eq!(color_preset_index(""), None);
        assert_eq!(color_preset_index("#123456"), None);
    }

    // --- pasting a connection string ---

    #[test]
    fn pasting_fills_the_form_and_returns_the_password() {
        let mut form = ConnectionForm::new_for(DbDriver::Postgres);
        let pasted =
            paste_connection_string(&mut form, "postgresql://app:s3cret@db.example.com:6543/shop")
                .unwrap();
        assert_eq!(form.host, "db.example.com");
        assert_eq!(form.port, "6543");
        assert_eq!(form.username, "app");
        assert_eq!(form.database, "shop");
        assert_eq!(pasted.password.as_deref(), Some("s3cret"));
    }

    #[test]
    fn pasting_another_drivers_string_is_an_error_and_leaves_the_form_alone() {
        let mut form = ConnectionForm::new_for(DbDriver::Postgres);
        let before = form.clone();
        let err = match paste_connection_string(&mut form, "mysql://u:p@h:3306/d") {
            Err(e) => e,
            Ok(_) => panic!("a mysql:// string should not fill a Postgres form"),
        };
        assert!(err.contains("postgresql://"), "{err}");
        assert_eq!(form, before);
        assert!(paste_connection_string(&mut form, "not a uri").is_err());
        assert!(paste_connection_string(&mut form, "   ").is_err());
    }
}
