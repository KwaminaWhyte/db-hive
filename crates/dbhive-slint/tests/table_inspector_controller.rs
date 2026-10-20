//! The table inspector driven end to end: the real window, the real
//! controller, a real SQLite file with more rows than one page.
//!
//! Covers what the unit tests in `dbhive_ui_model::table_view` cannot: that a
//! table tab pages through the database (first/next/last, page size), sorts
//! and filters in SQL (including a value with a quote in it), counts rows,
//! shows the structure (columns, indexes, foreign keys), opens a foreign key's
//! target in its own tab, copies a cell, a row, a column and a row's JSON to
//! the real clipboard path, and ends in a visible error when the table
//! vanishes.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, InspectorBridge, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

slint::slint! {
    // A window with one text box, to read the system clipboard back by pasting.
    export component PasteProbe inherits Window {
        in-out property <string> pasted <=> box.text;
        // Multi-line, or pasting would turn the copied newlines into spaces.
        box := TextInput { single-line: false; }
        public function paste-now() {
            box.text = "";
            box.paste();
        }
    }
}

const ORDERS: usize = 130;

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

fn settled(w: &AppWindow) -> bool {
    !w.get_workspace_running()
}

#[test]
fn page_filter_sort_inspect_and_copy_a_real_table() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-inspector-{}", std::process::id()));
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
                 customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
                 note TEXT,
                 total INTEGER
             );
             CREATE INDEX ix_orders_customer ON orders(customer_id);
             CREATE TABLE scratch (id INTEGER PRIMARY KEY, v TEXT);",
        )
        .unwrap();
        for i in 1..=5 {
            conn.execute(
                "INSERT INTO customers (name, email) VALUES (?1, ?2)",
                rusqlite::params![format!("Customer {i}"), format!("c{i}@example.com")],
            )
            .unwrap();
        }
        for i in 1..=ORDERS {
            // Every tenth order is a gift; order 7 has an apostrophe; order 3 has no note.
            let note: Option<String> = if i == 3 {
                None
            } else if i == 7 {
                Some("it's fragile".to_string())
            } else if i % 10 == 0 {
                Some(format!("gift-{i}"))
            } else {
                Some(format!("order-{i}"))
            };
            conn.execute(
                "INSERT INTO orders (customer_id, note, total) VALUES (?1, ?2, ?3)",
                rusqlite::params![(i % 5) as i64 + 1, note, (i * 10) as i64],
            )
            .unwrap();
        }
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
    let probe = PasteProbe::new().unwrap();
    probe.show().unwrap();

    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let quiet_since: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let (weak, probe_weak, fail, step2, db2) = (
        w.as_weak(),
        probe.as_weak(),
        failure.clone(),
        step.clone(),
        db.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let probe = probe_weak.upgrade().unwrap();
        let b = w.global::<InspectorBridge>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(30) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        // Read what a copy put on the clipboard.
        let pasted = || {
            probe.invoke_paste_now();
            probe.get_pasted().to_string()
        };
        macro_rules! expect {
            ($cond:expr, $($msg:tt)+) => {
                if !($cond) {
                    return bail(format!($($msg)+));
                }
            };
        }

        match step2.get() {
            // -- connect, open the orders table ---------------------------
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tables().row_count() == 3 =>
            {
                w.invoke_workspace_select_table("orders".into());
                step2.set(2);
            }

            // -- page 1 of 3 ----------------------------------------------
            2 if w.get_workspace_tabs().row_count() == 2
                && settled(&w)
                && w.get_workspace_has_result()
                && b.get_total_label() == "130 rows" =>
            {
                expect!(
                    !w.get_workspace_active_is_query(),
                    "the orders tab should be a table tab"
                );
                expect!(
                    rows(&w) == 50,
                    "page 1 should have 50 rows, got {}",
                    rows(&w)
                );
                expect!(
                    b.get_page_label() == "Page 1 of 3",
                    "page label was {:?}",
                    b.get_page_label()
                );
                expect!(
                    b.get_range_label() == "1-50 of 130 rows",
                    "range label was {:?}",
                    b.get_range_label()
                );
                expect!(
                    !b.get_can_prev() && !b.get_can_first() && b.get_can_next(),
                    "pager buttons wrong on page 1"
                );
                expect!(
                    cell(&w, 0, 0) == "1" && cell(&w, 49, 0) == "50",
                    "page 1 should be ids 1..50, got {} .. {}",
                    cell(&w, 0, 0),
                    cell(&w, 49, 0)
                );
                b.invoke_go_next();
                step2.set(3);
            }

            // -- page 2: ordered by the primary key, no gap or overlap ----
            3 if settled(&w) && b.get_page_label() == "Page 2 of 3" => {
                expect!(
                    rows(&w) == 50,
                    "page 2 should have 50 rows, got {}",
                    rows(&w)
                );
                expect!(
                    cell(&w, 0, 0) == "51" && cell(&w, 49, 0) == "100",
                    "page 2 should be ids 51..100, got {} .. {}",
                    cell(&w, 0, 0),
                    cell(&w, 49, 0)
                );
                b.invoke_go_next();
                step2.set(4);
            }

            // -- page 3: the short last page, Next is off -----------------
            4 if settled(&w) && b.get_page_label() == "Page 3 of 3" => {
                expect!(
                    rows(&w) == 30,
                    "the last page should have 30 rows, got {}",
                    rows(&w)
                );
                expect!(
                    b.get_range_label() == "101-130 of 130 rows",
                    "range label was {:?}",
                    b.get_range_label()
                );
                expect!(
                    !b.get_can_next() && b.get_can_prev(),
                    "pager buttons wrong on the last page"
                );
                expect!(
                    cell(&w, 29, 0) == "130",
                    "last row should be id 130, got {}",
                    cell(&w, 29, 0)
                );
                // Next on the last page must do nothing.
                b.invoke_go_next();
                quiet_since.set(Some(Instant::now()));
                step2.set(5);
            }
            5 => {
                // Let a wrongly started fetch have time to land (the loop keeps
                // running between ticks), then check we are still on page 3.
                if quiet_since.get().unwrap().elapsed() < Duration::from_millis(300) {
                    return;
                }
                expect!(
                    b.get_page_label() == "Page 3 of 3" && settled(&w),
                    "Next past the last page moved to {:?}",
                    b.get_page_label()
                );
                // A bigger page size returns to page 1.
                b.invoke_page_size_picked(2);
                step2.set(6);
            }
            6 if settled(&w) && b.get_page_label() == "Page 1 of 2" => {
                expect!(
                    rows(&w) == 100,
                    "100 per page should show 100 rows, got {}",
                    rows(&w)
                );
                expect!(
                    b.get_page_size_label() == "100 / page",
                    "page size label was {:?}",
                    b.get_page_size_label()
                );
                // Sort by id: ascending, then descending.
                b.invoke_sort_by(0);
                step2.set(7);
            }

            // -- sort -----------------------------------------------------
            7 if settled(&w) && b.get_sort_column() == 0 && !b.get_sort_desc() => {
                expect!(
                    cell(&w, 0, 0) == "1",
                    "ascending should start at 1, got {}",
                    cell(&w, 0, 0)
                );
                b.invoke_sort_by(0);
                step2.set(8);
            }
            8 if settled(&w) && b.get_sort_desc() => {
                // The sort is the database's, so descending starts at the table's last row,
                // not at the last row of the page that happened to be showing.
                expect!(
                    cell(&w, 0, 0) == "130",
                    "descending should start at 130, got {}",
                    cell(&w, 0, 0)
                );
                expect!(
                    cell(&w, 99, 0) == "31",
                    "descending page 1 should end at 31, got {}",
                    cell(&w, 99, 0)
                );
                b.invoke_sort_by(0); // off
                step2.set(9);
            }
            9 if settled(&w) && b.get_sort_column() == -1 => {
                expect!(
                    cell(&w, 0, 0) == "1",
                    "unsorted should be in key order again, got {}",
                    cell(&w, 0, 0)
                );

                // -- filter: editing alone changes nothing ----------------
                b.invoke_add_filter();
                step2.set(10);
            }
            10 if b.get_filter_rows().row_count() == 1 => {
                let cols: Vec<String> = b
                    .get_filter_columns()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                expect!(
                    cols == ["id", "customer_id", "note", "total"],
                    "filter columns were {cols:?}"
                );
                b.invoke_filter_column_picked(0, 2); // note
                step2.set(11);
            }
            11 if b
                .get_filter_rows()
                .row_data(0)
                .map(|r| r.column == "note")
                .unwrap_or(false) =>
            {
                b.invoke_filter_value_edited(0, "gift".into());
                expect!(
                    b.get_filters_dirty(),
                    "typing a filter value should mark the filters dirty"
                );
                expect!(
                    b.get_total_label() == "130 rows",
                    "the count must not change before Apply"
                );
                expect!(rows(&w) == 100, "the rows must not change before Apply");
                b.invoke_apply_filters();
                step2.set(12);
            }
            12 if settled(&w) && b.get_total_label() == "13 rows match" => {
                expect!(
                    rows(&w) == 13,
                    "the gift filter should leave 13 rows, got {}",
                    rows(&w)
                );
                expect!(
                    b.get_page_label() == "Page 1 of 1",
                    "page label was {:?}",
                    b.get_page_label()
                );
                expect!(
                    b.get_applied_filter_count() == 1 && !b.get_filters_dirty(),
                    "applied filter bookkeeping is wrong"
                );
                expect!(
                    (0..rows(&w)).all(|r| cell(&w, r, 2).starts_with("gift-")),
                    "a row without 'gift' got through"
                );

                // A value with a quote must be escaped, not break the SQL.
                b.invoke_filter_value_edited(0, "it's".into());
                b.invoke_apply_filters();
                step2.set(13);
            }
            13 if settled(&w) && b.get_total_label() == "1 row match" => {
                expect!(
                    w.get_workspace_rows_error().is_empty(),
                    "a quote in a filter value broke the query: {}",
                    w.get_workspace_rows_error()
                );
                expect!(
                    rows(&w) == 1 && cell(&w, 0, 2) == "it's fragile",
                    "the apostrophe filter should find exactly order 7"
                );

                // -- copy: cell, row, column, JSON -----------------------
                b.invoke_cell_clicked(0, 2);
                step2.set(14);
            }
            14 if b.get_selected_row() == 0 && b.get_selected_col() == 2 => {
                expect!(
                    b.get_panel_open(),
                    "selecting a cell should open the row panel"
                );
                expect!(
                    b.get_panel_title() == "Row 1 · note",
                    "panel title was {:?}",
                    b.get_panel_title()
                );
                let json: serde_json::Value =
                    serde_json::from_str(&b.get_panel_json()).expect("panel JSON must parse");
                expect!(
                    json["note"] == "it's fragile" && json["id"] == 7,
                    "panel JSON was {json}"
                );

                b.invoke_copy_cell();
                let got = pasted();
                expect!(
                    got == "it's fragile",
                    "copy cell put {got:?} on the clipboard"
                );

                b.invoke_copy_row();
                let got = pasted();
                expect!(
                    got == "7\t3\tit's fragile\t70",
                    "copy row put {got:?} on the clipboard"
                );

                b.invoke_copy_json();
                let got = pasted();
                let json: serde_json::Value =
                    serde_json::from_str(&got).expect("copied JSON must parse");
                expect!(
                    json["total"] == 70,
                    "copy JSON put {got:?} on the clipboard"
                );

                b.invoke_copy_column();
                let got = pasted();
                expect!(
                    got == "it's fragile",
                    "a one-row page copies one value in the column, got {got:?}"
                );

                // Clearing the filters brings every row back.
                b.invoke_clear_filters();
                step2.set(15);
            }
            15 if settled(&w) && b.get_total_label() == "130 rows" && rows(&w) == 100 => {
                expect!(
                    b.get_applied_filter_count() == 0,
                    "clearing should drop the applied filters"
                );
                expect!(
                    !b.get_panel_open(),
                    "a new page must drop the selection and close the panel"
                );
                // A column copy takes the whole page.
                b.invoke_cell_clicked(1, 0);
                step2.set(16);
            }
            16 if b.get_selected_row() == 1 => {
                b.invoke_copy_column();
                let got = pasted();
                let lines: Vec<&str> = got.split('\n').collect();
                expect!(
                    lines.len() == 100 && lines[0] == "1" && lines[99] == "100",
                    "copy column was {} lines, first {:?}",
                    lines.len(),
                    lines.first()
                );

                // -- structure -------------------------------------------
                b.invoke_set_view(1);
                step2.set(17);
            }
            17 if b.get_view() == 1 && b.get_struct_columns().row_count() == 4 => {
                let cols = b.get_struct_columns();
                let id = cols.row_data(0).unwrap();
                expect!(
                    id.name == "id" && id.primary && id.auto,
                    "id should be an auto-increment primary key"
                );
                let fk = cols.row_data(1).unwrap();
                expect!(
                    fk.name == "customer_id"
                        && fk.foreign == "customers(id)"
                        && fk.foreign_table == "customers"
                        && !fk.nullable,
                    "customer_id should be a NOT NULL foreign key to customers(id), got {:?} {:?}",
                    fk.name,
                    fk.foreign
                );
                let note = cols.row_data(2).unwrap();
                expect!(
                    note.nullable && !note.primary && note.foreign.is_empty(),
                    "note should be a plain nullable column"
                );
                expect!(
                    b.get_struct_indexes()
                        .iter()
                        .any(|i| i.name == "ix_orders_customer"
                            && i.columns == "customer_id"
                            && i.kind == "INDEX"),
                    "the customer index is missing from the structure"
                );
                expect!(
                    b.get_struct_relations().row_count() == 1,
                    "orders has one foreign key, got {}",
                    b.get_struct_relations().row_count()
                );
                let rel = b.get_struct_relations().row_data(0).unwrap();
                expect!(
                    rel.outgoing
                        && rel.other_table == "customers"
                        && rel.actions == "CASCADE / NO ACTION",
                    "relation was {:?} {:?}",
                    rel.other_table,
                    rel.actions
                );

                // Following the foreign key opens the target in its own tab.
                b.invoke_open_table("customers".into());
                step2.set(18);
            }
            18 if w.get_workspace_tabs().row_count() == 3
                && settled(&w)
                && b.get_total_label() == "5 rows" =>
            {
                expect!(
                    w.get_workspace_selected_table() == "customers",
                    "the customers tab should be active"
                );
                expect!(b.get_view() == 0, "a newly opened table starts on Data");
                expect!(rows(&w) == 5, "customers has 5 rows, got {}", rows(&w));
                expect!(
                    b.get_page_label() == "Page 1 of 1" && !b.get_can_next(),
                    "a one-page table has no next page"
                );

                // Its structure lists the key pointing at it, "referenced by".
                b.invoke_set_view(1);
                step2.set(19);
            }
            19 if b.get_view() == 1 && b.get_struct_columns().row_count() == 3 => {
                expect!(
                    b.get_struct_relations().row_count() == 1,
                    "customers is referenced by orders"
                );
                let rel = b.get_struct_relations().row_data(0).unwrap();
                expect!(
                    !rel.outgoing && rel.other_table == "orders",
                    "relation should be incoming from orders, got {:?}",
                    rel.other_table
                );

                // A table that is not in this schema is refused with a message, not a tab.
                b.invoke_open_table("no_such_table".into());
                expect!(
                    w.get_workspace_tabs().row_count() == 3,
                    "a missing table must not open a tab"
                );
                expect!(
                    w.get_toasts()
                        .iter()
                        .any(|t| t.text.contains("no_such_table")),
                    "opening a missing table should say so"
                );

                // -- empty table, then a vanished one ---------------------
                w.invoke_workspace_select_table("scratch".into());
                step2.set(20);
            }
            20 if w.get_workspace_tabs().row_count() == 4
                && settled(&w)
                && w.get_workspace_has_result() =>
            {
                expect!(
                    rows(&w) == 0 && b.get_total_label() == "0 rows",
                    "scratch is empty: {} rows, label {:?}",
                    rows(&w),
                    b.get_total_label()
                );
                expect!(
                    w.get_workspace_rows_error().is_empty(),
                    "an empty table is not an error"
                );
                // The table disappears underneath us.
                rusqlite::Connection::open(&db2)
                    .unwrap()
                    .execute("DROP TABLE scratch", [])
                    .unwrap();
                b.invoke_refresh();
                step2.set(21);
            }
            21 if settled(&w) && !w.get_workspace_rows_error().is_empty() => {
                expect!(
                    w.get_workspace_rows_error().contains("scratch"),
                    "the error should name the table, got {:?}",
                    w.get_workspace_rows_error()
                );
                expect!(!w.get_workspace_has_result(), "an error replaces the rows");
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
