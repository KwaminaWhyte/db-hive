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
    check_at(MANIFEST_URL, current_version).await
}

/// [`check`] against a manifest at `url`, for tests and for pointing a
/// development build at a local server.
pub async fn check_at(url: &str, current_version: &str) -> Result<UpdateStatus, DbError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("db-hive/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| DbError::InternalError(format!("Could not build HTTP client: {}", e)))?;

    let response = client
        .get(url)
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

    /// One-shot HTTP server on an ephemeral port that answers every request
    /// with `status` and `body`, for exercising `check_at` end to end.
    async fn serve(status: &'static str, body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(reply.as_bytes()).await;
            }
        });
        format!("http://{addr}/latest.json")
    }

    #[tokio::test]
    async fn check_at_reports_newer_same_and_older_manifests() {
        let url = serve("200 OK", r#"{"version":"2.0.0","notes":"big"}"#).await;
        match check_at(&url, "1.9.9").await.unwrap() {
            UpdateStatus::Available { latest, notes, .. } => {
                assert_eq!(latest, "2.0.0");
                assert_eq!(notes.as_deref(), Some("big"));
            }
            other => panic!("expected an update, got {other:?}"),
        }
        assert!(!check_at(&url, "2.0.0").await.unwrap().is_available());
        assert!(!check_at(&url, "3.0.0").await.unwrap().is_available());
    }

    #[tokio::test]
    async fn check_at_fails_loudly_on_a_server_error_or_an_unreadable_manifest() {
        let down = serve("500 Internal Server Error", "oops").await;
        let e = check_at(&down, "1.0.0").await.unwrap_err().to_string();
        assert!(e.contains("500"), "error was {e:?}");

        // An error page served as 200 must not be read as "newer".
        let junk = serve("200 OK", "<html>not a manifest</html>").await;
        assert!(check_at(&junk, "1.0.0").await.is_err());
        let not_a_version = serve("200 OK", r#"{"version":"<html>"}"#).await;
        assert!(!check_at(&not_a_version, "1.0.0").await.map(|s| s.is_available()).unwrap_or(false));
    }
}
