//! Shell-agnostic backup and restore operations.
//!
//! Lifted from the `#[tauri::command]` handlers in
//! `src-tauri/src/commands/backup.rs`, which were already portable: their
//! bodies only touch `AppState`, the filesystem and the native dump tools.
//! Only the signatures were Tauri-shaped (`State<'_, Mutex<AppState>>`).
//! Everything here takes a plain `&Mutex<AppState>` so both shells run the
//! same code.
//!
//! Backups go through the database's own CLI tools (`pg_dump`, `mysqldump`,
//! `mongodump`, and a plain file copy for SQLite) rather than being
//! reimplemented over the drivers. Those tools are the only things that
//! produce a dump the vendor's own restore path will accept.
//!
//! ## Progress
//!
//! The Tauri shell had no progress reporting at all: a dump either finished
//! or it did not. The native shell needs something to show during a
//! multi-minute `pg_dump`, so the long operations take a [`ProgressSink`].
//!
//! It is an **unbounded** `tokio::sync::mpsc` sender rather than a
//! `Arc<Mutex<Progress>>` for two reasons: sending must never block (these
//! calls happen on a blocking thread in the middle of driving a child
//! process, and a bounded channel whose consumer is a UI drawing at 60 fps
//! would eventually stall the dump), and a queue preserves the ordering of
//! the stage messages so nothing is silently overwritten between frames.
//! Progress is coarse and event-count-bounded (a handful per operation), so
//! unbounded cannot grow without limit.
//!
//! A sink with no receiver is valid and cheap: `ProgressSink::default()`
//! discards everything, which is what a caller that does not care about
//! progress (or a test) passes.
//!
//! ## Credentials
//!
//! Passwords never reach `argv`, which is world-readable via `ps` on every
//! Unix. `pg_dump`/`psql` take `PGPASSWORD`, `mysqldump`/`mysql` take
//! `MYSQL_PWD`, and `mongodump` gets a 0600 temp config file that is removed
//! whether or not the dump succeeded.

use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use std::sync::Mutex;

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::credentials::CredentialManager;
use crate::models::backup::{
    BackupEntry, BackupOptions, BackupProgress, BackupStatus, RestoreOptions,
};
use crate::models::{ConnectionProfile, DbDriver, DbError};
use crate::state::AppState;

/// File extensions `list_backups` recognises as backups.
pub const BACKUP_EXTENSIONS: [&str; 5] = ["sql", "dump", "bak", "sqlite", "gz"];

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

/// Where a long-running backup or restore reports its stage.
///
/// Cloneable and `Send`, so it can cross into a `spawn_blocking` closure.
/// The default sink drops everything.
#[derive(Clone, Default)]
pub struct ProgressSink {
    tx: Option<mpsc::UnboundedSender<BackupProgress>>,
}

impl ProgressSink {
    /// A sink and the receiver the UI polls.
    pub fn channel() -> (Self, mpsc::UnboundedReceiver<BackupProgress>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx: Some(tx) }, rx)
    }

    /// A sink that discards everything, for callers with no UI.
    pub fn silent() -> Self {
        Self::default()
    }

    fn emit(&self, p: BackupProgress) {
        if let Some(tx) = &self.tx {
            // A closed channel means the UI went away (dialog closed while
            // the dump ran). That is expected, not an error, and must not
            // abort the operation in flight.
            let _ = tx.send(p);
        }
    }

    /// Report entering a stage. `stage` is a stable key; `message` is prose.
    pub fn stage(&self, stage: &str, message: impl Into<String>) {
        self.emit(BackupProgress {
            stage: stage.to_string(),
            message: message.into(),
            done: false,
            error: None,
        });
    }

    /// Report successful completion.
    pub fn finished(&self, message: impl Into<String>) {
        self.emit(BackupProgress {
            stage: "done".to_string(),
            message: message.into(),
            done: true,
            error: None,
        });
    }

    /// Report failure. The driver/tool message is carried verbatim: it is
    /// usually the only thing that identifies the real cause.
    pub fn failed(&self, error: impl Into<String>) {
        let error = error.into();
        self.emit(BackupProgress {
            stage: "done".to_string(),
            message: error.clone(),
            done: true,
            error: Some(error),
        });
    }
}

// ---------------------------------------------------------------------------
// Pure logic: paths, names, validation, SQL
// ---------------------------------------------------------------------------

/// The default backup directory, created if missing.
///
/// `DBHIVE_DATA_DIR`, when set, moves it under that directory like every
/// other store (tests set it to a temp dir so a backup never lands in the
/// developer's own data folder).
pub fn backup_dir() -> Result<PathBuf, DbError> {
    let over = std::env::var_os(crate::storage::DATA_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    backup_dir_in(over, dirs::data_dir())
}

/// [`backup_dir`] with its two inputs given, so the choice can be tested
/// without touching the environment.
pub fn backup_dir_in(
    override_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
) -> Result<PathBuf, DbError> {
    let dir = match override_dir {
        Some(base) => base.join("backups"),
        None => data_dir
            .ok_or_else(|| DbError::InternalError("Cannot locate data directory".to_string()))?
            .join("db-hive")
            .join("backups"),
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| DbError::InternalError(format!("Cannot create backup dir: {}", e)))?;
    Ok(dir)
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Reduce an arbitrary connection name to something safe in a filename.
///
/// Everything non-alphanumeric becomes `_`, so a profile called
/// `prod/orders (eu)` cannot escape the backup directory or confuse a shell.
pub fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// The file extension a dump of this driver gets.
pub fn backup_extension(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::MongoDb => "bak",
        DbDriver::Sqlite => "sqlite",
        _ => "sql",
    }
}

/// `{safe connection name}_{unix seconds}.{ext}`.
///
/// The timestamp is part of the name rather than relying on mtime so two
/// backups of the same connection in the same session never collide and the
/// list can be read without stat-ing every file.
pub fn backup_filename(connection_name: &str, ts: i64, driver: &DbDriver) -> String {
    format!(
        "{}_{}.{}",
        safe_name(connection_name),
        ts,
        backup_extension(driver)
    )
}

/// Recover the connection name from a file stem produced by
/// [`backup_filename`].
///
/// The trailing `_{digits}` is only stripped when it really is a timestamp.
/// The Tauri version stripped any trailing `_segment`, which turned an
/// unrelated file called `nightly_dump.sql` into the connection "nightly".
pub fn connection_name_from_stem(stem: &str) -> String {
    match stem.rsplit_once('_') {
        Some((name, ts)) if !ts.is_empty() && ts.chars().all(|c| c.is_ascii_digit()) => {
            name.replace('_', " ")
        }
        _ => stem.to_string(),
    }
}

/// Whether `list_backups` should report this path.
pub fn is_backup_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| BACKUP_EXTENSIONS.contains(&ext))
}

/// Whether a dump can be taken for this driver at all.
pub fn supports_backup(driver: &DbDriver) -> bool {
    matches!(
        driver,
        DbDriver::Postgres
            | DbDriver::Supabase
            | DbDriver::Neon
            | DbDriver::MySql
            | DbDriver::Sqlite
            | DbDriver::MongoDb
    )
}

/// Whether a dump can be restored for this driver.
///
/// Narrower than [`supports_backup`]: there is no `mongorestore` path here,
/// matching the Tauri implementation.
pub fn supports_restore(driver: &DbDriver) -> bool {
    matches!(
        driver,
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon | DbDriver::MySql | DbDriver::Sqlite
    )
}

/// The external tool a dump of this driver shells out to, for messages.
pub fn backup_tool(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => "pg_dump",
        DbDriver::MySql => "mysqldump",
        DbDriver::MongoDb => "mongodump",
        DbDriver::Sqlite => "file copy",
        _ => "unsupported",
    }
}

/// The external tool a restore of this driver shells out to, for messages.
pub fn restore_tool(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => "psql",
        DbDriver::MySql => "mysql",
        DbDriver::Sqlite => "file copy",
        _ => "unsupported",
    }
}

/// Reject option combinations that would produce a useless or dangerous dump
/// before a child process is spawned.
pub fn validate_backup_options(driver: &DbDriver, o: &BackupOptions) -> Result<(), DbError> {
    if !supports_backup(driver) {
        return Err(DbError::InvalidInput(format!(
            "Backup is not supported for {:?} connections",
            driver
        )));
    }
    if !o.include_schema && !o.include_data {
        return Err(DbError::InvalidInput(
            "Select at least schema or data; a backup with neither is empty".to_string(),
        ));
    }
    // A table name reaches the dump tool as an argument. Empty entries are a
    // typo in a comma-separated list, and a leading `-` would be read as a
    // flag rather than a table.
    for t in &o.tables {
        if t.trim().is_empty() {
            return Err(DbError::InvalidInput(
                "Table list contains an empty name".to_string(),
            ));
        }
        if t.starts_with('-') {
            return Err(DbError::InvalidInput(format!(
                "Table name {:?} cannot start with '-'",
                t
            )));
        }
    }
    if !o.tables.is_empty() && matches!(driver, DbDriver::Sqlite | DbDriver::MongoDb) {
        return Err(DbError::InvalidInput(format!(
            "{:?} backups copy the whole database; per-table selection is not available",
            driver
        )));
    }
    if let Some(dir) = &o.output_dir {
        if dir.trim().is_empty() {
            return Err(DbError::InvalidInput(
                "Output directory is empty; leave it blank to use the default".to_string(),
            ));
        }
        if !Path::new(dir.trim()).is_absolute() {
            return Err(DbError::InvalidInput(format!(
                "Output directory must be an absolute path, got {:?}",
                dir.trim()
            )));
        }
    }
    Ok(())
}

/// Reject a restore that cannot possibly work before anything is dropped.
pub fn validate_restore_options(driver: &DbDriver, o: &RestoreOptions) -> Result<(), DbError> {
    if !supports_restore(driver) {
        return Err(DbError::InvalidInput(format!(
            "Restore is not supported for {:?} connections",
            driver
        )));
    }
    if o.file_path.trim().is_empty() {
        return Err(DbError::InvalidInput(
            "No backup file selected".to_string(),
        ));
    }
    if !Path::new(&o.file_path).is_file() {
        return Err(DbError::NotFound(format!(
            "Backup file {} does not exist",
            o.file_path
        )));
    }
    if o.drop_existing && matches!(driver, DbDriver::Sqlite) {
        // Restoring SQLite already replaces the whole file.
        return Err(DbError::InvalidInput(
            "SQLite restore replaces the database file; drop & recreate does not apply"
                .to_string(),
        ));
    }
    Ok(())
}

/// Quote a MySQL identifier. Backticks inside are doubled, so a database
/// literally called `` a`b `` cannot break out of the quoting and turn a
/// DROP DATABASE into two statements.
pub fn quote_mysql_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// The statements that drop and recreate a MySQL database.
///
/// Generated rather than templated at the call site so it can be tested; it
/// is the single most destructive string this module produces.
pub fn mysql_drop_recreate_sql(database: &str) -> Result<String, DbError> {
    if database.trim().is_empty() {
        return Err(DbError::InvalidInput(
            "Cannot drop and recreate: the connection names no database".to_string(),
        ));
    }
    let ident = quote_mysql_ident(database.trim());
    Ok(format!(
        "DROP DATABASE IF EXISTS {ident}; CREATE DATABASE {ident};"
    ))
}

/// The database a dump of this profile targets, with the same defaults the
/// Tauri handler applied.
pub fn target_database(profile: &ConnectionProfile) -> String {
    match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => profile
            .database
            .clone()
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| "postgres".to_string()),
        _ => profile.database.clone().unwrap_or_default(),
    }
}

/// The database file of a SQLite profile.
///
/// The connection editor stores the path in `database` and leaves `host`
/// blank; profiles from before that held it in `host`. Reading only `host`
/// made every backup of an editor-made SQLite connection copy "".
pub fn sqlite_file(profile: &ConnectionProfile) -> String {
    match profile.database.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => d.to_string(),
        _ => profile.host.clone(),
    }
}

// ---------------------------------------------------------------------------
// Directory listing
// ---------------------------------------------------------------------------

/// The default backup directory as a string, for display.
pub fn get_backup_directory() -> Result<String, DbError> {
    Ok(backup_dir()?.to_string_lossy().to_string())
}

/// Every backup file in `directory`, newest first.
///
/// `None` means the default directory. Nothing here reads the file contents:
/// the entries are reconstructed from the name and the metadata, which is
/// why the naming scheme in [`backup_filename`] matters.
pub fn list_backups(directory: Option<&str>) -> Result<Vec<BackupEntry>, DbError> {
    let dir = match directory {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => backup_dir()?,
    };

    let read_dir = std::fs::read_dir(&dir)
        .map_err(|e| DbError::InternalError(format!("Cannot read backup dir: {}", e)))?;

    let mut entries: Vec<BackupEntry> = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !is_backup_file(&path) {
            continue;
        }

        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // A file that vanished between the read_dir and the stat is not a
        // reason to fail the whole listing.
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };

        let created_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        entries.push(BackupEntry {
            id: Uuid::new_v4().to_string(),
            connection_id: String::new(),
            connection_name: connection_name_from_stem(&stem),
            driver: String::new(),
            file_path: path.to_string_lossy().to_string(),
            file_name,
            size_bytes: meta.len(),
            created_at,
            status: BackupStatus::Completed,
            note: None,
        });
    }

    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

/// Remove a backup file from disk. Irreversible.
pub fn delete_backup(file_path: &str) -> Result<(), DbError> {
    std::fs::remove_file(file_path)
        .map_err(|e| DbError::InternalError(format!("Delete failed: {}", e)))
}

/// Reveal a directory in the OS file manager.
pub fn open_directory(dir: &str) -> Result<(), DbError> {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let opener = "xdg-open";

    SysCommand::new(opener)
        .arg(dir)
        .spawn()
        .map_err(|e| DbError::InternalError(format!("Cannot open {}: {}", dir, e)))?;
    Ok(())
}

/// Reveal the default backup directory in the OS file manager.
pub fn open_backup_directory() -> Result<(), DbError> {
    let dir = backup_dir()?.to_string_lossy().to_string();
    open_directory(&dir)
}

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

/// The profile and its password, from the session cache or the OS keyring.
fn profile_and_password(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<(ConnectionProfile, String), DbError> {
    let (profile, cached) = {
        let guard = state.lock().unwrap();
        let profile = guard
            .get_profile(connection_id)
            .cloned()
            .ok_or_else(|| DbError::NotFound(format!("Connection {} not found", connection_id)))?;
        let cached = guard.connection_passwords.get(connection_id).cloned();
        (profile, cached)
    };

    // Fall back to the OS keyring if the session cache has no entry. An
    // absent password is not fatal: trust auth and SQLite need none.
    let password = match cached {
        Some(p) => p,
        None => CredentialManager::get_password(connection_id)
            .ok()
            .flatten()
            .unwrap_or_default(),
    };
    Ok((profile, password))
}

// ---------------------------------------------------------------------------
// Backup
// ---------------------------------------------------------------------------

/// Dump a connection's database to a file.
///
/// The dump itself is synchronous (it drives a child process to completion),
/// so it runs on a blocking thread; awaiting it on a worker thread would
/// park that worker for the whole dump.
pub async fn create_backup(
    state: &Mutex<AppState>,
    connection_id: &str,
    options: BackupOptions,
    progress: ProgressSink,
) -> Result<BackupEntry, DbError> {
    progress.stage("preparing", "Reading connection profile");

    let (profile, password) = match profile_and_password(state, connection_id) {
        Ok(v) => v,
        Err(e) => {
            progress.failed(e.to_string());
            return Err(e);
        }
    };

    if let Err(e) = validate_backup_options(&profile.driver, &options) {
        progress.failed(e.to_string());
        return Err(e);
    }

    let out_dir = match options.output_dir.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => match backup_dir() {
            Ok(d) => d,
            Err(e) => {
                progress.failed(e.to_string());
                return Err(e);
            }
        },
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir)
        .map_err(|e| DbError::InternalError(format!("Cannot create {}: {}", out_dir.display(), e)))
    {
        progress.failed(e.to_string());
        return Err(e);
    }

    let ts = unix_now();
    let filename = backup_filename(&profile.name, ts, &profile.driver);
    let output_path = out_dir.join(&filename);

    progress.stage(
        "dumping",
        format!("Running {}…", backup_tool(&profile.driver)),
    );

    let sink = progress.clone();
    let joined = tokio::task::spawn_blocking({
        let out_dir = out_dir.clone();
        let output_path = output_path.clone();
        move || {
            let result = run_dump(&profile, &password, &options, &out_dir, &output_path, ts, &sink);
            (profile, options, result)
        }
    })
    .await;

    let (profile, options, result) = match joined {
        Ok(v) => v,
        Err(e) => {
            let err = DbError::InternalError(format!("Backup task failed: {}", e));
            progress.failed(err.to_string());
            return Err(err);
        }
    };

    if let Err(e) = result {
        progress.failed(e.to_string());
        return Err(e);
    }

    let size_bytes = std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
    progress.finished(format!("Wrote {}", filename));

    Ok(BackupEntry {
        id: Uuid::new_v4().to_string(),
        connection_id: connection_id.to_string(),
        connection_name: profile.name,
        driver: format!("{:?}", profile.driver),
        file_path: output_path.to_string_lossy().to_string(),
        file_name: filename,
        size_bytes,
        created_at: ts,
        status: BackupStatus::Completed,
        note: options.note,
    })
}

/// The blocking half of [`create_backup`]: spawn the vendor tool and wait.
#[allow(clippy::too_many_arguments)]
fn run_dump(
    profile: &ConnectionProfile,
    password: &str,
    options: &BackupOptions,
    out_dir: &Path,
    output_path: &Path,
    ts: i64,
    progress: &ProgressSink,
) -> Result<(), DbError> {
    match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            let database = target_database(profile);
            let mut cmd = SysCommand::new("pg_dump");
            cmd.env("PGPASSWORD", password)
                .arg("-h")
                .arg(&profile.host)
                .arg("-p")
                .arg(profile.port.to_string())
                .arg("-U")
                .arg(&profile.username)
                .arg("-d")
                .arg(&database)
                .arg("-f")
                .arg(output_path)
                .arg("--no-password");

            if !options.include_data {
                cmd.arg("--schema-only");
            } else if !options.include_schema {
                cmd.arg("--data-only");
            }
            for table in &options.tables {
                cmd.arg("-t").arg(table);
            }

            let out = cmd
                .output()
                .map_err(|e| DbError::InternalError(format!("pg_dump not found: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        DbDriver::MySql => {
            let database = target_database(profile);
            let build = |gtid_purged_off: bool| {
                let mut cmd = SysCommand::new("mysqldump");
                // Password goes through the environment, not argv, where it
                // would be visible to every local process via `ps`
                cmd.env("MYSQL_PWD", password)
                    .arg("-h")
                    .arg(&profile.host)
                    .arg("-P")
                    .arg(profile.port.to_string())
                    .arg("-u")
                    .arg(&profile.username);
                // On a server with GTIDs the default dump carries a
                // `SET @@GLOBAL.GTID_PURGED`, which the same server refuses
                // when the dump is replayed ("added gtid set must not overlap
                // with @@GLOBAL.GTID_EXECUTED"): a backup that cannot be
                // restored where it came from.
                if gtid_purged_off {
                    cmd.arg("--set-gtid-purged=OFF");
                }
                cmd.arg(&database);
                if !options.include_data {
                    cmd.arg("--no-data");
                }
                if !options.include_schema {
                    cmd.arg("--no-create-info");
                }
                for table in &options.tables {
                    cmd.arg(table);
                }
                cmd
            };

            let mut out = build(true)
                .output()
                .map_err(|e| DbError::InternalError(format!("mysqldump not found: {}", e)))?;
            // MariaDB's dump tool has no such option: run it without.
            if !out.status.success()
                && String::from_utf8_lossy(&out.stderr).contains("set-gtid-purged")
            {
                out = build(false)
                    .output()
                    .map_err(|e| DbError::InternalError(format!("mysqldump not found: {}", e)))?;
            }
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
            // mysqldump writes to stdout, so the file is written here.
            progress.stage("writing", "Writing dump to disk");
            std::fs::write(output_path, &out.stdout)
                .map_err(|e| DbError::InternalError(format!("Write failed: {}", e)))?;
        }

        DbDriver::Sqlite => {
            std::fs::copy(sqlite_file(profile), output_path)
                .map_err(|e| DbError::InternalError(format!("Copy failed: {}", e)))?;
        }

        DbDriver::MongoDb => {
            // NOTE: mongodump writes a directory tree under --out, not the
            // single `.bak` file named in the entry. Carried over from the
            // Tauri implementation; the entry's size therefore reads 0 and
            // the dump does not appear in the list. Fixing it means changing
            // the on-disk layout, which is out of scope for the port.
            let out_dir_str = out_dir.to_string_lossy().to_string();
            let mut cmd = SysCommand::new("mongodump");
            cmd.arg("--host")
                .arg(&profile.host)
                .arg("--port")
                .arg(profile.port.to_string())
                .arg("--username")
                .arg(&profile.username)
                .arg("--out")
                .arg(&out_dir_str);
            if let Some(db) = &profile.database {
                cmd.arg("--db").arg(db);
            }

            // Pass the password via a 0600 temp config file rather than
            // argv, where it would be visible to every local process
            let config_path = if password.is_empty() {
                None
            } else {
                let path = std::env::temp_dir().join(format!("db-hive-mongodump-{}.yaml", ts));
                let yaml = format!(
                    "password: \"{}\"\n",
                    password.replace('\\', "\\\\").replace('"', "\\\"")
                );
                std::fs::write(&path, yaml).map_err(|e| {
                    DbError::InternalError(format!("Temp config write failed: {}", e))
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                        .map_err(|e| {
                            DbError::InternalError(format!("Temp config chmod failed: {}", e))
                        })?;
                }
                cmd.arg("--config").arg(&path);
                Some(path)
            };

            let out = cmd
                .output()
                .map_err(|e| DbError::InternalError(format!("mongodump not found: {}", e)));
            if let Some(path) = config_path {
                let _ = std::fs::remove_file(path);
            }
            let out = out?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        _ => {
            return Err(DbError::InvalidInput(format!(
                "Backup is not supported for {:?} connections",
                profile.driver
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// Load a backup file back into a connection's database. Destructive.
///
/// With `drop_existing` the target database is dropped and recreated first.
/// The Tauri handler accepted that flag and silently ignored it; here it is
/// honoured, because a destructive toggle that does nothing is the worse
/// failure mode: the user believes the target was cleared when it was not,
/// and the restore then fails halfway on duplicate objects.
pub async fn restore_backup(
    state: &Mutex<AppState>,
    connection_id: &str,
    options: RestoreOptions,
    progress: ProgressSink,
) -> Result<(), DbError> {
    progress.stage("preparing", "Reading connection profile");

    let (profile, password) = match profile_and_password(state, connection_id) {
        Ok(v) => v,
        Err(e) => {
            progress.failed(e.to_string());
            return Err(e);
        }
    };

    if let Err(e) = validate_restore_options(&profile.driver, &options) {
        progress.failed(e.to_string());
        return Err(e);
    }

    if options.drop_existing {
        progress.stage(
            "dropping",
            format!("Dropping and recreating {}", target_database(&profile)),
        );
    }
    progress.stage(
        "restoring",
        format!("Running {}…", restore_tool(&profile.driver)),
    );

    let joined = tokio::task::spawn_blocking({
        let profile = profile.clone();
        move || run_restore(&profile, &password, &options)
    })
    .await;

    let result = match joined {
        Ok(v) => v,
        Err(e) => {
            let err = DbError::InternalError(format!("Restore task failed: {}", e));
            progress.failed(err.to_string());
            return Err(err);
        }
    };

    match result {
        Ok(()) => {
            progress.finished("Restore complete");
            Ok(())
        }
        Err(e) => {
            progress.failed(e.to_string());
            Err(e)
        }
    }
}

/// The blocking half of [`restore_backup`].
fn run_restore(
    profile: &ConnectionProfile,
    password: &str,
    options: &RestoreOptions,
) -> Result<(), DbError> {
    match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            let database = target_database(profile);

            if options.drop_existing {
                // dropdb/createdb connect to the maintenance database, so
                // they can drop the one being restored into.
                let tool = |name: &str, args: &[&str]| -> Result<std::process::Output, DbError> {
                    let mut cmd = SysCommand::new(name);
                    cmd.env("PGPASSWORD", password)
                        .arg("-h")
                        .arg(&profile.host)
                        .arg("-p")
                        .arg(profile.port.to_string())
                        .arg("-U")
                        .arg(&profile.username)
                        .arg("--no-password");
                    for a in args {
                        cmd.arg(a);
                    }
                    cmd.output()
                        .map_err(|e| DbError::InternalError(format!("{} not found: {}", name, e)))
                };
                let failed = |out: &std::process::Output| {
                    DbError::QueryError(String::from_utf8_lossy(&out.stderr).to_string())
                };

                // The app itself almost always has this database open (the
                // Backup screen is used from a connected workspace), so a
                // plain dropdb stops at "database is being accessed by other
                // users". `--force` (PostgreSQL 13+) ends those sessions.
                let mut dropped = tool("dropdb", &["--if-exists", "--force", &database])?;
                if !dropped.status.success() {
                    let err = String::from_utf8_lossy(&dropped.stderr).to_string();
                    if err.contains("--force") || err.contains("unrecognized option") {
                        // An older client: end the sessions by hand, then drop.
                        let literal = database.replace('\'', "''");
                        let kill = format!(
                            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                             WHERE datname = '{literal}' AND pid <> pg_backend_pid()"
                        );
                        let _ = tool("psql", &["-d", "postgres", "-c", &kill])?;
                        dropped = tool("dropdb", &["--if-exists", &database])?;
                    }
                }
                if !dropped.status.success() {
                    return Err(failed(&dropped));
                }
                let created = tool("createdb", &[&database])?;
                if !created.status.success() {
                    return Err(failed(&created));
                }
            }

            let out = SysCommand::new("psql")
                .env("PGPASSWORD", password)
                .arg("-h")
                .arg(&profile.host)
                .arg("-p")
                .arg(profile.port.to_string())
                .arg("-U")
                .arg(&profile.username)
                .arg("-d")
                .arg(&database)
                // psql exits 0 even when statements failed, which would
                // report a half-applied restore as a success. Stop at the
                // first error (exit status 3) and apply the file atomically,
                // so a failed restore leaves the database as it was.
                .arg("-v")
                .arg("ON_ERROR_STOP=1")
                .arg("--single-transaction")
                .arg("-f")
                .arg(&options.file_path)
                .output()
                .map_err(|e| DbError::InternalError(format!("psql not found: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        DbDriver::MySql => {
            let database = target_database(profile);

            if options.drop_existing {
                let sql = mysql_drop_recreate_sql(&database)?;
                let out = SysCommand::new("mysql")
                    .env("MYSQL_PWD", password)
                    .arg("-h")
                    .arg(&profile.host)
                    .arg("-P")
                    .arg(profile.port.to_string())
                    .arg("-u")
                    .arg(&profile.username)
                    .arg("-e")
                    .arg(&sql)
                    .output()
                    .map_err(|e| DbError::InternalError(format!("mysql not found: {}", e)))?;
                if !out.status.success() {
                    return Err(DbError::QueryError(
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ));
                }
            }

            let sql = std::fs::read(&options.file_path)
                .map_err(|e| DbError::InternalError(format!("Read file failed: {}", e)))?;

            let mut child = SysCommand::new("mysql")
                // Password via env, not argv (argv is world-readable)
                .env("MYSQL_PWD", password)
                .arg("-h")
                .arg(&profile.host)
                .arg("-P")
                .arg(profile.port.to_string())
                .arg("-u")
                .arg(&profile.username)
                .arg(&database)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| DbError::InternalError(format!("mysql not found: {}", e)))?;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin
                    .write_all(&sql)
                    .map_err(|e| DbError::InternalError(format!("Stdin write: {}", e)))?;
            }
            // Close the pipe or mysql waits forever for more input.
            drop(child.stdin.take());

            let out = child
                .wait_with_output()
                .map_err(|e| DbError::InternalError(format!("Wait failed: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        DbDriver::Sqlite => {
            std::fs::copy(&options.file_path, sqlite_file(profile))
                .map_err(|e| DbError::InternalError(format!("Copy failed: {}", e)))?;
        }

        _ => {
            return Err(DbError::InvalidInput(format!(
                "Restore is not supported for {:?} connections",
                profile.driver
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> BackupOptions {
        BackupOptions {
            include_data: true,
            include_schema: true,
            tables: Vec::new(),
            note: None,
            output_dir: None,
        }
    }

    fn profile(driver: DbDriver) -> ConnectionProfile {
        let mut p = ConnectionProfile::new(
            "id-1".to_string(),
            "test".to_string(),
            driver,
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
        );
        p.database = Some("shop".to_string());
        p
    }

    #[test]
    fn the_data_dir_override_moves_the_backup_folder_under_it() {
        let base = std::env::temp_dir().join(format!("dbhive-backup-dir-{}", std::process::id()));
        let got = backup_dir_in(Some(base.clone()), Some(PathBuf::from("/never/used"))).unwrap();
        assert_eq!(got, base.join("backups"));
        assert!(got.is_dir(), "the folder is created");
        // Without an override the platform data dir is used.
        let other = base.join("platform");
        let got = backup_dir_in(None, Some(other.clone())).unwrap();
        assert_eq!(got, other.join("db-hive").join("backups"));
        assert!(backup_dir_in(None, None).is_err());
    }

    #[test]
    fn a_sqlite_backup_copies_the_database_field_not_the_blank_host() {
        let mut p = profile(DbDriver::Sqlite);
        p.host = String::new();
        p.database = Some("/data/shop.sqlite".to_string());
        assert_eq!(sqlite_file(&p), "/data/shop.sqlite");
        // Older profiles kept the path in host.
        p.database = None;
        p.host = "/old/shop.sqlite".to_string();
        assert_eq!(sqlite_file(&p), "/old/shop.sqlite");
        p.database = Some("  ".to_string());
        assert_eq!(sqlite_file(&p), "/old/shop.sqlite");
    }

    #[test]
    fn safe_name_replaces_every_non_alphanumeric() {
        assert_eq!(safe_name("prod/orders (eu)"), "prod_orders__eu_");
        assert_eq!(safe_name("simple"), "simple");
        // Path separators cannot survive, so a name cannot escape the dir.
        assert!(!safe_name("../../etc/passwd").contains('/'));
        assert!(!safe_name("..\\win").contains('\\'));
    }

    #[test]
    fn extension_follows_the_driver() {
        assert_eq!(backup_extension(&DbDriver::Postgres), "sql");
        assert_eq!(backup_extension(&DbDriver::MySql), "sql");
        assert_eq!(backup_extension(&DbDriver::Sqlite), "sqlite");
        assert_eq!(backup_extension(&DbDriver::MongoDb), "bak");
    }

    #[test]
    fn filename_is_name_timestamp_extension() {
        assert_eq!(
            backup_filename("Local Postgres", 1_700_000_000, &DbDriver::Postgres),
            "Local_Postgres_1700000000.sql"
        );
        assert_eq!(
            backup_filename("db", 42, &DbDriver::Sqlite),
            "db_42.sqlite"
        );
    }

    #[test]
    fn filename_round_trips_back_to_the_connection_name() {
        let name = "Local Postgres";
        let file = backup_filename(name, 1_700_000_000, &DbDriver::Postgres);
        let stem = Path::new(&file).file_stem().unwrap().to_string_lossy();
        assert_eq!(connection_name_from_stem(&stem), name);
    }

    #[test]
    fn stem_without_a_numeric_suffix_is_kept_whole() {
        // The Tauri version turned this into "nightly", inventing a
        // connection that does not exist.
        assert_eq!(connection_name_from_stem("nightly_dump"), "nightly_dump");
        assert_eq!(connection_name_from_stem("plain"), "plain");
        assert_eq!(connection_name_from_stem("trailing_"), "trailing_");
    }

    #[test]
    fn only_known_extensions_are_backups() {
        assert!(is_backup_file(Path::new("/b/x.sql")));
        assert!(is_backup_file(Path::new("/b/x.dump")));
        assert!(is_backup_file(Path::new("/b/x.bak")));
        assert!(is_backup_file(Path::new("/b/x.sqlite")));
        assert!(is_backup_file(Path::new("/b/x.gz")));
        assert!(!is_backup_file(Path::new("/b/notes.txt")));
        assert!(!is_backup_file(Path::new("/b/noextension")));
        // Case matters, matching the Tauri implementation.
        assert!(!is_backup_file(Path::new("/b/x.SQL")));
    }

    #[test]
    fn options_need_schema_or_data() {
        let mut o = opts();
        o.include_data = false;
        o.include_schema = false;
        let err = validate_backup_options(&DbDriver::Postgres, &o).unwrap_err();
        assert!(err.to_string().contains("at least schema or data"));

        o.include_schema = true;
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_ok());
    }

    #[test]
    fn table_names_cannot_be_empty_or_look_like_flags() {
        let mut o = opts();
        o.tables = vec!["  ".to_string()];
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_err());

        o.tables = vec!["--exclude-schema=public".to_string()];
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_err());

        o.tables = vec!["orders".to_string(), "customers".to_string()];
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_ok());
    }

    #[test]
    fn per_table_selection_is_rejected_where_it_cannot_work() {
        let mut o = opts();
        o.tables = vec!["orders".to_string()];
        assert!(validate_backup_options(&DbDriver::Sqlite, &o).is_err());
        assert!(validate_backup_options(&DbDriver::MongoDb, &o).is_err());
        assert!(validate_backup_options(&DbDriver::MySql, &o).is_ok());
    }

    #[test]
    fn output_dir_must_be_absolute_or_absent() {
        let mut o = opts();
        o.output_dir = Some("relative/path".to_string());
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_err());

        o.output_dir = Some("   ".to_string());
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_err());

        o.output_dir = Some("/tmp/db-hive-backups".to_string());
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_ok());

        o.output_dir = None;
        assert!(validate_backup_options(&DbDriver::Postgres, &o).is_ok());
    }

    #[test]
    fn unsupported_drivers_are_rejected_before_spawning_anything() {
        assert!(validate_backup_options(&DbDriver::Redis, &opts()).is_err());
        assert!(validate_backup_options(&DbDriver::SqlServer, &opts()).is_err());
        assert!(validate_backup_options(&DbDriver::Turso, &opts()).is_err());
        assert!(!supports_restore(&DbDriver::MongoDb));
        assert!(supports_backup(&DbDriver::MongoDb));
    }

    #[test]
    fn restore_requires_an_existing_file() {
        let o = RestoreOptions {
            file_path: String::new(),
            drop_existing: false,
        };
        assert!(validate_restore_options(&DbDriver::Postgres, &o).is_err());

        let o = RestoreOptions {
            file_path: "/definitely/not/here.sql".to_string(),
            drop_existing: false,
        };
        assert!(validate_restore_options(&DbDriver::Postgres, &o).is_err());

        let dir = std::env::temp_dir().join("dbhive-backup-test");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("restore-me.sql");
        std::fs::write(&f, b"SELECT 1;").unwrap();
        let o = RestoreOptions {
            file_path: f.to_string_lossy().to_string(),
            drop_existing: false,
        };
        assert!(validate_restore_options(&DbDriver::Postgres, &o).is_ok());
        // Dropping makes no sense when the whole file is replaced.
        let o = RestoreOptions {
            drop_existing: true,
            ..o
        };
        assert!(validate_restore_options(&DbDriver::Sqlite, &o).is_err());
        let _ = std::fs::remove_file(f);
    }

    #[test]
    fn mysql_identifiers_are_quoted_and_backticks_doubled() {
        assert_eq!(quote_mysql_ident("shop"), "`shop`");
        // Without doubling, this would end the identifier and let the rest
        // run as its own statement.
        assert_eq!(quote_mysql_ident("a`b"), "`a``b`");
    }

    #[test]
    fn drop_recreate_sql_is_quoted_and_refuses_an_empty_database() {
        assert_eq!(
            mysql_drop_recreate_sql("shop").unwrap(),
            "DROP DATABASE IF EXISTS `shop`; CREATE DATABASE `shop`;"
        );
        assert_eq!(
            mysql_drop_recreate_sql(" shop ").unwrap(),
            "DROP DATABASE IF EXISTS `shop`; CREATE DATABASE `shop`;"
        );
        assert!(mysql_drop_recreate_sql("").is_err());
        assert!(mysql_drop_recreate_sql("   ").is_err());
    }

    #[test]
    fn postgres_falls_back_to_the_postgres_database() {
        let mut p = profile(DbDriver::Postgres);
        p.database = None;
        assert_eq!(target_database(&p), "postgres");
        p.database = Some(String::new());
        assert_eq!(target_database(&p), "postgres");
        p.database = Some("shop".to_string());
        assert_eq!(target_database(&p), "shop");

        let mut m = profile(DbDriver::MySql);
        m.database = None;
        // MySQL has no equivalent default; the empty string means "server".
        assert_eq!(target_database(&m), "");
    }

    #[test]
    fn tool_names_match_the_driver() {
        assert_eq!(backup_tool(&DbDriver::Neon), "pg_dump");
        assert_eq!(backup_tool(&DbDriver::MySql), "mysqldump");
        assert_eq!(restore_tool(&DbDriver::Supabase), "psql");
        assert_eq!(restore_tool(&DbDriver::MySql), "mysql");
    }

    #[test]
    fn progress_events_arrive_in_order_and_a_silent_sink_is_harmless() {
        let (sink, mut rx) = ProgressSink::channel();
        sink.stage("preparing", "one");
        sink.stage("dumping", "two");
        sink.finished("three");

        let a = rx.try_recv().unwrap();
        let b = rx.try_recv().unwrap();
        let c = rx.try_recv().unwrap();
        assert_eq!((a.stage.as_str(), a.done), ("preparing", false));
        assert_eq!(b.message, "two");
        assert!(c.done && c.error.is_none());

        let failed = {
            let (s, mut r) = ProgressSink::channel();
            s.failed("pg_dump: connection refused");
            r.try_recv().unwrap()
        };
        assert_eq!(failed.error.as_deref(), Some("pg_dump: connection refused"));

        // No receiver: sending must not panic.
        ProgressSink::silent().stage("dumping", "nobody is listening");
    }

    #[test]
    fn listing_a_missing_directory_reports_the_reason() {
        let err = list_backups(Some("/definitely/not/a/directory")).unwrap_err();
        assert!(err.to_string().contains("Cannot read backup dir"));
    }

    #[test]
    fn listing_reports_backups_newest_first_and_skips_other_files() {
        let dir = std::env::temp_dir().join(format!("dbhive-list-test-{}", unix_now()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Shop_1000.sql"), b"a").unwrap();
        std::fs::write(dir.join("Shop_2000.sql"), b"bb").unwrap();
        std::fs::write(dir.join("readme.txt"), b"ignored").unwrap();

        let entries = list_backups(Some(&dir.to_string_lossy())).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.connection_name == "Shop"));
        assert!(entries.iter().all(|e| e.status == BackupStatus::Completed));
        // Sorted by mtime descending; both were just written, so only assert
        // the ordering invariant rather than which file wins.
        assert!(entries[0].created_at >= entries[1].created_at);
        assert_eq!(
            entries.iter().map(|e| e.size_bytes).sum::<u64>(),
            3,
            "sizes come from the file metadata"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
