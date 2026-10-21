//! The create-table wizard and the edit-table dialog driven end to end: the
//! real window, the real controller, a real SQLite file.
//!
//! Covers what the unit tests in `dbhive_ui_model::table_dialog` cannot: that
//! the New Table command opens the wizard, that a foreign key target's columns
//! are read from the live driver, that the preview shows the SQL the core's
//! generator wrote, that Create runs it and the table really exists (with its
//! foreign key and unique constraint) and shows up in the sidebar, that the
//! edit dialog reads the live table, refuses to drop a primary key column,
//! counts pending operations, soft-deletes and restores, asks before dropping
//! a column, applies the ALTER statements, refreshes the open table tab, and
//! reports a failing statement and the ones skipped after it.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{InspectorBridge, Screen, TableDialogState};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn strings(m: slint::ModelRc<slint::SharedString>) -> Vec<String> {
    m.iter().map(|s| s.to_string()).collect()
}

/// Column names of a table, read straight from the file.
fn file_columns(db: &std::path::Path, table: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn create_a_table_then_alter_it_and_see_a_failure_reported() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-tabledialog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, total INTEGER);",
        )
        .unwrap();
    }

    {
        let mut profile = ConnectionProfile::new(
            "shop-db".to_string(),
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

    let (weak, fail, step2, db2) = (w.as_weak(), failure.clone(), step.clone(), db.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let d = w.global::<TableDialogState>();
        let ins = w.global::<InspectorBridge>();
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
        let act = |action: &str, i: i32, j: i32, text: &str| {
            d.invoke_act(action.into(), i, j, text.into());
        };
        let edit_row = |i: usize| d.get_edit_rows().row_data(i).unwrap();

        match step2.get() {
            // -- connect, open the wizard from its command ------------------
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 2 => {
                expect!(!d.get_visible(), "the dialog must start closed");
                w.invoke_run_command("new-table".into());
                step2.set(2);
            }

            // -- step 1: a name is required before Next ------------------------
            2 if d.get_visible() => {
                expect!(d.get_mode() == 0 && d.get_title() == "New table", "should be the create wizard");
                expect!(d.get_step() == 0 && d.get_steps().row_count() == 4, "four steps starting at the first");
                expect!(!d.get_can_next(), "Next must wait for a table name");
                expect!(
                    strings(d.get_step_errors()) == ["Table name is required"],
                    "step errors were {:?}",
                    strings(d.get_step_errors())
                );
                act("next", 0, 0, "");
                step2.set(3);
            }
            3 => {
                expect!(d.get_step() == 0, "Next must not advance past a missing name");
                act("name", 0, 0, "pets");
                act("ine", 0, 0, "");
                step2.set(4);
            }
            4 if d.get_can_next() => {
                act("next", 0, 0, "");
                step2.set(5);
            }

            // -- step 2: columns --------------------------------------------------
            5 if d.get_step() == 1 => {
                expect!(d.get_columns().row_count() == 1, "the wizard starts with an id column");
                let id = d.get_columns().row_data(0).unwrap();
                expect!(id.primary_key && id.auto_increment && !id.nullable, "id should be an auto key");
                act("col-add", 0, 0, "");
                act("col-name", 1, 0, "owner_id", );
                act("col-add", 0, 0, "");
                act("col-name", 2, 0, "label");
                act("col-type", 2, 2, ""); // VARCHAR
                act("col-param", 2, 0, "30");
                act("col-add", 0, 0, "");
                act("col-name", 3, 0, "note");
                act("col-type", 3, 3, ""); // TEXT
                // Auto-increment is only for integers: refused on text.
                act("col-auto", 3, 0, "");
                act("next", 0, 0, "");
                step2.set(6);
            }

            // -- step 3: a unique constraint and a foreign key ------------------
            6 if d.get_step() == 2 => {
                expect!(
                    !d.get_columns().row_data(3).unwrap().auto_increment,
                    "a text column must not become auto-increment"
                );
                act("uq-add", 0, 0, "");
                act("uq-col", 0, 2, ""); // label
                act("fk-add", 0, 0, "");
                act("fk-col", 0, 1, ""); // owner_id
                // The foreign key must be finished before Next; then pick a target.
                expect!(!d.get_can_next(), "an unfinished foreign key must block Next");
                act("fk-table", 0, 0, ""); // customers
                step2.set(7);
            }
            7 if d.get_foreign_keys().row_count() == 1
                && d.get_foreign_keys().row_data(0).unwrap().ref_chips.row_count() == 3 =>
            {
                // The target's columns were read from the live table.
                let fk = d.get_foreign_keys().row_data(0).unwrap();
                expect!(fk.ref_table == "customers", "target was {:?}", fk.ref_table);
                let cols: Vec<String> = fk.ref_chips.iter().map(|c| c.label.to_string()).collect();
                expect!(cols == ["id", "name", "email"], "referenced columns were {cols:?}");
                act("fk-rcol", 0, 0, ""); // id
                act("fk-del", 0, 2, ""); // CASCADE
                step2.set(8);
            }
            8 if d.get_can_next() => {
                act("next", 0, 0, "");
                step2.set(9);
            }

            // -- step 4: the SQL from the core's generator, then Create ---------
            9 if d.get_step() == 3 => {
                let sql = d.get_sql().to_string();
                expect!(d.get_sql_error().is_empty(), "preview error: {}", d.get_sql_error());
                expect!(sql.contains("CREATE TABLE IF NOT EXISTS \"pets\""), "create missing:\n{sql}");
                // SQLite has no sized text type, so its generator writes VARCHAR as TEXT.
                expect!(sql.contains("\"label\" TEXT"), "label column missing:\n{sql}");
                expect!(
                    sql.contains("FOREIGN KEY (\"owner_id\") REFERENCES \"customers\" (\"id\") ON DELETE CASCADE"),
                    "foreign key missing:\n{sql}"
                );
                expect!(sql.contains("UNIQUE (\"label\")"), "unique missing:\n{sql}");
                expect!(d.get_statement_count() == 1, "one statement expected");
                expect!(
                    file_columns(&db2, "pets").is_empty(),
                    "nothing may run before Create is pressed"
                );
                d.invoke_apply();
                step2.set(10);
            }
            10 if d.get_results().row_count() == 1 && !d.get_running() => {
                expect!(d.get_result_ok(), "create failed: {}", d.get_result_summary());
                expect!(
                    file_columns(&db2, "pets") == ["id", "owner_id", "label", "note"],
                    "the table should exist in the file, columns were {:?}",
                    file_columns(&db2, "pets")
                );
                let conn = rusqlite::Connection::open(&db2).unwrap();
                let target: String = conn
                    .query_row("SELECT \"table\" FROM pragma_foreign_key_list('pets')", [], |r| r.get(0))
                    .unwrap();
                expect!(target == "customers", "foreign key should point at customers, got {target}");
                step2.set(11);
            }
            // The sidebar picked the new table up.
            11 if w.get_workspace_tables().row_count() == 3 => {
                d.invoke_close();
                expect!(!d.get_visible(), "Close should hide the dialog");
                w.invoke_workspace_select_table("pets".into());
                step2.set(12);
            }

            // -- edit: open from the table tab ---------------------------------------
            12 if w.get_workspace_tabs().row_count() == 2
                && !w.get_workspace_active_is_query()
                && ins.get_struct_columns().row_count() == 4 =>
            {
                d.invoke_open_edit_active();
                step2.set(13);
            }
            13 if d.get_visible() && !d.get_loading() && d.get_edit_rows().row_count() == 4 => {
                expect!(d.get_mode() == 1 && d.get_title() == "Edit table", "should be the edit dialog");
                expect!(d.get_pending() == 0, "an untouched table has nothing pending");
                let names: Vec<String> = (0..4).map(|i| edit_row(i).name.to_string()).collect();
                expect!(names == ["id", "owner_id", "label", "note"], "rows were {names:?}");
                expect!(edit_row(0).primary_key && !edit_row(0).can_remove, "the key column must not be droppable");

                // The primary key cannot be dropped, however it is asked.
                act("col-remove", 0, 0, "");
                step2.set(14);
            }
            14 => {
                expect!(!edit_row(0).removed && d.get_pending() == 0, "dropping the primary key must be refused");
                act("col-add", 0, 0, "");
                act("col-name", 4, 0, "nickname");
                act("col-type", 4, 3, ""); // TEXT
                act("col-remove", 3, 0, ""); // note, soft-deleted
                step2.set(15);
            }
            15 => {
                expect!(edit_row(3).removed && edit_row(3).badges.row_count() == 1, "note should be marked to drop");
                expect!(d.get_pending() == 2, "add + drop = 2 pending, got {}", d.get_pending());
                act("col-restore", 3, 0, "");
                step2.set(16);
            }
            16 => {
                expect!(!edit_row(3).removed && d.get_pending() == 1, "restore should undo the drop");
                act("col-remove", 3, 0, "");
                act("col-name", 2, 0, "title"); // rename label
                step2.set(17);
            }
            17 => {
                expect!(d.get_pending() == 3, "add + drop + rename = 3 pending, got {}", d.get_pending());
                act("next", 0, 0, "");
                step2.set(18);
            }
            18 if d.get_step() == 1 => {
                let sql = d.get_sql().to_string();
                expect!(sql.contains("DROP COLUMN \"note\""), "drop missing:\n{sql}");
                expect!(sql.contains("RENAME COLUMN \"label\" TO \"title\""), "rename missing:\n{sql}");
                expect!(sql.contains("ADD COLUMN \"nickname\" TEXT"), "add missing:\n{sql}");
                expect!(!sql.contains("CASCADE"), "a column drop must never cascade:\n{sql}");
                expect!(d.get_statement_count() == 3, "three statements expected");
                expect!(
                    file_columns(&db2, "pets") == ["id", "owner_id", "label", "note"],
                    "nothing may run before Apply is pressed"
                );
                d.invoke_apply();
                step2.set(19);
            }
            // Dropping a column deletes data, so it asks first.
            19 if w.get_show_confirm() => {
                expect!(
                    w.get_confirm_message().contains("note"),
                    "the confirmation should name the column, got {:?}",
                    w.get_confirm_message()
                );
                expect!(
                    file_columns(&db2, "pets") == ["id", "owner_id", "label", "note"],
                    "nothing may run before the confirmation is accepted"
                );
                w.invoke_confirm_accepted();
                step2.set(20);
            }
            20 if d.get_results().row_count() == 3 && !d.get_running() => {
                expect!(d.get_result_ok(), "alter failed: {}", d.get_result_summary());
                expect!(
                    file_columns(&db2, "pets") == ["id", "owner_id", "title", "nickname"],
                    "columns after the alter were {:?}",
                    file_columns(&db2, "pets")
                );
                step2.set(21);
            }
            // The open table tab was refreshed, and the editor reloaded from the live table.
            21 if ins.get_struct_columns().row_count() == 4
                && ins.get_struct_columns().row_data(2).unwrap().name == "title"
                && d.get_pending() == 0 =>
            {
                expect!(edit_row(2).name == "title", "the editor should show the live table");
                // A second edit that cannot work: SQLite will not drop a UNIQUE
                // column. The failure is reported, and what follows is skipped.
                act("back", 0, 0, "");
                step2.set(22);
            }
            22 => {
                expect!(d.get_results().row_count() == 0, "going back should clear the old report");
                act("col-remove", 2, 0, ""); // title (unique)
                act("col-add", 0, 0, "");
                act("col-name", 4, 0, "extra");
                act("next", 0, 0, "");
                step2.set(23);
            }
            23 if d.get_step() == 1 => {
                d.invoke_apply();
                step2.set(24);
            }
            24 if w.get_show_confirm() => {
                w.invoke_confirm_accepted();
                step2.set(25);
            }
            25 if d.get_results().row_count() == 2 && !d.get_running() => {
                let r0 = d.get_results().row_data(0).unwrap();
                let r1 = d.get_results().row_data(1).unwrap();
                expect!(!d.get_result_ok(), "a failing alter must not report success");
                expect!(r0.status == 1 && !r0.message.is_empty(), "the drop should be reported as failed with the database's message");
                expect!(r1.status == 2, "the statement after a failure must be skipped");
                expect!(
                    d.get_result_summary().contains("1 failed") && d.get_result_summary().contains("1 skipped"),
                    "summary was {:?}",
                    d.get_result_summary()
                );
                expect!(
                    file_columns(&db2, "pets") == ["id", "owner_id", "title", "nickname"],
                    "the failed alter must leave the table as it was, got {:?}",
                    file_columns(&db2, "pets")
                );
                d.invoke_close();
                expect!(!d.get_visible(), "Close should hide the dialog");
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
