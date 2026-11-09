//! Stop, Clear, the query grid's selection and the tab bar's menu, driven
//! through the real window and controller against a real SQLite file.
//!
//! What the pure tests cannot prove: that the Stop button reaches the running
//! statement, that a stopped tab shows the neutral cancelled state instead of
//! an error, that a cell click lands a selection the copy bar can act on, and
//! that the tab menu closes the tabs it names (asking first only once for a
//! bulk close).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{QueryState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

#[test]
fn stop_clear_select_and_the_tab_menu_work_through_the_window() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-query-tools-{}", std::process::id()));
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

    let (weak, fail, step2) = (w.as_weak(), failure.clone(), step.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(60) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        let q = w.global::<QueryState>();
        match step2.get() {
            0 => {
                w.invoke_connect("widgets-db".into());
                step2.set(1);
            }
            // The workspace is up: start a statement long enough to stop.
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tabs().row_count() == 1
                && w.get_workspace_tables().row_count() == 1 =>
            {
                let slow = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT count(*) FROM c";
                w.invoke_workspace_editor_edited(slow.into());
                w.invoke_workspace_run_query();
                step2.set(2);
            }
            // It is in flight: Stop.
            2 if w.get_workspace_running() => {
                q.invoke_stop();
                step2.set(3);
            }
            // Stopped: a neutral state, not an error, and the editor is
            // editable again.
            3 if !w.get_workspace_running() => {
                if !q.get_cancelled() {
                    return bail("a stopped run should show the cancelled state".into());
                }
                if !q.get_cancelled_summary().starts_with("Cancelled after ") {
                    return bail(format!(
                        "cancel summary was {:?}",
                        q.get_cancelled_summary()
                    ));
                }
                if w.get_workspace_has_result() {
                    return bail("a stopped run should not keep a result".into());
                }
                if w.get_workspace_rows_error().as_str() != dbhive_ui_model::format::CANCELLED {
                    return bail(format!(
                        "the tab's error should be the cancel sentinel, was {:?}",
                        w.get_workspace_rows_error()
                    ));
                }
                // Run something that finishes, for the selection.
                w.invoke_workspace_editor_edited(
                    "SELECT id, name FROM widgets ORDER BY id".into(),
                );
                w.invoke_workspace_run_query();
                step2.set(4);
            }
            // Two rows: select one, then copy and clear through the global.
            4 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                if q.get_cancelled() {
                    return bail("a new run must clear the cancelled state".into());
                }
                q.invoke_cell_clicked(1, 1);
                if q.get_selected_row() != 1 || q.get_selected_col() != 1 {
                    return bail(format!(
                        "selection was ({}, {})",
                        q.get_selected_row(),
                        q.get_selected_col()
                    ));
                }
                if q.get_selection_label().as_str() != "Row 2 · name" {
                    return bail(format!("selection label was {:?}", q.get_selection_label()));
                }
                // Copying a row and a column must not move the selection.
                q.invoke_copy_row();
                q.invoke_copy_column();
                if q.get_selected_row() != 1 {
                    return bail("copying moved the selection".into());
                }
                q.invoke_clear_selection();
                if q.get_selected_row() != -1 {
                    return bail("clearing the selection left it in place".into());
                }
                // Clear empties the editor.
                q.invoke_clear_editor();
                if !w.get_workspace_editor_text().is_empty() {
                    return bail("Clear left SQL in the editor".into());
                }
                step2.set(5);
            }
            // The tab menu: Close Others on a second, empty tab.
            5 => {
                w.invoke_workspace_editor_edited("SELECT 1".into());
                w.invoke_workspace_new_query();
                step2.set(6);
            }
            6 if w.get_workspace_tabs().row_count() == 2 => {
                q.invoke_tab_context(0, 200.0, 60.0);
                if !q.get_menu_visible() {
                    return bail("the tab menu did not open".into());
                }
                if q.get_menu_items().row_count() != 4 {
                    return bail(format!(
                        "the menu had {} entries",
                        q.get_menu_items().row_count()
                    ));
                }
                // Close Others on the first tab closes the second (empty) one.
                q.invoke_menu_chosen(1);
                step2.set(7);
            }
            // One tab left, holding SQL: Close asks before losing it.
            7 if w.get_workspace_tabs().row_count() == 1 => {
                q.invoke_tab_context(0, 200.0, 60.0);
                q.invoke_menu_chosen(0);
                step2.set(8);
            }
            8 if w.get_show_confirm() => {
                w.invoke_confirm_accepted();
                step2.set(9);
            }
            9 if w.get_workspace_tabs().row_count() == 0 => {
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });

    w.run().unwrap();
    timer.stop();
    if let Some(msg) = failure.borrow().as_ref() {
        panic!("{msg}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
