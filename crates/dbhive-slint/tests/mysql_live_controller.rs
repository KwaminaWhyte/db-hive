//! The table inspector and row editing against a real MySQL.
//!
//! Ignored by default: it needs a MySQL on localhost:3306 that accepts `root`
//! without a password, and a `mysql` client on the PATH.
//!
//!     cargo test -p dbhive-slint --test mysql_live_controller -- --ignored
//!
//! It never touches an existing database: it creates one with a unique name,
//! works only inside it, and drops it at the end (also when an assertion
//! fails, because the drop runs from a guard). Nothing else on the server is
//! read or written. Every store is redirected to a temp directory, so it never
//! touches the developer's saved connections.
//!
//! What it proves that the SQLite scenarios cannot: the inspector reads a MySQL
//! table (auto_increment, decimal, tinyint(1), datetime, json, date) and its
//! structure; typed edits are coerced and quoted with backticks so the server
//! accepts them; an added row leaves the auto key and defaults to the server; a
//! delete and an update land by primary key; filter and sort run as MySQL SQL;
//! and a statement the server rejects rolls the whole commit back.

use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, InspectorBridge, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

/// Run SQL through the mysql client, returning tab-separated rows. Panics on error.
fn mysql(db: Option<&str>, sql: &str) -> String {
    let mut cmd = Command::new("mysql");
    cmd.args(["-uroot", "-h127.0.0.1", "-N", "-B"]);
    if let Some(db) = db {
        cmd.args(["-D", db]);
    }
    let out = cmd.args(["-e", sql]).output().expect("mysql must be on the PATH");
    assert!(
        out.status.success(),
        "mysql failed for {sql:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Drops the scratch database when the test ends, however it ends.
struct Scratch(String);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = Command::new("mysql")
            .args(["-uroot", "-h127.0.0.1", "-e", &format!("DROP DATABASE IF EXISTS `{}`", self.0)])
            .output();
    }
}

fn cell(w: &AppWindow, row: usize, col: usize) -> String {
    w.get_workspace_rows()
        .row_data(row)
        .and_then(|r| r.cells.row_data(col))
        .map(|c| c.text.to_string())
        .unwrap_or_else(|| "<missing>".into())
}

fn rows(w: &AppWindow) -> usize {
    w.get_workspace_rows().row_count()
}

#[test]
#[ignore = "needs a local MySQL on localhost:3306 (root, no password) and the mysql client"]
fn inspect_edit_add_delete_and_a_rejected_commit_on_mysql() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-mylive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let name = format!("dbhive_ui_{}", std::process::id());
    mysql(None, &format!("DROP DATABASE IF EXISTS `{name}`"));
    mysql(None, &format!("CREATE DATABASE `{name}`"));
    let _guard = Scratch(name.clone());
    mysql(
        Some(&name),
        "CREATE TABLE gadgets (
             id INT AUTO_INCREMENT PRIMARY KEY,
             name VARCHAR(100) NOT NULL,
             qty INT,
             price DECIMAL(10,2),
             active TINYINT(1) DEFAULT 1,
             made_at DATETIME,
             meta JSON,
             d DATE
         );
         INSERT INTO gadgets (name, qty, price, made_at, meta, d) VALUES
             ('Sprocket', 5, 9.50, '2026-01-01 00:00:00', '{\"k\": 1}', '2026-01-01'),
             ('Gear', 7, 12.25, NULL, NULL, NULL),
             ('O\\'Brien cog', NULL, NULL, NULL, '[1,2]', NULL);",
    );

    {
        let mut p = ConnectionProfile::new(
            "my".to_string(),
            "Local MySQL".to_string(),
            DbDriver::MySql,
            "127.0.0.1".to_string(),
            3306,
            "root".to_string(),
        );
        p.database = Some(name.clone());
        let mut state = AppState::new();
        state.add_profile(p);
        state.save_profiles_to_store().unwrap();
    }

    let w = dbhive_slint::app::build().unwrap();

    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, db) = (w.as_weak(), failure.clone(), step.clone(), name.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        let w = weak.upgrade().unwrap();
        let b = w.global::<InspectorBridge>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(60) {
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
        let edit = |r: i32, c: i32, text: &str| {
            b.invoke_cell_double_clicked(r, c);
            b.invoke_edit_accepted(text.into());
        };

        match step2.get() {
            0 => {
                w.invoke_connect("my".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 1 => {
                w.invoke_workspace_select_table("gadgets".into());
                step2.set(2);
            }

            // -- the table reads: rows, total, a quote in a value ---------
            2 if ready(3) && b.get_total_label() == "3 rows" => {
                expect!(cell(&w, 0, 1) == "Sprocket", "row 0 name was {:?}", cell(&w, 0, 1));
                expect!(cell(&w, 2, 1) == "O'Brien cog", "a quote in a value: {:?}", cell(&w, 2, 1));
                expect!(b.get_editable(), "a MySQL table should be editable: {:?}", b.get_edit_note());
                edit(0, 1, "Widget Z");
                edit(0, 2, "42");
                edit(0, 3, "19.99");
                edit(0, 4, "0");
                step2.set(3);
            }
            3 => {
                edit(0, 5, "2026-01-02 03:04:05");
                edit(0, 6, "{\"a\": [1, 2]}");
                edit(0, 7, "2026-02-03");
                step2.set(4);
            }
            4 => {
                expect!(
                    b.get_pending_count() == 1,
                    "one row edited is one pending change, got {} ({:?})",
                    b.get_pending_count(),
                    b.get_edit_message()
                );
                expect!(b.get_edit_message().is_empty(), "an edit was refused: {:?}", b.get_edit_message());
                b.invoke_open_review();
                step2.set(5);
            }
            5 if b.get_review_open() => {
                b.invoke_commit_edits();
                step2.set(6);
            }

            // -- the server took every value ------------------------------
            6 if b.get_pending_count() == 0 && !b.get_committing() && !b.get_review_open() => {
                let got = mysql(
                    Some(&db),
                    "SELECT name, qty, price, active, made_at, meta, d FROM gadgets WHERE id = 1",
                );
                expect!(
                    got == "Widget Z\t42\t19.99\t0\t2026-01-02 03:04:05\t{\"a\": [1, 2]}\t2026-02-03",
                    "after the commit the row was {got:?}"
                );
                let rest = mysql(Some(&db), "SELECT COUNT(*) FROM gadgets");
                expect!(rest == "3", "row count {rest}");
                step2.set(7);
            }

            // -- a boolean word typed into a tinyint(1) column -------------
            7 if ready(3) => {
                edit(1, 4, "true");
                step2.set(8);
            }
            8 => {
                expect!(
                    b.get_edit_message().is_empty() && b.get_pending_count() == 1,
                    "typing 'true' into a tinyint(1) column should be accepted, got {:?}",
                    b.get_edit_message()
                );
                b.invoke_discard_edits();
                step2.set(9);
            }

            // -- add a row: the auto key and defaults are the server's -----
            9 if b.get_pending_count() == 0 && ready(3) => {
                b.invoke_add_row();
                step2.set(10);
            }
            10 => {
                edit(3, 1, "Flange");
                step2.set(11);
            }
            11 => {
                b.invoke_open_review();
                step2.set(12);
            }
            12 if b.get_review_open() => {
                let sql: String = b.get_review_items().iter().map(|i| i.sql.to_string()).collect::<Vec<_>>().join("\n");
                expect!(
                    sql.contains('`') && !sql.contains("`id`") && sql.contains("INSERT") && sql.contains("Flange"),
                    "the insert should use backticks and name only what was filled in:\n{sql}"
                );
                b.invoke_commit_edits();
                step2.set(13);
            }
            13 if b.get_pending_count() == 0 && !b.get_committing() && !b.get_review_open() => {
                let got = mysql(Some(&db), "SELECT id, name, active FROM gadgets WHERE name = 'Flange'");
                expect!(got == "4\tFlange\t1", "the new row was {got:?}");
                step2.set(14);
            }

            // -- delete the row with a quote in it -------------------------
            14 if ready(4) => {
                b.invoke_row_toggled(2);
                b.invoke_delete_checked();
                step2.set(15);
            }
            15 => {
                b.invoke_open_review();
                step2.set(16);
            }
            16 if b.get_review_open() => {
                b.invoke_commit_edits();
                step2.set(17);
            }
            17 if b.get_pending_count() == 0 && !b.get_committing() && !b.get_review_open() => {
                let got = mysql(Some(&db), "SELECT COUNT(*) FROM gadgets WHERE name LIKE 'O%Brien%'");
                expect!(got == "0", "the deleted row is still there: {got}");
                step2.set(18);
            }

            // -- a value the server rejects rolls everything back ----------
            18 if ready(3) => {
                edit(0, 1, "Should not stick");
                step2.set(19);
            }
            19 => {
                edit(1, 2, "99999999999");
                step2.set(20);
            }
            20 => {
                b.invoke_open_review();
                step2.set(21);
            }
            21 if b.get_review_open() => {
                b.invoke_commit_edits();
                step2.set(22);
            }
            22 if !b.get_committing() && !b.get_edit_message().is_empty() => {
                let msg = b.get_edit_message().to_string();
                expect!(msg.contains("Nothing was changed"), "message was {msg:?}");
                let got = mysql(Some(&db), "SELECT name FROM gadgets WHERE id = 1");
                expect!(got == "Widget Z", "the failed commit changed the first row: {got:?}");
                b.invoke_discard_edits();
                step2.set(30);
            }

            // -- filter and sort run as MySQL SQL --------------------------
            30 if b.get_pending_count() == 0 && ready(3) => {
                b.invoke_toggle_filters();
                b.invoke_add_filter();
                b.invoke_filter_column_picked(0, 1); // name
                b.invoke_filter_value_edited(0, "ea".into());
                b.invoke_apply_filters();
                step2.set(31);
            }
            31 if ready(1) => {
                expect!(cell(&w, 0, 1) == "Gear", "filter 'contains ea' kept {:?}", cell(&w, 0, 1));
                b.invoke_filter_value_edited(0, "it's \\ here".into());
                b.invoke_apply_filters();
                step2.set(32);
            }
            32 if !w.get_workspace_running() && w.get_workspace_has_result() && rows(&w) == 0 => {
                expect!(
                    w.get_workspace_rows_error().is_empty(),
                    "a quote and a backslash in a filter value must not be an error: {:?}",
                    w.get_workspace_rows_error()
                );
                b.invoke_clear_filters();
                step2.set(33);
            }
            33 if ready(3) => {
                b.invoke_sort_by(1);
                step2.set(34);
            }
            34 if ready(3) && cell(&w, 0, 1) == "Flange" => {
                b.invoke_sort_by(1);
                step2.set(35);
            }
            35 if ready(3) && cell(&w, 0, 1) == "Widget Z" => {
                step2.set(40);
            }

            // -- structure reads from the catalogue ------------------------
            40 if b.get_pending_count() == 0 => {
                b.invoke_set_view(1);
                step2.set(41);
            }
            41 if !b.get_meta_loading() && b.get_struct_columns().row_count() == 8 => {
                let cols: Vec<_> = b.get_struct_columns().iter().collect();
                expect!(cols[0].primary && cols[0].auto, "id should be a primary, auto column");
                expect!(cols[1].name == "name" && !cols[1].nullable, "name is NOT NULL");
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
