//! Checking for a new version: when to check, what a check turned up, and what
//! the banner and the About dialog say about it.
//!
//! Pure state, no toolkit, no IO and no clock: the shell passes the time in and
//! does the network call (`dbhive_core::service_updater`). The app checks and
//! reports; it does not install, because releases are not signed yet.

use dbhive_core::service_updater::UpdateStatus;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    /// Nothing checked yet.
    #[default]
    Idle,
    Checking,
    UpToDate { current: String },
    Available { current: String, latest: String, notes: Option<String> },
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct UpdateState {
    pub phase: Phase,
    /// Unix seconds of the last finished check, successful or not.
    last_checked: Option<i64>,
    /// A version whose banner the person closed. Not shown again unless a
    /// newer version appears.
    dismissed: Option<String>,
    /// The check in flight (or the last one) was asked for by the person, so
    /// a failure is worth saying out loud.
    manual: bool,
}

/// What the banner shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub message: String,
}

/// A failed automatic check is retried no sooner than this, so a machine that
/// is offline is not asked to try every minute.
const RETRY_AFTER_FAILURE_SECS: i64 = 60 * 60;

/// The core's error already begins "Update check failed: "; the sentence the
/// person reads says that once.
fn reason(e: &str) -> &str {
    e.strip_prefix("Update check failed: ").unwrap_or(e)
}

impl UpdateState {
    /// Start a check. `false` if one is already running.
    pub fn begin(&mut self, manual: bool) -> bool {
        if self.phase == Phase::Checking {
            return false;
        }
        self.phase = Phase::Checking;
        self.manual = manual;
        true
    }

    /// Record how a check ended.
    pub fn finish(&mut self, result: Result<UpdateStatus, String>, now: i64) {
        self.last_checked = Some(now);
        self.phase = match result {
            Ok(UpdateStatus::UpToDate { current }) => Phase::UpToDate { current },
            Ok(UpdateStatus::Available { current, latest, notes }) => {
                Phase::Available { current, latest, notes }
            }
            Err(e) => Phase::Failed(e),
        };
    }

    /// Whether the automatic check should run now: it is switched on, nothing
    /// is running, and the interval has passed since the last one (a failed
    /// check waits an hour, whatever the interval).
    pub fn due(&self, now: i64, auto: bool, interval_hours: u32) -> bool {
        if !auto || self.phase == Phase::Checking {
            return false;
        }
        let Some(last) = self.last_checked else {
            return true;
        };
        let wait = if matches!(self.phase, Phase::Failed(_)) {
            RETRY_AFTER_FAILURE_SECS
        } else {
            i64::from(interval_hours.max(1)) * 3600
        };
        now - last >= wait
    }

    /// The person closed the banner: for the version it announced, or for the
    /// failure it reported (the About line keeps saying either).
    pub fn dismiss(&mut self) {
        match &self.phase {
            Phase::Available { latest, .. } => self.dismissed = Some(latest.clone()),
            Phase::Failed(_) => self.manual = false,
            _ => {}
        }
    }

    /// The banner, when there is something to announce: a newer version the
    /// person has not already dismissed, or a failure of a check they asked for.
    pub fn banner(&self) -> Option<Banner> {
        match &self.phase {
            Phase::Available { latest, .. } if self.dismissed.as_deref() != Some(latest) => {
                Some(Banner {
                    message: format!("Version {latest} is available."),
                })
            }
            Phase::Failed(e) if self.manual => Some(Banner {
                message: format!("Could not check for updates: {}", reason(e)),
            }),
            _ => None,
        }
    }

    /// The line under "Check for updates" in the About dialog.
    pub fn about_line(&self) -> String {
        match &self.phase {
            Phase::Idle => String::new(),
            Phase::Checking => "Checking for updates…".to_string(),
            Phase::UpToDate { current } => format!("You are up to date (version {current})."),
            Phase::Available { latest, .. } => format!("Version {latest} is available."),
            Phase::Failed(e) => format!("Could not check for updates: {}", reason(e)),
        }
    }

    /// Whether the About dialog should offer the release page.
    pub fn has_update(&self) -> bool {
        matches!(self.phase, Phase::Available { .. })
    }

    pub fn is_checking(&self) -> bool {
        self.phase == Phase::Checking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer() -> UpdateStatus {
        UpdateStatus::Available {
            current: "1.0.0".into(),
            latest: "1.1.0".into(),
            notes: Some("fixes".into()),
        }
    }

    #[test]
    fn a_check_runs_once_at_a_time() {
        let mut s = UpdateState::default();
        assert!(s.begin(false));
        assert!(s.is_checking());
        assert!(!s.begin(true), "a second check must not start over the first");
        s.finish(Ok(newer()), 100);
        assert!(!s.is_checking());
        assert!(s.begin(true));
    }

    #[test]
    fn a_newer_version_shows_a_banner_until_it_is_dismissed() {
        let mut s = UpdateState::default();
        s.begin(false);
        s.finish(Ok(newer()), 100);
        assert_eq!(s.banner(), Some(Banner { message: "Version 1.1.0 is available.".into() }));
        assert!(s.has_update());
        s.dismiss();
        assert_eq!(s.banner(), None, "dismissed for this version");
        // The about line still says so: closing the banner is not denying it.
        assert_eq!(s.about_line(), "Version 1.1.0 is available.");
        // A later check of the same version stays dismissed; a newer one does not.
        s.begin(false);
        s.finish(Ok(newer()), 200);
        assert_eq!(s.banner(), None);
        s.begin(false);
        s.finish(
            Ok(UpdateStatus::Available { current: "1.0.0".into(), latest: "1.2.0".into(), notes: None }),
            300,
        );
        assert_eq!(s.banner().unwrap().message, "Version 1.2.0 is available.");
    }

    #[test]
    fn up_to_date_says_so_in_about_and_not_in_the_banner() {
        let mut s = UpdateState::default();
        s.begin(true);
        s.finish(Ok(UpdateStatus::UpToDate { current: "1.0.0".into() }), 10);
        assert_eq!(s.banner(), None);
        assert_eq!(s.about_line(), "You are up to date (version 1.0.0).");
        assert!(!s.has_update());
    }

    #[test]
    fn only_a_failure_the_person_asked_for_is_announced() {
        let mut quiet = UpdateState::default();
        quiet.begin(false);
        quiet.finish(Err("offline".into()), 10);
        assert_eq!(quiet.banner(), None, "an automatic check that fails stays quiet");
        assert_eq!(quiet.about_line(), "Could not check for updates: offline");

        let mut asked = UpdateState::default();
        asked.begin(true);
        asked.finish(Err("offline".into()), 10);
        assert_eq!(
            asked.banner().unwrap().message,
            "Could not check for updates: offline"
        );
    }

    #[test]
    fn the_failure_sentence_does_not_say_the_check_failed_twice() {
        let mut s = UpdateState::default();
        s.begin(true);
        s.finish(Err("Update check failed: the server replied 500 Internal Server Error".into()), 1);
        assert_eq!(
            s.about_line(),
            "Could not check for updates: the server replied 500 Internal Server Error"
        );
        assert_eq!(s.banner().unwrap().message, s.about_line());
    }

    #[test]
    fn a_failure_banner_can_be_closed_and_about_still_says_it() {
        let mut s = UpdateState::default();
        s.begin(true);
        s.finish(Err("offline".into()), 10);
        assert!(s.banner().is_some());
        s.dismiss();
        assert_eq!(s.banner(), None);
        assert_eq!(s.about_line(), "Could not check for updates: offline");
    }

    #[test]
    fn the_automatic_check_follows_the_setting_and_the_interval() {
        let mut s = UpdateState::default();
        // Never checked: due at once, if switched on.
        assert!(s.due(0, true, 24));
        assert!(!s.due(0, false, 24), "switched off");
        s.begin(false);
        assert!(!s.due(10_000_000, true, 24), "not while one is running");
        s.finish(Ok(newer()), 1_000);
        assert!(!s.due(1_000 + 23 * 3600, true, 24));
        assert!(s.due(1_000 + 24 * 3600, true, 24));
        // An interval of 0 is treated as 1 hour, never a tight loop.
        assert!(!s.due(1_000 + 60, true, 0));
        assert!(s.due(1_000 + 3600, true, 0));
    }

    #[test]
    fn a_failed_check_is_retried_after_an_hour_even_with_a_long_interval() {
        let mut s = UpdateState::default();
        s.begin(false);
        s.finish(Err("offline".into()), 1_000);
        assert!(!s.due(1_000 + 59 * 60, true, 24 * 7));
        assert!(s.due(1_000 + 3600, true, 24 * 7));
    }
}
