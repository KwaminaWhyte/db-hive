//! Editing rows in the data grid, end to end: the real window, the real
//! controller, a real SQLite file.
//!
//! Covers what the unit tests in `dbhive_ui_model::row_edits` cannot: that a
//! double-click opens the editor with the stored value, that an edit is held
//! as pending (the database is untouched), that a primary key cell refuses to
//! be edited, that added and deleted rows are marked in the grid, that the
//! review lists the SQL, that Commit changes the database in one transaction,
//! that a constraint failure changes nothing and says which change failed,
//! that a table with no primary key refuses edits, and that closing a tab
//! with pending changes asks first.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, InspectorBridge, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn cell_text(w: &AppWindow, row: usize, col: usize) -> String {
    w.get_workspace_rows()
        .row_data(row)
        .and_then(|r| r.cells.row_data(col))
        .map(|c| c.text.to_string())
        .unwrap_or_else(|| "<missing>".into())
}

fn cell_modified(w: &AppWindow, row: usize, col: usize) -> bool {
    w.get_workspace_rows()
        .row_data(row)
        .and_then(|r| r.cells.row_data(col))
        .is_some_and(|c| c.modified)
}

fn row_deleted(w: &AppWindow, row: usize) -> bool {
    w.get_workspace_rows()
        .row_data(row)
        .is_some_and(|r| r.deleted)
}

fn row_added(w: &AppWindow, row: usize) -> bool {
    w.get_workspace_rows()
        .row_data(row)
        .is_some_and(|r| r.added)
}

fn rows(w: &AppWindow) -> usize {
    w.get_workspace_rows().row_count()
}

/// Run a query against the file directly, bypassing the app.
fn db_rows(db: &PathBuf, sql: &str) -> Vec<Vec<String>> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        (0..n)
            .map(|i| {
                Ok(match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => "NULL".to_string(),
                    rusqlite::types::ValueRef::Integer(i) => i.to_string(),
                    rusqlite::types::ValueRef::Real(f) => f.to_string(),
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                    rusqlite::types::ValueRef::Blob(_) => "<blob>".to_string(),
                })
            })
            .collect()
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

#[test]
fn edit_add_delete_review_and_commit_rows_in_a_real_table() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-rowedit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("pantry.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE items (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL,
                 qty INTEGER,
                 code TEXT UNIQUE
             );
             INSERT INTO items (name, qty, code) VALUES ('Apple', 5, 'A-1');
             INSERT INTO items (name, qty, code) VALUES ('Banana', 7, 'B-1');
             INSERT INTO items (name, qty, code) VALUES ('Carrot', 9, 'C-1');
             CREATE TABLE loose (a TEXT, b TEXT);
             INSERT INTO loose VALUES ('x', 'y');",
        )
        .unwrap();
    }

    {
        let mut profile = ConnectionProfile::new(
            "pantry-db".to_string(),
            "Pantry".to_string(),
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

    let (weak, fail, step2, db2) = (w.as_weak(), failure.clone(), step.clone(), db.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let b = w.global::<InspectorBridge>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(30) {
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
        let ready = |n: usize| {
            !w.get_workspace_running()
                && w.get_workspace_has_result()
                && rows(&w) == n
                && !b.get_meta_loading()
        };

        match step2.get() {
            // -- connect, open the items table --------------------------
            0 => {
                w.invoke_connect("pantry-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tables().row_count() == 2 =>
            {
                w.invoke_workspace_select_table("items".into());
                step2.set(2);
            }

            // -- double-click opens the editor with the stored value ------
            2 if ready(3) => {
                expect!(b.get_editable(), "a SQLite table should be editable");
                expect!(
                    b.get_edit_note().is_empty(),
                    "note was {:?}",
                    b.get_edit_note()
                );
                b.invoke_cell_double_clicked(0, 1);
                step2.set(3);
            }
            3 => {
                expect!(
                    b.get_editing_row() == 0 && b.get_editing_col() == 1,
                    "editor at ({}, {})",
                    b.get_editing_row(),
                    b.get_editing_col()
                );
                expect!(
                    b.get_edit_initial() == "Apple",
                    "initial was {:?}",
                    b.get_edit_initial()
                );
                b.invoke_edit_accepted("Apricot".into());
                step2.set(4);
            }

            // -- the edit is pending; the database is untouched -----------
            4 => {
                expect!(b.get_editing_row() == -1, "the editor should close");
                expect!(
                    cell_text(&w, 0, 1) == "Apricot",
                    "grid shows {:?}",
                    cell_text(&w, 0, 1)
                );
                expect!(cell_modified(&w, 0, 1), "the edited cell should be marked");
                expect!(!cell_modified(&w, 0, 2), "other cells should not be marked");
                expect!(
                    b.get_pending_count() == 1,
                    "pending {}",
                    b.get_pending_count()
                );
                let stored = db_rows(&db2, "SELECT name FROM items WHERE id = 1");
                expect!(
                    stored == vec![vec!["Apple".to_string()]],
                    "db changed early: {stored:?}"
                );

                // A value that does not fit the column type is refused with a
                // reason, and nothing is held.
                b.invoke_cell_double_clicked(1, 2);
                step2.set(5);
            }
            5 => {
                b.invoke_edit_accepted("seven".into());
                step2.set(6);
            }
            6 => {
                expect!(
                    b.get_edit_message().contains("Not saved"),
                    "message was {:?}",
                    b.get_edit_message()
                );
                expect!(
                    cell_text(&w, 1, 2) == "7",
                    "bad text must not stick: {:?}",
                    cell_text(&w, 1, 2)
                );
                expect!(
                    b.get_pending_count() == 1,
                    "pending {}",
                    b.get_pending_count()
                );

                // The primary key cannot be edited.
                b.invoke_cell_double_clicked(0, 0);
                step2.set(7);
            }
            7 => {
                expect!(
                    b.get_editing_row() == -1,
                    "a primary key cell must not open an editor"
                );
                expect!(
                    b.get_edit_message().contains("Primary key"),
                    "message was {:?}",
                    b.get_edit_message()
                );

                // Add a row, fill two cells.
                b.invoke_add_row();
                step2.set(8);
            }
            8 => {
                expect!(
                    rows(&w) == 4,
                    "an added row should show, got {} rows",
                    rows(&w)
                );
                expect!(row_added(&w, 3), "row 3 should be marked as added");
                expect!(
                    cell_text(&w, 3, 0) == "auto",
                    "id placeholder was {:?}",
                    cell_text(&w, 3, 0)
                );
                b.invoke_cell_double_clicked(3, 1);
                step2.set(9);
            }
            9 => {
                b.invoke_edit_accepted("Dill".into());
                step2.set(10);
            }
            10 => {
                b.invoke_cell_double_clicked(3, 3);
                step2.set(11);
            }
            11 => {
                b.invoke_edit_accepted("D-1".into());
                step2.set(12);
            }

            // -- delete the second row ------------------------------------
            12 => {
                expect!(
                    cell_text(&w, 3, 1) == "Dill",
                    "new row name {:?}",
                    cell_text(&w, 3, 1)
                );
                b.invoke_cell_clicked(1, 1);
                step2.set(13);
            }
            13 if b.get_selected_row() == 1 => {
                b.invoke_delete_selected();
                step2.set(14);
            }
            14 => {
                expect!(row_deleted(&w, 1), "row 1 should be marked deleted");
                expect!(
                    b.get_selected_deleted(),
                    "the selection should know it is deleted"
                );
                expect!(
                    b.get_pending_count() == 3,
                    "pending {}",
                    b.get_pending_count()
                );
                b.invoke_open_review();
                step2.set(15);
            }

            // -- the review lists the SQL; still nothing written ----------
            15 if b.get_review_open() => {
                let items: Vec<_> = b.get_review_items().iter().collect();
                expect!(items.len() == 3, "review has {} items", items.len());
                let sql: String = items
                    .iter()
                    .map(|i| i.sql.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                expect!(
                    sql.contains("UPDATE") && sql.contains("Apricot"),
                    "no update in:\n{sql}"
                );
                expect!(
                    sql.contains("DELETE") && sql.contains("id"),
                    "no delete in:\n{sql}"
                );
                expect!(
                    sql.contains("INSERT") && sql.contains("Dill"),
                    "no insert in:\n{sql}"
                );
                let stored = db_rows(&db2, "SELECT COUNT(*) FROM items");
                expect!(
                    stored == vec![vec!["3".to_string()]],
                    "db changed at review: {stored:?}"
                );
                b.invoke_commit_edits();
                step2.set(16);
            }

            // -- committed: the database has all three changes ------------
            16 if b.get_pending_count() == 0 && !b.get_committing() && !b.get_review_open() => {
                let stored = db_rows(&db2, "SELECT id, name, qty, code FROM items ORDER BY id");
                let want: Vec<Vec<String>> = [
                    ["1", "Apricot", "5", "A-1"],
                    ["3", "Carrot", "9", "C-1"],
                    ["4", "Dill", "NULL", "D-1"],
                ]
                .iter()
                .map(|r| r.iter().map(|s| s.to_string()).collect())
                .collect();
                expect!(stored == want, "db after commit: {stored:?}");
                expect!(
                    w.get_toasts()
                        .iter()
                        .any(|t| t.text.contains("Saved 3 changes")),
                    "expected a saved toast"
                );
                step2.set(17);
            }

            // -- the grid reloaded from the database ----------------------
            17 if ready(3) => {
                expect!(
                    cell_text(&w, 1, 1) == "Carrot",
                    "reloaded row 1 was {:?}",
                    cell_text(&w, 1, 1)
                );
                expect!(
                    !cell_modified(&w, 0, 1),
                    "marks should be gone after the reload"
                );

                // -- a failing change leaves everything as it was ---------
                // Change Carrot's name, and add a row whose unique code
                // collides with Apricot's.
                b.invoke_cell_double_clicked(1, 1);
                step2.set(18);
            }
            18 => {
                b.invoke_edit_accepted("Carrot Cake".into());
                step2.set(19);
            }
            19 => {
                b.invoke_add_row();
                step2.set(20);
            }
            20 => {
                b.invoke_cell_double_clicked(3, 1);
                step2.set(21);
            }
            21 => {
                b.invoke_edit_accepted("Clash".into());
                step2.set(22);
            }
            22 => {
                b.invoke_cell_double_clicked(3, 3);
                step2.set(23);
            }
            23 => {
                b.invoke_edit_accepted("A-1".into());
                step2.set(24);
            }
            24 => {
                b.invoke_open_review();
                step2.set(25);
            }
            25 if b.get_review_open() => {
                b.invoke_commit_edits();
                step2.set(26);
            }
            26 if !b.get_committing() && !b.get_edit_message().is_empty() => {
                let msg = b.get_edit_message().to_string();
                expect!(msg.contains("Nothing was changed"), "message was {msg:?}");
                expect!(
                    msg.contains("2 of 2") || msg.contains("1 of 2"),
                    "should name the change: {msg:?}"
                );
                expect!(
                    b.get_review_open(),
                    "the review should stay open to fix or discard"
                );
                expect!(
                    b.get_pending_count() == 2,
                    "pending {}",
                    b.get_pending_count()
                );
                let stored = db_rows(&db2, "SELECT id, name FROM items ORDER BY id");
                let want: Vec<Vec<String>> = [["1", "Apricot"], ["3", "Carrot"], ["4", "Dill"]]
                    .iter()
                    .map(|r| r.iter().map(|s| s.to_string()).collect())
                    .collect();
                expect!(stored == want, "a failed commit changed the db: {stored:?}");

                // Closing the tab with pending changes asks first.
                b.invoke_close_review();
                step2.set(27);
            }
            27 => {
                w.invoke_workspace_tab_closed(w.get_workspace_active_tab());
                step2.set(28);
            }
            28 if w.get_show_confirm() => {
                expect!(
                    w.get_confirm_title() == "Close this tab?",
                    "title {:?}",
                    w.get_confirm_title()
                );
                expect!(
                    w.get_confirm_message().contains("2 unsaved row changes"),
                    "message was {:?}",
                    w.get_confirm_message()
                );
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                step2.set(29);
            }
            29 => {
                expect!(
                    w.get_workspace_tabs().row_count() == 2,
                    "the tab closed without confirmation"
                );
                // Discard throws the changes away.
                b.invoke_discard_edits();
                step2.set(30);
            }
            30 => {
                expect!(
                    b.get_pending_count() == 0,
                    "pending {}",
                    b.get_pending_count()
                );
                expect!(
                    rows(&w) == 3,
                    "discard should drop the added row, got {}",
                    rows(&w)
                );
                expect!(!cell_modified(&w, 1, 1), "discard should clear marks");

                // -- tick rows: copy them, tick all, delete them ---------
                b.invoke_row_toggled(0);
                b.invoke_row_toggled(2);
                step2.set(40);
            }
            40 => {
                expect!(
                    b.get_checked_count() == 2,
                    "ticked {}",
                    b.get_checked_count()
                );
                expect!(!b.get_all_checked(), "two of three is not all");
                expect!(
                    w.get_workspace_rows()
                        .row_data(0)
                        .is_some_and(|r| r.checked)
                        && w.get_workspace_rows()
                            .row_data(2)
                            .is_some_and(|r| r.checked)
                        && !w
                            .get_workspace_rows()
                            .row_data(1)
                            .is_some_and(|r| r.checked),
                    "the grid should mark rows 0 and 2 as ticked"
                );
                b.invoke_copy_checked_json();
                step2.set(41);
            }
            41 => {
                expect!(
                    w.get_toasts()
                        .iter()
                        .any(|t| t.text.contains("Copied 2 rows as JSON")),
                    "copying ticked rows should say so"
                );
                b.invoke_toggle_all();
                step2.set(42);
            }
            42 => {
                expect!(
                    b.get_checked_count() == 3 && b.get_all_checked(),
                    "header box should tick all three"
                );
                b.invoke_toggle_all();
                step2.set(43);
            }
            43 => {
                expect!(
                    b.get_checked_count() == 0 && !b.get_all_checked(),
                    "header box again should clear"
                );
                b.invoke_row_toggled(0);
                b.invoke_row_toggled(1);
                step2.set(44);
            }
            44 => {
                b.invoke_delete_checked();
                step2.set(45);
            }
            45 => {
                expect!(
                    row_deleted(&w, 0) && row_deleted(&w, 1) && !row_deleted(&w, 2),
                    "rows 0 and 1 should be marked deleted"
                );
                expect!(
                    b.get_pending_count() == 2,
                    "pending {}",
                    b.get_pending_count()
                );
                expect!(
                    b.get_checked_count() == 0,
                    "ticks should clear after marking"
                );
                let stored = db_rows(&db2, "SELECT COUNT(*) FROM items");
                expect!(
                    stored == vec![vec!["3".to_string()]],
                    "bulk delete wrote early: {stored:?}"
                );
                b.invoke_discard_edits();
                step2.set(46);
            }

            // -- a table with no primary key refuses edits ------------
            46 => {
                expect!(
                    b.get_pending_count() == 0,
                    "pending {}",
                    b.get_pending_count()
                );
                w.invoke_workspace_select_table("loose".into());
                step2.set(31);
            }
            31 if ready(1) && w.get_workspace_tabs().row_count() == 3 => {
                b.invoke_cell_double_clicked(0, 0);
                step2.set(32);
            }
            32 => {
                expect!(
                    b.get_editing_row() == -1,
                    "no editor on a table without a key"
                );
                expect!(
                    b.get_edit_message().contains("no primary key"),
                    "message was {:?}",
                    b.get_edit_message()
                );
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
