//! The visual query builder driven end to end: the real window, the real
//! controller, a real SQLite file.
//!
//! Covers what the unit tests in `dbhive_ui_model::builder_editor` cannot:
//! that the View command opens the builder, that adding a table reads its
//! columns from the live driver, that a foreign key from the file becomes a
//! join suggestion and an actual JOIN, that a value with a quote in it is
//! escaped by the driver, that "Run" hands the statement to a query tab and
//! the rows come back, that an empty query tab is reused while a busy one is
//! not, and that Reset starts over.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, BuilderState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn cell(w: &AppWindow, row: usize, col: usize) -> String {
    w.get_workspace_rows()
        .row_data(row)
        .and_then(|r| r.cells.row_data(col))
        .map(|c| c.text.to_string())
        .unwrap_or_else(|| "<missing>".into())
}

fn strings(m: slint::ModelRc<slint::SharedString>) -> Vec<String> {
    m.iter().map(|s| s.to_string()).collect()
}

#[test]
fn build_a_query_run_it_in_a_tab_and_reset() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-builder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
             CREATE TABLE orders (
                 id INTEGER PRIMARY KEY,
                 customer_id INTEGER NOT NULL REFERENCES customers(id),
                 note TEXT,
                 total INTEGER
             );
             INSERT INTO customers (id, name) VALUES (1, 'Ann'), (2, 'Bob'), (3, 'Cy');
             INSERT INTO orders (customer_id, note, total) VALUES
                 (1, 'it''s fragile', 70),
                 (1, 'plain', 10),
                 (2, 'it''s fragile', 30),
                 (3, 'other', 5);",
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

    let (weak, fail, step2) = (w.as_weak(), failure.clone(), step.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let b = w.global::<BuilderState>();
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
            b.invoke_act(action.into(), i, j, text.into());
        };
        let sources = || b.get_sources().row_count();
        let idle = || !w.get_workspace_running();

        match step2.get() {
            // -- connect, then open the builder from its command -----------
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 2 => {
                expect!(!b.get_visible(), "the builder must start closed");
                w.invoke_run_command("go-visual-query-builder".into());
                step2.set(2);
            }
            2 if b.get_visible() => {
                expect!(b.get_sections().row_count() == 7, "seven sections expected");
                expect!(
                    strings(b.get_catalog()) == ["customers", "orders"],
                    "catalog should list the schema's tables, got {:?}",
                    strings(b.get_catalog())
                );
                expect!(b.get_sql().is_empty(), "an empty builder has no SQL");
                expect!(!b.get_can_use(), "an empty builder cannot be sent to a tab");
                b.invoke_add_table("customers".into());
                step2.set(3);
            }

            // -- adding a table reads its columns from the driver ----------
            3 if sources() == 1 => {
                let s = b.get_sources().row_data(0).unwrap();
                expect!(
                    strings(s.columns) == ["id", "name", "email"],
                    "columns should come from the live table"
                );
                b.invoke_add_table("orders".into());
                step2.set(4);
            }
            4 if sources() == 2 && b.get_join_suggestions().row_count() == 1 => {
                // The foreign key in the SQLite file, written so it resolves:
                // customers is FROM, so the join runs from customers to orders.
                expect!(
                    strings(b.get_join_suggestions()) == ["customers.id = orders.customer_id"],
                    "suggestion was {:?}",
                    strings(b.get_join_suggestions())
                );
                act("join-suggest", 0, 0, "");
                step2.set(5);
            }
            5 if b.get_joins().row_count() == 1 => {
                expect!(
                    b.get_sql().contains("INNER JOIN \"orders\" AS \"orders\" ON \"customers\".\"id\" = \"orders\".\"customer_id\""),
                    "the join should reach the statement:\n{}",
                    b.get_sql()
                );
                expect!(
                    b.get_join_suggestions().row_count() == 0,
                    "an accepted suggestion is not offered again"
                );

                // customers.name, SUM(orders.total) AS revenue, grouped and ordered.
                act("col-add", 0, 2, "");
                act("col-add", 1, 4, "");
                act("col-agg", 1, 2, "");
                act("col-alias", 1, 0, "revenue");
                act("gb-add", 0, 1, "");
                act("ob-add", 0, 1, "");
                // WHERE orders.note = 'it''s fragile' (a value with a quote in it).
                act("f-cond-add", 0, 0, "");
                act("f-alias", 1, 1, "");
                act("f-col", 1, 2, "");
                act("f-val", 1, 0, "it's fragile");
                act("limit", 0, 0, "5");
                step2.set(6);
            }
            6 => {
                let sql = b.get_sql().to_string();
                expect!(
                    sql.contains("SUM(\"orders\".\"total\") AS \"revenue\""),
                    "aggregate and alias missing:\n{sql}"
                );
                expect!(
                    sql.contains("WHERE \"orders\".\"note\" = 'it''s fragile'"),
                    "the quote in the value must be escaped by the driver:\n{sql}"
                );
                expect!(sql.contains("GROUP BY \"customers\".\"name\""), "group by missing:\n{sql}");
                expect!(sql.ends_with("LIMIT 5;"), "limit missing:\n{sql}");
                expect!(b.get_can_use(), "a complete query can be sent to a tab");
                expect!(
                    b.get_warnings().row_count() == 0,
                    "no warnings expected, got {:?}",
                    strings(b.get_warnings())
                );
                // The empty first query tab is reused rather than a second one opened.
                expect!(w.get_workspace_tabs().row_count() == 1, "one query tab to start with");
                b.invoke_run_sql();
                step2.set(7);
            }

            // -- Run: the statement goes to a tab and the rows come back ----
            7 if !b.get_visible() && idle() && w.get_workspace_has_result() => {
                expect!(
                    w.get_workspace_tabs().row_count() == 1,
                    "the empty query tab should have been reused, got {} tabs",
                    w.get_workspace_tabs().row_count()
                );
                expect!(
                    w.get_workspace_rows_error().is_empty(),
                    "the query failed: {}",
                    w.get_workspace_rows_error()
                );
                expect!(
                    w.get_workspace_rows().row_count() == 2,
                    "Ann and Bob have fragile orders, got {} rows",
                    w.get_workspace_rows().row_count()
                );
                expect!(
                    cell(&w, 0, 0) == "Ann" && cell(&w, 0, 1) == "70",
                    "first row should be Ann 70, got {} {}",
                    cell(&w, 0, 0),
                    cell(&w, 0, 1)
                );
                expect!(
                    cell(&w, 1, 0) == "Bob" && cell(&w, 1, 1) == "30",
                    "second row should be Bob 30, got {} {}",
                    cell(&w, 1, 0),
                    cell(&w, 1, 1)
                );
                expect!(
                    w.get_workspace_editor_text().contains("SUM(\"orders\".\"total\")"),
                    "the tab should hold the generated SQL"
                );
                // The builder keeps its work: reopen it.
                w.invoke_run_command("go-visual-query-builder".into());
                step2.set(8);
            }
            8 if b.get_visible() => {
                expect!(sources() == 2, "the builder should remember its tables");
                expect!(b.get_columns().row_count() == 2, "and its columns");
                expect!(b.get_limit_text() == "5", "and the limit box");

                // An invalid limit keeps the statement from being sent.
                act("limit", 0, 0, "lots");
                step2.set(9);
            }
            9 => {
                expect!(
                    b.get_limit_error().contains("whole number"),
                    "limit error was {:?}",
                    b.get_limit_error()
                );
                expect!(!b.get_can_use(), "an invalid limit blocks sending");
                expect!(!b.get_sql().contains("LIMIT"), "an invalid limit must not stay in the SQL");
                // "Open in tab" with a blocked statement does nothing.
                b.invoke_use_sql();
                step2.set(10);
            }
            10 => {
                expect!(
                    b.get_visible() && w.get_workspace_tabs().row_count() == 1,
                    "a blocked statement must not open a tab"
                );
                act("limit", 0, 0, "");
                step2.set(11);
            }

            // -- "Open in tab" with a busy tab opens a new one, unrun --------
            11 if b.get_can_use() => {
                b.invoke_use_sql();
                step2.set(12);
            }
            12 if !b.get_visible() && w.get_workspace_tabs().row_count() == 2 => {
                expect!(
                    w.get_workspace_editor_text().contains("SELECT"),
                    "the new tab should hold the SQL"
                );
                expect!(
                    !w.get_workspace_has_result(),
                    "Open in tab must not run the statement"
                );

                // -- Reset ------------------------------------------------
                w.invoke_run_command("go-visual-query-builder".into());
                step2.set(13);
            }
            13 if b.get_visible() => {
                act("reset", 0, 0, "");
                step2.set(14);
            }
            14 => {
                expect!(sources() == 0, "reset should drop every table");
                expect!(b.get_sql().is_empty(), "reset should clear the SQL");
                expect!(b.get_limit_text().is_empty(), "reset should clear the limit box");
                expect!(b.get_catalog().row_count() == 2, "the catalog is not part of the query");
                b.invoke_close();
                expect!(!b.get_visible(), "Close should hide the builder");
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
