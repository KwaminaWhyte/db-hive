//! The workspace driven end to end: the real window, the real controller, a
//! real SQLite file.
//!
//! Covers what the logic tests and snapshots cannot: that a click on a saved
//! connection lands in a workspace with a query tab, that Run executes the
//! tab's SQL and shows the result, that a destructive statement stops at a
//! confirmation (and does nothing when cancelled), that confirming runs it,
//! that history lands on disk, and that closing a tab with SQL asks first.
//!
//! Every store is redirected to a temp directory, so this never touches the
//! developer's own saved connections or history.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::Screen;
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn sqlite_count(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM widgets", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn run_a_query_guard_a_delete_record_history_and_close_the_tab() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-controller-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("widgets.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO widgets (name) VALUES ('sprocket'), ('gizmo')",
            [],
        )
        .unwrap();
    }

    // Seed the saved connection the way the app would have stored it.
    {
        let mut profile = ConnectionProfile::new(
            "widgets-db".to_string(),
            "Widgets".to_string(),
            DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        profile.database = Some(db.to_string_lossy().to_string());
        let mut state = AppState::new();
        state.add_profile(profile);
        state.save_profiles_to_store().unwrap();
    }

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, db2, dir2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        db.clone(),
        dir.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(20) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            // Connect, and land in a workspace with one empty query tab.
            0 => {
                w.invoke_connect("widgets-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tabs().row_count() == 1
                && w.get_workspace_tables().row_count() == 1 =>
            {
                if !w.get_workspace_active_is_query() {
                    return bail("the first tab should be a query tab".into());
                }
                // Run with no SQL does nothing.
                w.invoke_workspace_run_query();
                w.invoke_workspace_editor_edited("SELECT name FROM widgets ORDER BY id".into());
                w.invoke_workspace_run_query();
                step2.set(2);
            }
            2 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                if w.get_workspace_row_count() != 2 {
                    return bail(format!(
                        "expected 2 rows, got {}",
                        w.get_workspace_row_count()
                    ));
                }
                if !w.get_workspace_result_summary().starts_with("2 rows") {
                    return bail(format!(
                        "summary was {:?}",
                        w.get_workspace_result_summary()
                    ));
                }
                // A destructive statement must stop at the confirmation.
                w.invoke_workspace_editor_edited("DELETE FROM widgets".into());
                w.invoke_workspace_run_query();
                step2.set(3);
            }
            3 if w.get_show_confirm() => {
                if w.get_confirm_title() != "Run this statement?"
                    || !w.get_confirm_message().contains("every row of widgets")
                {
                    return bail(format!(
                        "wrong confirmation: {:?} / {:?}",
                        w.get_confirm_title(),
                        w.get_confirm_message()
                    ));
                }
                // Cancelling must leave the data alone.
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                if sqlite_count(&db2) != 2 {
                    return bail("cancelling the confirmation still deleted rows".into());
                }
                w.invoke_workspace_run_query();
                step2.set(4);
            }
            4 if w.get_show_confirm() => {
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(5);
            }
            5 if !w.get_workspace_running()
                && w.get_workspace_result_message().contains("affected") =>
            {
                if sqlite_count(&db2) != 0 {
                    return bail("confirming did not run the DELETE".into());
                }
                step2.set(6);
            }
            // History is persisted asynchronously; wait for it to land.
            6 => {
                let history =
                    std::fs::read_to_string(dir2.join("history.json")).unwrap_or_default();
                if history.contains("DELETE FROM widgets")
                    && history.contains("SELECT name FROM widgets")
                {
                    // Closing a tab that has SQL in it asks first.
                    w.invoke_workspace_tab_closed(0);
                    step2.set(7);
                } else if started.elapsed() > Duration::from_secs(15) {
                    bail("history.json never received both queries".into());
                }
            }
            7 if w.get_show_confirm() => {
                if w.get_confirm_title() != "Close this tab?" {
                    return bail(format!(
                        "close confirmation was {:?}",
                        w.get_confirm_title()
                    ));
                }
                if w.get_workspace_tabs().row_count() != 1 {
                    return bail("the tab closed before it was confirmed".into());
                }
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(8);
            }
            8 => {
                if w.get_workspace_tabs().row_count() != 0 {
                    return bail("confirming did not close the tab".into());
                }
                step2.set(99);
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });

    w.run().unwrap();
    drop(timer);
    let _ = std::fs::remove_dir_all(&dir);

    if let Some(msg) = failure.borrow().as_ref() {
        panic!("{msg}");
    }
    assert_eq!(step.get(), 99, "the scenario did not run to its end");
}
