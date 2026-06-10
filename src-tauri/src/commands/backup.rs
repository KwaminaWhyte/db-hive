//! Backup and restore commands for DB Hive
//!
//! Provides database backup/restore via native CLI tools (pg_dump, mysqldump,
//! sqlite file copy). Backups are stored in the app data directory.

use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::process::Command as SysCommand;
use std::sync::Mutex;
use tauri::State;
use uuid::Uuid;

use crate::models::backup::{BackupEntry, BackupOptions, BackupStatus, RestoreOptions};
use crate::models::{DbDriver, DbError};
use crate::state::AppState;

fn backup_dir() -> Result<PathBuf, DbError> {
    let dir = dirs::data_dir()
        .ok_or_else(|| DbError::InternalError("Cannot locate data directory".to_string()))?
        .join("db-hive")
        .join("backups");
    std::fs::create_dir_all(&dir)
        .map_err(|e| DbError::InternalError(format!("Cannot create backup dir: {}", e)))?;
    Ok(dir)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[tauri::command]
pub fn get_backup_directory() -> Result<String, DbError> {
    Ok(backup_dir()?.to_string_lossy().to_string())
}

#[tauri::command]
pub fn list_backups(directory: Option<String>) -> Result<Vec<BackupEntry>, DbError> {
    let dir = match directory {
        Some(d) => PathBuf::from(d),
        None => backup_dir()?,
    };

    let mut entries: Vec<BackupEntry> = Vec::new();

    let read_dir = std::fs::read_dir(&dir)
        .map_err(|e| DbError::InternalError(format!("Cannot read backup dir: {}", e)))?;

    for entry in read_dir.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "sql" | "dump" | "bak" | "sqlite" | "gz") {
            continue;
        }

        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let meta = std::fs::metadata(&path)
            .map_err(|e| DbError::InternalError(format!("Metadata error: {}", e)))?;

        let size_bytes = meta.len();
        let created_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Parse connection_name from filename: {name}_{timestamp}.{ext}
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let connection_name = stem
            .rsplit_once('_')
            .map(|(name, _ts)| name.replace('_', " "))
            .unwrap_or_else(|| stem.clone());

        entries.push(BackupEntry {
            id: Uuid::new_v4().to_string(),
            connection_id: String::new(),
            connection_name,
            driver: String::new(),
            file_path: path.to_string_lossy().to_string(),
            file_name,
            size_bytes,
            created_at,
            status: BackupStatus::Completed,
            note: None,
        });
    }

    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

#[tauri::command]
pub async fn create_backup(
    connection_id: String,
    options: BackupOptions,
    state: State<'_, Mutex<AppState>>,
) -> Result<BackupEntry, DbError> {
    let (profile, password) = {
        let st = state.lock().unwrap();
        let profile = st
            .connection_profiles
            .get(&connection_id)
            .cloned()
            .ok_or_else(|| DbError::NotFound(format!("Connection {} not found", connection_id)))?;
        let password = st
            .connection_passwords
            .get(&connection_id)
            .cloned()
            .unwrap_or_default();
        (profile, password)
    };

    let out_dir = match &options.output_dir {
        Some(d) => PathBuf::from(d),
        None => backup_dir()?,
    };

    let ts = unix_now();
    let ext = match profile.driver {
        DbDriver::MongoDb => "bak",
        DbDriver::Sqlite => "sqlite",
        _ => "sql",
    };
    let filename = format!("{}_{}.{}", safe_name(&profile.name), ts, ext);
    let output_path = out_dir.join(&filename);

    match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            let database = profile.database.as_deref().unwrap_or("postgres");
            let mut cmd = SysCommand::new("pg_dump");
            cmd.env("PGPASSWORD", &password)
                .arg("-h").arg(&profile.host)
                .arg("-p").arg(profile.port.to_string())
                .arg("-U").arg(&profile.username)
                .arg("-d").arg(database)
                .arg("-f").arg(&output_path)
                .arg("--no-password");

            if !options.include_data {
                cmd.arg("--schema-only");
            } else if !options.include_schema {
                cmd.arg("--data-only");
            }
            for table in &options.tables {
                cmd.arg("-t").arg(table);
            }

            let out = cmd.output()
                .map_err(|e| DbError::InternalError(format!("pg_dump not found: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        DbDriver::MySql => {
            let database = profile.database.as_deref().unwrap_or("");
            let mut cmd = SysCommand::new("mysqldump");
            cmd.arg("-h").arg(&profile.host)
                .arg("-P").arg(profile.port.to_string())
                .arg("-u").arg(&profile.username)
                .arg(format!("-p{}", password))
                .arg(database);

            if !options.include_data {
                cmd.arg("--no-data");
            }
            if !options.include_schema {
                cmd.arg("--no-create-info");
            }
            for table in &options.tables {
                cmd.arg(table);
            }

            let out = cmd.output()
                .map_err(|e| DbError::InternalError(format!("mysqldump not found: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
            std::fs::write(&output_path, &out.stdout)
                .map_err(|e| DbError::InternalError(format!("Write failed: {}", e)))?;
        }

        DbDriver::Sqlite | DbDriver::Turso => {
            if profile.driver == DbDriver::Turso {
                return Err(DbError::InternalError(
                    "Turso backup: use the Turso dashboard".to_string(),
                ));
            }
            std::fs::copy(&profile.host, &output_path)
                .map_err(|e| DbError::InternalError(format!("Copy failed: {}", e)))?;
        }

        DbDriver::MongoDb => {
            let out_dir_str = out_dir.to_string_lossy().to_string();
            let mut cmd = SysCommand::new("mongodump");
            cmd.arg("--host").arg(&profile.host)
                .arg("--port").arg(profile.port.to_string())
                .arg("--username").arg(&profile.username)
                .arg("--password").arg(&password)
                .arg("--out").arg(&out_dir_str);
            if let Some(db) = &profile.database {
                cmd.arg("--db").arg(db);
            }

            let out = cmd.output()
                .map_err(|e| DbError::InternalError(format!("mongodump not found: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        _ => {
            return Err(DbError::InternalError(
                "Backup not supported for this driver".to_string(),
            ));
        }
    }

    let size_bytes = std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);

    Ok(BackupEntry {
        id: Uuid::new_v4().to_string(),
        connection_id,
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

#[tauri::command]
pub async fn restore_backup(
    connection_id: String,
    options: RestoreOptions,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), DbError> {
    let (profile, password) = {
        let st = state.lock().unwrap();
        let profile = st
            .connection_profiles
            .get(&connection_id)
            .cloned()
            .ok_or_else(|| DbError::NotFound(format!("Connection {} not found", connection_id)))?;
        let password = st
            .connection_passwords
            .get(&connection_id)
            .cloned()
            .unwrap_or_default();
        (profile, password)
    };

    match profile.driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            let database = profile.database.as_deref().unwrap_or("postgres");
            let out = SysCommand::new("psql")
                .env("PGPASSWORD", &password)
                .arg("-h").arg(&profile.host)
                .arg("-p").arg(profile.port.to_string())
                .arg("-U").arg(&profile.username)
                .arg("-d").arg(database)
                .arg("-f").arg(&options.file_path)
                .output()
                .map_err(|e| DbError::InternalError(format!("psql not found: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        DbDriver::MySql => {
            let database = profile.database.as_deref().unwrap_or("");
            let sql = std::fs::read(&options.file_path)
                .map_err(|e| DbError::InternalError(format!("Read file failed: {}", e)))?;

            let mut child = SysCommand::new("mysql")
                .arg("-h").arg(&profile.host)
                .arg("-P").arg(profile.port.to_string())
                .arg("-u").arg(&profile.username)
                .arg(format!("-p{}", password))
                .arg(database)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| DbError::InternalError(format!("mysql not found: {}", e)))?;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(&sql)
                    .map_err(|e| DbError::InternalError(format!("Stdin write: {}", e)))?;
            }

            let out = child.wait_with_output()
                .map_err(|e| DbError::InternalError(format!("Wait failed: {}", e)))?;
            if !out.status.success() {
                return Err(DbError::QueryError(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
        }

        DbDriver::Sqlite => {
            std::fs::copy(&options.file_path, &profile.host)
                .map_err(|e| DbError::InternalError(format!("Copy failed: {}", e)))?;
        }

        _ => {
            return Err(DbError::InternalError(
                "Restore not supported for this driver".to_string(),
            ));
        }
    }

    Ok(())
}

#[tauri::command]
pub fn delete_backup(file_path: String) -> Result<(), DbError> {
    std::fs::remove_file(&file_path)
        .map_err(|e| DbError::InternalError(format!("Delete failed: {}", e)))
}

#[tauri::command]
pub fn open_backup_directory() -> Result<(), DbError> {
    let dir = backup_dir()?.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    SysCommand::new("open")
        .arg(&dir)
        .spawn()
        .map_err(|e| DbError::InternalError(e.to_string()))?;

    #[cfg(target_os = "windows")]
    SysCommand::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| DbError::InternalError(e.to_string()))?;

    #[cfg(target_os = "linux")]
    SysCommand::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(|e| DbError::InternalError(e.to_string()))?;

    Ok(())
}
