//! SQL autocomplete driven end to end: the real window, the real controller, a
//! real SQLite file.
//!
//! Covers what the pure tests in `dbhive_ui_model::sql_suggest` cannot: that
//! the workspace's table list becomes completion metadata, that a table's
//! columns are fetched in the background the first time a statement needs
//! them, that choosing a row rewrites the editor and moves the caret, and that
//! a dismissed or just-finished word stays quiet.
//!
//! The editor reports its caret with `cursor-moved`; the test plays that part
//! (and the text edit that comes with it) by calling the same callbacks the
//! editor calls. Key handling for the popup is a few lines of Slint around the
//! editor and is covered by hand and by snapshot, not here.
//!
//! Every store is redirected to a temp directory.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn labels(w: &AppWindow) -> Vec<String> {
    w.get_workspace_completions()
        .iter()
        .map(|c| c.label.to_string())
        .collect()
}

/// What the editor does when the user types `text` and the caret lands after
/// the last character.
fn type_text(w: &AppWindow, text: &str) {
    type_text_with_caret(w, text, text.len());
}

/// As `type_text`, with the caret at byte offset `caret`.
fn type_text_with_caret(w: &AppWindow, text: &str, caret: usize) {
    w.set_workspace_editor_text(text.into());
    w.invoke_workspace_editor_edited(text.into());
    w.invoke_workspace_cursor_moved(caret as i32);
}

#[test]
fn tables_columns_accept_dismiss_and_strings() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir =
        std::env::temp_dir().join(format!("dbhive-slint-autocomplete-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, plan TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, total REAL);",
        )
        .unwrap();
    }
    {
        let mut profile = ConnectionProfile::new(
            "shop".to_string(),
            "Shop".to_string(),
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
        if started.elapsed() > Duration::from_secs(20) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            0 => {
                w.invoke_connect("shop".into());
                step2.set(1);
            }
            // In the workspace with its tables listed: ask for a table.
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tables().row_count() == 2
                && w.get_workspace_tabs().row_count() == 1 =>
            {
                type_text(&w, "SELECT * FROM cu");
                step2.set(2);
            }
            2 => {
                let l = labels(&w);
                if l.first().map(String::as_str) != Some("customers") {
                    return bail(format!("after FROM, expected customers first, got {l:?}"));
                }
                let first = w.get_workspace_completions().row_data(0).unwrap();
                if first.kind != "table" {
                    return bail(format!("a table row should say so, got {:?}", first.kind));
                }
                // Accepting it rewrites the editor and asks for the caret to move.
                w.invoke_workspace_completion_accept();
                step2.set(3);
            }
            3 => {
                if w.get_workspace_editor_text() != "SELECT * FROM customers" {
                    return bail(format!(
                        "accepted text was {:?}",
                        w.get_workspace_editor_text()
                    ));
                }
                if !labels(&w).is_empty() {
                    return bail("the popup should close once a choice is applied".into());
                }
                // The caret lands after the word: it must stay quiet.
                w.invoke_workspace_cursor_moved("SELECT * FROM customers".len() as i32);
                if !labels(&w).is_empty() {
                    return bail("a just-completed word must not pop up again".into());
                }
                // Columns: not loaded yet for `orders`, fetched in the background.
                type_text_with_caret(&w, "SELECT cu FROM orders", "SELECT cu".len());
                step2.set(4);
            }
            4 if !labels(&w).is_empty() => {
                let l = labels(&w);
                if l.first().map(String::as_str) != Some("customer_id") {
                    return bail(format!("expected customer_id after the fetch, got {l:?}"));
                }
                let first = w.get_workspace_completions().row_data(0).unwrap();
                if first.kind != "column" || first.detail.is_empty() {
                    return bail(format!(
                        "a column row carries its type, got {:?}/{:?}",
                        first.kind, first.detail
                    ));
                }
                // Dismiss: gone, and quiet while the caret stays on the word.
                w.invoke_workspace_completion_dismiss();
                w.invoke_workspace_cursor_moved("SELECT cu".len() as i32);
                if !labels(&w).is_empty() {
                    return bail("a dismissed word must stay quiet".into());
                }
                // A different word lifts the quiet. `o.` lists orders' columns.
                type_text_with_caret(&w, "SELECT o. FROM orders o", "SELECT o.".len());
                step2.set(5);
            }
            5 if !labels(&w).is_empty() => {
                let l = labels(&w);
                if l != vec![
                    "id".to_string(),
                    "total".to_string(),
                    "customer_id".to_string(),
                ] {
                    return bail(format!("alias columns were {l:?}"));
                }
                // Nothing inside a string literal.
                type_text(&w, "SELECT 'cu");
                step2.set(6);
            }
            6 => {
                if !labels(&w).is_empty() {
                    return bail(format!(
                        "no completion inside a string, got {:?}",
                        labels(&w)
                    ));
                }
                // Open a popup again, then run: running closes it.
                type_text(&w, "SELECT * FROM cu");
                if labels(&w).is_empty() {
                    return bail("a table prefix after FROM should offer a table".into());
                }
                w.invoke_workspace_run_query();
                step2.set(7);
            }
            7 => {
                if !labels(&w).is_empty() {
                    return bail("running a query should close the popup".into());
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
