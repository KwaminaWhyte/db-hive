//! Errors raised by the plugin subsystem.
//!
//! Deliberately dependency-free (std only, no `thiserror`): this type is
//! shared by every consumer of the plugin engine, including the native shell
//! during the migration, and none of them should have to take a derive crate
//! to name an error.
//!
//! `Io` and `Json` carry the formatted message rather than the source error
//! so that `PluginError` stays `Clone + PartialEq`, which makes the pure
//! logic below testable with plain `assert_eq!`.

use std::fmt;

pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    NotFound(String),
    AlreadyInstalled(String),
    InvalidManifest(String),
    /// The plugin did not declare this permission in its manifest.
    PermissionDenied(String),
    IntegrityCheckFailed(String),
    ExecutionError(String),
    IncompatibleVersion {
        required: String,
        current: String,
    },
    MissingDependency(String),
    NetworkError(String),
    /// A capability a plugin asked for that this build intentionally does
    /// not expose. Distinct from `PermissionDenied`, which is about the
    /// manifest; this one no manifest can unlock.
    NotSupported(String),
    Io(String),
    Json(String),
    Other(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::NotFound(id) => write!(f, "Plugin not found: {}", id),
            PluginError::AlreadyInstalled(id) => write!(f, "Plugin already installed: {}", id),
            PluginError::InvalidManifest(m) => write!(f, "Invalid manifest: {}", m),
            PluginError::PermissionDenied(p) => write!(f, "Permission denied: {}", p),
            PluginError::IntegrityCheckFailed(m) => write!(f, "Integrity check failed: {}", m),
            PluginError::ExecutionError(m) => write!(f, "Plugin execution error: {}", m),
            PluginError::IncompatibleVersion { required, current } => write!(
                f,
                "Incompatible version: requires {}, current is {}",
                required, current
            ),
            PluginError::MissingDependency(d) => write!(f, "Missing dependency: {}", d),
            PluginError::NetworkError(m) => write!(f, "Network error: {}", m),
            PluginError::NotSupported(m) => write!(f, "Not available to plugins: {}", m),
            PluginError::Io(m) => write!(f, "IO error: {}", m),
            PluginError::Json(m) => write!(f, "JSON error: {}", m),
            PluginError::Other(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        PluginError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        PluginError::Json(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_the_thing_that_failed() {
        assert_eq!(
            PluginError::NotFound("com.x.y".into()).to_string(),
            "Plugin not found: com.x.y"
        );
        assert_eq!(
            PluginError::PermissionDenied("Write Files".into()).to_string(),
            "Permission denied: Write Files"
        );
        assert_eq!(
            PluginError::IncompatibleVersion {
                required: "1.0.0".into(),
                current: "0.21.0".into(),
            }
            .to_string(),
            "Incompatible version: requires 1.0.0, current is 0.21.0"
        );
    }

    #[test]
    fn io_and_json_errors_convert() {
        let io: PluginError = std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into();
        assert_eq!(io, PluginError::Io("gone".to_string()));

        let json: PluginError = serde_json::from_str::<serde_json::Value>("{oops")
            .unwrap_err()
            .into();
        assert!(matches!(json, PluginError::Json(_)));
    }
}
