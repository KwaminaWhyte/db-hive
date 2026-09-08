//! Update checking.
//!
//! Replaces `tauri-plugin-updater`, which fetched a `latest.json` manifest,
//! verified it against a minisign public key, then downloaded and swapped
//! the app in place.
//!
//! This implementation checks and reports; it does not install. That is a
//! deliberate limit, not an oversight:
//!
//!   - The signing story is not in place. The Tauri config carries a public
//!     key, but the release build fails with "A public key has been found,
//!     but no private key", so nothing is currently signed. An updater that
//!     downloads and executes an unverified binary is a remote code
//!     execution channel, and shipping one is worse than shipping none.
//!   - Replacing a running binary is platform-specific and easy to get
//!     half-right in a way that leaves an unlaunchable app.
//!
//! So: check the manifest, compare versions honestly, and hand the user the
//! release page. Wiring real installation means signing releases first.

use serde::{Deserialize, Serialize};

use crate::models::DbError;

/// Where the Tauri shell looked, kept so both agree on what "latest" means.
pub const MANIFEST_URL: &str =
    "https://github.com/KwaminaWhyte/db-hive/releases/latest/download/latest.json";
pub const RELEASES_URL: &str = "https://github.com/KwaminaWhyte/db-hive/releases/latest";

/// The subset of the Tauri updater manifest that matters here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate { current: String },
    Available { current: String, latest: String, notes: Option<String> },
}

impl UpdateStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, UpdateStatus::Available { .. })
    }
}

/// Split a semver-ish string into comparable numbers plus any pre-release.
///
/// Returns `None` for something that is not a version at all, so a manifest
/// serving an error page cannot be read as "newer".
fn parse_version(v: &str) -> Option<(Vec<u64>, Option<String>)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };
    let parts: Vec<u64> = core
        .split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    if parts.is_empty() {
        return None;
    }
    Some((parts, pre))
}

/// Whether `candidate` is newer than `current`.
///
/// A release beats a pre-release of the same numbers (1.0.0 > 1.0.0-beta),
/// which is what semver says and what matters here: the app's own version is
/// currently a `-beta`, so getting this backwards would either offer a
/// downgrade or hide a real release.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some((c, cpre)), Some((n, npre))) = (parse_version(candidate), parse_version(current))
    else {
        return false;
    };
    let len = c.len().max(n.len());
    for i in 0..len {
        let a = c.get(i).copied().unwrap_or(0);
        let b = n.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    match (cpre, npre) {
        (None, Some(_)) => true,   // 1.0.0 beats 1.0.0-beta
        (Some(_), None) => false,
        (Some(a), Some(b)) => a > b,
        (None, None) => false,
    }
}

/// Compare a fetched manifest against the running version.
pub fn status_for(manifest: &UpdateManifest, current: &str) -> UpdateStatus {
    if is_newer(&manifest.version, current) {
        UpdateStatus::Available {
            current: current.to_string(),
            latest: manifest.version.clone(),
            notes: manifest.notes.clone(),
        }
    } else {
        UpdateStatus::UpToDate {
            current: current.to_string(),
        }
    }
}

/// Fetch the manifest and compare it with `current_version`.
pub async fn check(current_version: &str) -> Result<UpdateStatus, DbError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("db-hive/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| DbError::InternalError(format!("Could not build HTTP client: {}", e)))?;

    let response = client
        .get(MANIFEST_URL)
        .send()
        .await
        .map_err(|e| DbError::InternalError(format!("Update check failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(DbError::InternalError(format!(
            "Update check failed: the server replied {}",
            response.status()
        )));
    }

    let manifest: UpdateManifest = response
        .json()
        .await
        .map_err(|e| DbError::InternalError(format!("Update manifest was not readable: {}", e)))?;

    Ok(status_for(&manifest, current_version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_minor_and_major_are_all_detected() {
        assert!(is_newer("0.21.1", "0.21.0"));
        assert!(is_newer("0.22.0", "0.21.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("0.21.0", "0.21.0"));
        assert!(!is_newer("0.20.9", "0.21.0"));
    }

    #[test]
    fn a_release_beats_a_prerelease_of_the_same_version() {
        // The app currently ships as 0.21.0-beta, so this is the live case.
        assert!(is_newer("0.21.0", "0.21.0-beta"));
        assert!(!is_newer("0.21.0-beta", "0.21.0"));
    }

    #[test]
    fn a_v_prefix_is_tolerated() {
        assert!(is_newer("v0.22.0", "0.21.0"));
    }

    #[test]
    fn differing_component_counts_compare_sensibly() {
        assert!(is_newer("0.21.1", "0.21"));
        assert!(!is_newer("0.21", "0.21.0"));
    }

    #[test]
    fn garbage_is_never_treated_as_newer() {
        // A manifest URL that returns an HTML error page must not be read
        // as an available update.
        assert!(!is_newer("<!DOCTYPE html>", "0.21.0"));
        assert!(!is_newer("", "0.21.0"));
        assert!(!is_newer("latest", "0.21.0"));
    }

    #[test]
    fn status_reports_the_running_version_either_way() {
        let m = UpdateManifest {
            version: "0.22.0".into(),
            notes: Some("fixes".into()),
            pub_date: None,
        };
        match status_for(&m, "0.21.0") {
            UpdateStatus::Available { current, latest, notes } => {
                assert_eq!(current, "0.21.0");
                assert_eq!(latest, "0.22.0");
                assert_eq!(notes.as_deref(), Some("fixes"));
            }
            other => panic!("expected an update, got {:?}", other),
        }
        let same = UpdateManifest {
            version: "0.21.0".into(),
            notes: None,
            pub_date: None,
        };
        assert!(!status_for(&same, "0.21.0").is_available());
    }

    #[test]
    fn manifest_parses_with_only_a_version() {
        let m: UpdateManifest = serde_json::from_str(r#"{"version":"1.2.3"}"#).unwrap();
        assert_eq!(m.version, "1.2.3");
        assert!(m.notes.is_none());
    }
}
