//! Checking for a newer version: an automatic check that follows Settings
//! (on or off, and its interval), a button in About for a check on demand, a
//! banner when a newer version exists, and a way to open the release page.
//!
//! The app checks and reports; it does not download or install, because
//! releases are not signed yet (see `dbhive_core::service_updater`).
//!
//! The rules (when a check is due, what a result means, what the banner and the
//! About line say) are `dbhive_ui_model::update_state`. This file runs the
//! network call off the UI thread and projects the state into `UpdateBridge`.
//!
//! Two guards keep development and tests off the real update server: the first
//! automatic check waits a little after start-up, and when `DBHIVE_DATA_DIR`
//! is set (a test or a throwaway profile) the automatic check does not run
//! unless `DBHIVE_UPDATE_URL` names a manifest to use. `DBHIVE_UPDATE_URL` also
//! points a development build at a local manifest.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::rc::Rc;
use std::time::Duration;

use dbhive_core::service_updater::{check_at, MANIFEST_URL, RELEASES_URL};
use dbhive_ui_model::update_state::UpdateState;
use slint::{ComponentHandle, Timer, TimerMode};

use super::{now_secs, with_ctx, Ctx};
use crate::{runtime, AppWindow, UpdateBridge};

/// How often the shell asks the model whether a check is due.
const TICK: Duration = Duration::from_secs(30);
/// The first automatic check waits at least this long after start-up.
const STARTUP_DELAY_SECS: i64 = 15;

/// With an explicit manifest URL (a development build or a test) there is no
/// real server to be gentle with, so the clock runs fast.
fn explicit_url() -> bool {
    std::env::var_os("DBHIVE_UPDATE_URL").is_some()
}

fn tick() -> Duration {
    if explicit_url() {
        Duration::from_millis(200)
    } else {
        TICK
    }
}

fn startup_delay() -> i64 {
    if explicit_url() {
        0
    } else {
        STARTUP_DELAY_SECS
    }
}

#[cfg(target_os = "macos")]
const OPEN_CMD: &str = "open";
#[cfg(target_os = "windows")]
const OPEN_CMD: &str = "explorer";
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
const OPEN_CMD: &str = "xdg-open";

#[derive(Default)]
pub(super) struct UpdateSession {
    state: UpdateState,
    /// Unix seconds when the app started, for the start-up delay.
    started: Option<i64>,
}

thread_local! {
    static TIMER: Timer = Timer::default();
}

fn manifest_url() -> String {
    std::env::var("DBHIVE_UPDATE_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| MANIFEST_URL.to_string())
}

/// Whether this process may reach the real update server on its own: not when
/// it runs on a redirected data directory (a test or a throwaway profile),
/// unless a manifest URL was given explicitly.
fn auto_allowed() -> bool {
    std::env::var_os(dbhive_core::storage::DATA_DIR_ENV).is_none() || explicit_url()
}

impl Ctx {
    /// Ask the model whether the automatic check is due, and run it if so.
    fn updates_tick(self: &Rc<Self>) {
        let now = now_secs();
        let due = {
            let mut s = self.session.borrow_mut();
            let started = *s.updates.started.get_or_insert(now);
            let general = s.settings.editor.saved().general.clone();
            now - started >= startup_delay()
                && auto_allowed()
                && s.updates
                    .state
                    .due(now, general.auto_check_updates, general.update_check_interval_hours)
        };
        if due {
            self.updates_check(false);
        }
    }

    /// Run a check now. `manual` is a check the person asked for.
    pub(super) fn updates_check(self: &Rc<Self>, manual: bool) {
        if !self.session.borrow_mut().updates.state.begin(manual) {
            return;
        }
        self.updates_project();
        let url = manifest_url();
        let current = env!("CARGO_PKG_VERSION").to_string();
        runtime::spawn(
            async move { check_at(&url, &current).await.map_err(|e| e.to_string()) },
            move |result| {
                with_ctx(|ctx| {
                    ctx.session
                        .borrow_mut()
                        .updates
                        .state
                        .finish(result, now_secs());
                    ctx.updates_project();
                });
            },
        );
    }

    fn updates_dismiss(&self) {
        self.session.borrow_mut().updates.state.dismiss();
        self.updates_project();
    }

    fn updates_open_release(&self) {
        if let Err(e) = std::process::Command::new(OPEN_CMD).arg(RELEASES_URL).spawn() {
            eprintln!("Warning: could not open the release page: {e}");
        }
    }

    fn updates_project(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let s = self.session.borrow();
        let st = &s.updates.state;
        let b = w.global::<UpdateBridge>();
        let banner = st.banner();
        b.set_banner_visible(banner.is_some());
        b.set_banner_message(banner.map(|x| x.message).unwrap_or_default().into());
        b.set_about_status(st.about_line().into());
        b.set_checking(st.is_checking());
        b.set_has_update(st.has_update());
    }
}

pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let b = window.global::<UpdateBridge>();
    {
        let ctx = ctx.clone();
        b.on_check(move || ctx.updates_check(true));
    }
    {
        let ctx = ctx.clone();
        b.on_dismiss(move || ctx.updates_dismiss());
    }
    {
        let ctx = ctx.clone();
        b.on_open_release(move || ctx.updates_open_release());
    }
    TIMER.with(|t| {
        t.start(TimerMode::Repeated, tick(), || {
            with_ctx(|ctx| ctx.updates_tick());
        });
    });
}
