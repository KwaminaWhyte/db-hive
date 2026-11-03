//! Workspace tabs surviving a restart, end to end: the real window, the real
//! controller, a real SQLite file and the real `workspace_tabs.json`.
//!
//! Covers what the unit tests in `dbhive_ui_model::saved_tabs` cannot: that
//! typing in a query tab and opening tables are autosaved by the timer, that
//! the same connection reopens with the same tabs in the same order (SQL back
//! in the editor, the table tab re-queried, the same tab active), that a
//! table dropped meanwhile is not brought back, that results are not stored,
//! and that a workspace left with only blank editors is not kept.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn tab_titles(w: &AppWindow) -> Vec<String> {
    w.get_workspace_tabs()
        .iter()
        .map(|t| t.title.to_string())
        .collect()
}

fn saved(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("workspace_tabs.json")).unwrap_or_default()
}

#[test]
fn tabs_and_sql_come_back_when_the_connection_is_reopened() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-tabs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("notes.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO items (name) VALUES ('a'), ('b'), ('c');
             CREATE TABLE temp_notes (id INTEGER PRIMARY KEY, body TEXT);",
        )
        .unwrap();
    }
    {
        let mut profile = ConnectionProfile::new(
            "notes-db".to_string(),
            "Notes".to_string(),
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

    let (weak, fail, step2, dir2, db2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        dir.clone(),
        db.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(40) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        macro_rules! expect {
            ($cond:expr, $($msg:tt)+) => {
                if !($cond) {
                    return bail(format!($($msg)+));
                }
            };
        }

        match step2.get() {
            // -- first visit: type, open two tables, add another query ----
            0 => {
                w.invoke_connect("notes-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tables().row_count() == 2 =>
            {
                expect!(
                    saved(&dir2).is_empty(),
                    "nothing should be saved before anything is typed"
                );
                w.invoke_workspace_editor_edited("SELECT 'first tab' AS marker".into());
                step2.set(2);
            }
            2 => {
                w.invoke_workspace_select_table("items".into());
                step2.set(3);
            }
            3 if w.get_workspace_tabs().row_count() == 2 => {
                w.invoke_workspace_select_table("temp_notes".into());
                step2.set(4);
            }
            4 if w.get_workspace_tabs().row_count() == 3 => {
                w.invoke_workspace_new_query();
                step2.set(5);
            }
            5 if w.get_workspace_tabs().row_count() == 4 => {
                w.invoke_workspace_editor_edited("SELECT 'last tab' AS marker".into());
                // Back to the items table tab, so the active tab is neither first nor last.
                w.invoke_workspace_tab_selected(1);
                step2.set(6);
            }

            // -- the autosave timer writes it, without leaving ------------
            6 if saved(&dir2).contains("last tab") => {
                let s = saved(&dir2);
                expect!(
                    s.contains("first tab") && s.contains("\"items\"") && s.contains("temp_notes"),
                    "saved: {s}"
                );
                expect!(
                    !s.contains("\"rows\"") && !s.contains("\"b\""),
                    "results must not be stored: {s}"
                );
                // While away, one saved table disappears.
                w.invoke_workspace_back();
                step2.set(7);
            }
            7 if w.get_screen() == Screen::Connections => {
                rusqlite::Connection::open(&db2)
                    .unwrap()
                    .execute_batch("DROP TABLE temp_notes;")
                    .unwrap();
                w.invoke_connect("notes-db".into());
                step2.set(8);
            }

            // -- second visit: the tabs are back, in order ----------------
            8 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tabs().row_count() == 3
                && !w.get_workspace_running()
                && w.get_workspace_has_result() =>
            {
                let titles = tab_titles(&w);
                expect!(
                    titles[1] == "items"
                        && titles[0].starts_with("Query")
                        && titles[2].starts_with("Query"),
                    "tab order was {titles:?} (temp_notes was dropped and must not return)"
                );
                expect!(
                    w.get_workspace_active_tab() == 1,
                    "the items tab was active, got {}",
                    w.get_workspace_active_tab()
                );
                expect!(
                    w.get_workspace_rows().row_count() == 3,
                    "the table tab should have been re-queried, got {} rows",
                    w.get_workspace_rows().row_count()
                );
                w.invoke_workspace_tab_selected(0);
                step2.set(9);
            }
            9 => {
                expect!(
                    w.get_workspace_editor_text() == "SELECT 'first tab' AS marker",
                    "first tab text was {:?}",
                    w.get_workspace_editor_text()
                );
                w.invoke_workspace_tab_selected(2);
                step2.set(10);
            }
            10 => {
                expect!(
                    w.get_workspace_editor_text() == "SELECT 'last tab' AS marker",
                    "last tab text was {:?}",
                    w.get_workspace_editor_text()
                );
                // The restore did not run the query for us.
                expect!(
                    !w.get_workspace_has_result(),
                    "a restored query must not have run"
                );

                // Blank both editors and close the table: nothing is worth keeping.
                w.invoke_workspace_editor_edited("".into());
                step2.set(11);
            }
            11 => {
                w.invoke_workspace_tab_selected(0);
                step2.set(12);
            }
            12 => {
                w.invoke_workspace_editor_edited("".into());
                step2.set(13);
            }
            13 => {
                // The table tab has no pending edits, so it closes unasked.
                w.invoke_workspace_tab_closed(1);
                step2.set(14);
            }
            14 if w.get_workspace_tabs().row_count() == 2 && !saved(&dir2).contains("notes-db") => {
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
