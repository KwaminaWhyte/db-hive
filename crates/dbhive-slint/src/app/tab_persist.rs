//! Keep a workspace's tabs across restarts: the SQL typed into query tabs and
//! which tables were open, per connection and database.
//!
//! The rules (what is saved, what is not worth saving, how a saved value is
//! read back) are `dbhive_ui_model::saved_tabs`. This file decides when:
//! a restore once the table list has loaded, an autosave once a second when
//! something changed, and a final save when the workspace is left. Results are
//! never saved.
//!
//! Saving stays off until the restore attempt has finished, so the blank
//! start-up tab can never overwrite what was saved.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::rc::Rc;
use std::time::Duration;

use dbhive_core::storage::JsonStore;
use dbhive_ui_model::saved_tabs::{self, SavedTab, SavedWorkspace};
use dbhive_ui_model::workspace_tabs::Tabs;
use slint::{Timer, TimerMode};

use super::Ctx;
use crate::AppWindow;

const STORE: &str = "workspace_tabs.json";
const SAVE_EVERY: Duration = Duration::from_millis(1000);

#[derive(Default)]
pub(super) struct TabPersist {
    /// Store key of the open workspace; empty when there is none.
    key: String,
    /// False until the restore attempt has finished.
    ready: bool,
    /// What was last written (or read), to skip writes when nothing changed.
    last: Option<SavedWorkspace>,
}

thread_local! {
    static TIMER: Timer = Timer::default();
}

impl Ctx {
    /// A workspace has opened: remember which store entry is its own.
    pub(super) fn tabs_persist_begin(&self, key: String) {
        self.session.borrow_mut().tab_persist = TabPersist {
            key,
            ready: false,
            last: None,
        };
    }

    /// The table list has loaded (or failed to): put back the saved tabs if
    /// the workspace is still the untouched start state, then let saving begin.
    pub(super) fn tabs_restore(self: &Rc<Self>, tables_loaded: bool) {
        let (key, saved) = {
            let s = self.session.borrow();
            if s.tab_persist.ready || s.tab_persist.key.is_empty() {
                return;
            }
            let key = s.tab_persist.key.clone();
            let saved = read(&key).filter(|_| saved_tabs::is_pristine(&s.workspace.tabs));
            (key, saved)
        };

        let mut restored = None;
        if let Some(saved) = saved.filter(|_| tables_loaded) {
            let exists: std::collections::HashSet<String> = self
                .session
                .borrow()
                .workspace
                .tables
                .iter()
                .map(|t| t.name.clone())
                .collect();
            let saved = saved.without_missing_tables(|n| exists.contains(n));
            if saved.is_worth_saving() {
                self.session.borrow_mut().workspace.tabs = Tabs::default();
                let mut ids = Vec::new();
                for t in &saved.tabs {
                    match t {
                        SavedTab::Query { sql } => {
                            let id = self
                                .session
                                .borrow_mut()
                                .workspace
                                .tabs
                                .open_query(sql.as_str());
                            ids.push(id);
                        }
                        SavedTab::Table { name } => {
                            self.select_table(name);
                            let id = self.session.borrow().workspace.tabs.active().map(|t| t.id);
                            ids.extend(id);
                        }
                    }
                }
                if let Some(id) = saved.active.and_then(|i| ids.get(i)).copied() {
                    self.session.borrow_mut().workspace.tabs.select(id);
                }
                self.refresh_workspace();
                restored = Some(saved);
            }
        }

        let mut s = self.session.borrow_mut();
        // Only the same workspace may start saving: it could have been left
        // while the table list loaded.
        if s.tab_persist.key == key {
            // The baseline autosave compares against. If nothing was put
            // back and the workspace is still the blank start state (the table
            // list failed to load, say), the baseline is that blank state, so
            // the saved tabs are not deleted until the person changes
            // something. If they already typed, the baseline is what is stored,
            // so their work is saved at the next tick.
            let baseline = if saved_tabs::is_pristine(&s.workspace.tabs) {
                SavedWorkspace::capture(&s.workspace.tabs)
            } else {
                read(&key).unwrap_or_default()
            };
            s.tab_persist.ready = true;
            s.tab_persist.last = Some(restored.unwrap_or(baseline));
        }
    }

    /// Write the tabs if they differ from what is stored.
    pub(super) fn tabs_save_if_changed(&self) {
        let (key, now) = {
            let s = self.session.borrow();
            let p = &s.tab_persist;
            if !p.ready || p.key.is_empty() {
                return;
            }
            let now = SavedWorkspace::capture(&s.workspace.tabs);
            // A blank workspace and "nothing stored" are the same thing.
            let unchanged = match &p.last {
                Some(last) => *last == now,
                None => !now.is_worth_saving(),
            };
            if unchanged {
                return;
            }
            (p.key.clone(), now)
        };
        if write(&key, &now) {
            self.session.borrow_mut().tab_persist.last = Some(now);
        }
    }

    /// Save one last time, before the workspace's state is thrown away.
    pub(super) fn tabs_flush(&self) {
        self.tabs_save_if_changed();
        self.session.borrow_mut().tab_persist = TabPersist::default();
    }
}

/// The saved tabs for `key`, if there are readable ones. A store that cannot
/// be read is the same as nothing saved: restoring tabs is a convenience and
/// must never keep a connection from opening.
fn read(key: &str) -> Option<SavedWorkspace> {
    let store = match JsonStore::open(STORE) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: could not read saved tabs: {e}");
            return None;
        }
    };
    store.get(key).and_then(SavedWorkspace::from_value)
}

/// Store `tabs` under `key`, or remove the entry when there is nothing worth
/// keeping. Returns whether the store was updated.
fn write(key: &str, tabs: &SavedWorkspace) -> bool {
    let mut store = match JsonStore::open(STORE) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: could not read saved tabs: {e}");
            return false;
        }
    };
    if tabs.is_worth_saving() {
        store.set(key, tabs.to_value());
    } else {
        store.delete(key);
    }
    match store.save() {
        Ok(()) => true,
        Err(e) => {
            eprintln!("Warning: could not save tabs: {e}");
            false
        }
    }
}

pub(super) fn wire(_window: &AppWindow, _ctx: &Rc<Ctx>) {
    TIMER.with(|t| {
        t.start(TimerMode::Repeated, SAVE_EVERY, || {
            super::for_each_ctx(|ctx| ctx.tabs_save_if_changed());
        });
    });
}
