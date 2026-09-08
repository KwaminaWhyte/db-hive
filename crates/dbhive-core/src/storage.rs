//! JSON-backed key/value stores.
//!
//! Replaces `tauri_plugin_store`, which was the only reason `AppState`
//! needed an `AppHandle`. The on-disk format and location are deliberately
//! identical to what the plugin wrote, so an existing installation keeps its
//! saved connections, history and snippets when it moves to the native
//! shell:
//!
//!   macOS    ~/Library/Application Support/com.kwamina.db-hive/<name>
//!   Linux    ~/.local/share/com.kwamina.db-hive/<name>
//!   Windows  %APPDATA%\com.kwamina.db-hive\<name>
//!
//! Each file is a flat JSON object. `profiles.json` holds a single
//! `"profiles"` key, and so on; the plugin's own format, unchanged.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::models::DbError;

/// Application identifier, matching `tauri.conf.json`. Changing this orphans
/// every existing user's data, so it is deliberately hard-coded rather than
/// derived.
pub const APP_IDENTIFIER: &str = "com.kwamina.db-hive";

/// Directory the stores live in, created if absent.
pub fn app_data_dir() -> Result<PathBuf, DbError> {
    let base = dirs::data_dir().ok_or_else(|| {
        DbError::InternalError("Could not resolve the platform data directory".into())
    })?;
    let dir = base.join(APP_IDENTIFIER);
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| {
            DbError::InternalError(format!("Failed to create {}: {}", dir.display(), e))
        })?;
    }
    Ok(dir)
}

/// One JSON file, loaded into memory and written back on `save`.
///
/// Reads are in-memory and cheap; `save` rewrites the whole file, which is
/// what the plugin did too. These files are small (profiles, history capped
/// at 1000 entries, snippets), so this stays well clear of mattering.
#[derive(Debug, Clone)]
pub struct JsonStore {
    path: PathBuf,
    data: HashMap<String, Value>,
}

impl JsonStore {
    /// Open the named store, e.g. `"profiles.json"`.
    ///
    /// A missing or unreadable file yields an empty store rather than an
    /// error: that is a first run, not a failure. A file that exists but
    /// holds invalid JSON *is* an error, because silently discarding a
    /// user's saved connections would be worse than refusing to start.
    pub fn open(name: &str) -> Result<Self, DbError> {
        let path = app_data_dir()?.join(name);
        Self::open_at(path)
    }

    pub fn open_at(path: PathBuf) -> Result<Self, DbError> {
        let data = if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| {
                DbError::InternalError(format!("Failed to read {}: {}", path.display(), e))
            })?;
            if raw.trim().is_empty() {
                HashMap::new()
            } else {
                serde_json::from_str(&raw).map_err(|e| {
                    DbError::InternalError(format!(
                        "{} is not valid JSON: {}. Refusing to overwrite it.",
                        path.display(),
                        e
                    ))
                })?
            }
        } else {
            HashMap::new()
        };
        Ok(Self { path, data })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        self.data.insert(key.into(), value);
    }

    pub fn delete(&mut self, key: &str) -> Option<Value> {
        self.data.remove(key)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write the store back to disk.
    ///
    /// Writes to a sibling temp file and renames over the target, so an
    /// interrupted write cannot leave a user with a truncated profiles.json.
    pub fn save(&self) -> Result<(), DbError> {
        let serialized = serde_json::to_string_pretty(&self.data)
            .map_err(|e| DbError::InternalError(format!("Failed to serialize store: {}", e)))?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                DbError::InternalError(format!("Failed to create {}: {}", parent.display(), e))
            })?;
        }

        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serialized).map_err(|e| {
            DbError::InternalError(format!("Failed to write {}: {}", tmp.display(), e))
        })?;
        fs::rename(&tmp, &self.path).map_err(|e| {
            DbError::InternalError(format!("Failed to replace {}: {}", self.path.display(), e))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dbhive-store-test-{}-{}", std::process::id(), name))
    }

    #[test]
    fn missing_file_opens_empty() {
        let p = temp_path("missing.json");
        let _ = fs::remove_file(&p);
        let store = JsonStore::open_at(p).unwrap();
        assert!(store.get("profiles").is_none());
    }

    #[test]
    fn round_trips_through_disk() {
        let p = temp_path("roundtrip.json");
        let _ = fs::remove_file(&p);

        let mut store = JsonStore::open_at(p.clone()).unwrap();
        store.set("profiles", serde_json::json!([{"id": "a"}]));
        store.save().unwrap();

        let reopened = JsonStore::open_at(p.clone()).unwrap();
        assert_eq!(
            reopened.get("profiles").unwrap(),
            &serde_json::json!([{"id": "a"}])
        );
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn corrupt_file_errors_rather_than_silently_emptying() {
        let p = temp_path("corrupt.json");
        fs::write(&p, "{not json").unwrap();
        assert!(JsonStore::open_at(p.clone()).is_err());
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn empty_file_is_treated_as_empty_store() {
        let p = temp_path("empty.json");
        fs::write(&p, "").unwrap();
        assert!(JsonStore::open_at(p.clone()).unwrap().get("x").is_none());
        let _ = fs::remove_file(&p);
    }
}
