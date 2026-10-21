//! The visual schema designer driven end to end: the real window, the real
//! controller, a real SQLite file.
//!
//! Design three tables with a chain of foreign keys through the same
//! callbacks the properties panel uses, see the problems list shrink as the
//! design becomes valid, preview the generated SQL, and apply it against a
//! database that already has a table named like one of the new ones. That
//! table fails, the table that references it is skipped, and the independent
//! one is still created. Fix the name (the reference follows the rename) and
//! apply again: only the two outstanding tables are created, the one made
//! earlier is left alone. Then the ER diagram, opened from the designer,
//! shows all four tables because creating them invalidated its cache.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own connections or layouts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{DesignerState, ErdState, Screen};
use dbhive_ui_model::designer_session::type_index;
use dbhive_ui_model::table_editor::TypeKind;
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn tables(db: &std::path::Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn design_tables_preview_apply_with_a_failure_and_retry() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-designer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        // A table that will collide with one of the designed ones.
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("CREATE TABLE orders (id INTEGER PRIMARY KEY)", [])
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
    w.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, db2) = (w.as_weak(), failure.clone(), step.clone(), db.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let st = w.global::<DesignerState>();
        // See erd_controller: a pointer event makes the headless window
        // instantiate the `if` blocks a real frame would.
        w.window()
            .dispatch_event(slint::platform::WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(5.0, 5.0),
            });
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(40) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace
                && w.get_workspace_tables().row_count() == 1 =>
            {
                w.invoke_run_command("go-schema-designer".into());
                if !st.get_visible() {
                    return bail("the palette command did not open the designer".into());
                }
                if !st.get_empty() || st.get_has_selection() {
                    return bail("a fresh designer should be empty".into());
                }
                if !st.get_subtitle().contains("SQLite") {
                    return bail(format!(
                        "subtitle should name the driver: {:?}",
                        st.get_subtitle()
                    ));
                }
                step2.set(2);
            }

            // Build users(id, email VARCHAR(40) unique), orders(id, user_id)
            // and order_items(id, order_id) with the panel's own callbacks.
            2 => {
                st.invoke_add_table();
                if st.get_empty() || !st.get_has_selection() || st.get_nodes().row_count() != 1 {
                    return bail("adding a table should select it on the canvas".into());
                }
                if st.get_problems().is_empty() || st.get_can_apply() {
                    return bail("an unnamed table is a problem and cannot be applied".into());
                }
                st.set_table_name("users".into());
                st.invoke_table_name_edited("users".into());
                if !st.get_problems().is_empty() {
                    return bail(format!(
                        "a named table with a key is valid, got {:?}",
                        st.get_problems()
                    ));
                }
                if !st.get_can_apply() || st.get_apply_label() != "Create 1 table" {
                    return bail(format!("apply label was {:?}", st.get_apply_label()));
                }
                st.invoke_add_column();
                if !st.get_has_column() {
                    return bail("adding a column should select it".into());
                }
                st.invoke_col_name_edited("email".into());
                st.invoke_col_type_picked(type_index(TypeKind::Varchar) as i32);
                if !st.get_col_takes_length() {
                    return bail("VARCHAR should ask for a length".into());
                }
                st.invoke_col_length_edited("40".into());
                st.invoke_toggle_unique();
                if !st.get_col_unique() || st.get_col_type_label() != "VARCHAR(n)" {
                    return bail("unique or type did not stick".into());
                }
                let nodes = st.get_nodes();
                let users = nodes.row_data(0).unwrap();
                let email = users.rows.row_data(1).unwrap();
                if email.name != "email" || email.detail != "VARCHAR(40) UNQ" {
                    return bail(format!(
                        "the table box should read the column back: {:?} {:?}",
                        email.name, email.detail
                    ));
                }

                st.invoke_add_table();
                st.invoke_table_name_edited("orders".into());
                st.invoke_add_column();
                st.invoke_col_name_edited("user_id".into());
                st.invoke_col_type_picked(type_index(TypeKind::BigInt) as i32);
                let options: Vec<String> = st
                    .get_ref_table_options()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                if options != ["(none)", "users", "orders"] {
                    return bail(format!("reference targets were {:?}", options));
                }
                st.invoke_ref_table_picked(1);
                // users has two candidate keys (id, email): the user picks.
                if st.get_ref_column_value() != "(choose)" || !st.get_ref_column_enabled() {
                    return bail(format!(
                        "expected a choice of key column, got {:?}",
                        st.get_ref_column_value()
                    ));
                }
                if !st.get_problems().contains("referenced table and column") {
                    return bail(format!(
                        "a half-set reference should be a problem: {:?}",
                        st.get_problems()
                    ));
                }
                st.invoke_ref_column_picked(0);
                if st.get_ref_column_value() != "id" || !st.get_problems().is_empty() {
                    return bail(format!(
                        "reference to users.id: {:?} / {:?}",
                        st.get_ref_column_value(),
                        st.get_problems()
                    ));
                }
                if st.get_edges().row_count() != 1 {
                    return bail("the resolved reference should be drawn".into());
                }

                st.invoke_add_table();
                st.invoke_table_name_edited("order_items".into());
                st.invoke_add_column();
                st.invoke_col_name_edited("order_id".into());
                st.invoke_col_type_picked(type_index(TypeKind::BigInt) as i32);
                st.invoke_ref_table_picked(2); // orders
                if st.get_ref_column_value() != "id" {
                    return bail(format!(
                        "a single candidate key is chosen for you, got {:?}",
                        st.get_ref_column_value()
                    ));
                }
                if st.get_apply_label() != "Create 3 tables" || !st.get_can_apply() {
                    return bail(format!("apply label {:?}", st.get_apply_label()));
                }
                step2.set(3);
            }

            // Preview: referenced tables first, references in the DDL, nothing run.
            3 => {
                st.invoke_preview();
                let sql = st.get_preview_sql().to_string();
                let (u, o, i) = (
                    sql.find("CREATE TABLE \"users\""),
                    sql.find("CREATE TABLE \"orders\""),
                    sql.find("CREATE TABLE \"order_items\""),
                );
                let (Some(u), Some(o), Some(i)) = (u, o, i) else {
                    return bail(format!("preview is missing a table: {sql}"));
                };
                if !(u < o && o < i) {
                    return bail("referenced tables must be created first".into());
                }
                if !sql.contains("REFERENCES \"users\"")
                    || !sql.contains("VARCHAR(40)") && !sql.contains("TEXT")
                {
                    return bail(format!("preview does not show the design: {sql}"));
                }
                if !st.get_preview_open() || !st.get_preview_problems().is_empty() {
                    return bail("preview should be open and clean".into());
                }
                if tables(&db2) != ["orders"] {
                    return bail(format!("previewing created something: {:?}", tables(&db2)));
                }
                st.invoke_preview_close();
                if st.get_preview_open() {
                    return bail("preview did not close".into());
                }
                // Apply: users is created, orders collides, order_items is skipped.
                st.invoke_apply();
                if !st.get_applying() {
                    return bail("applying should show the busy state".into());
                }
                step2.set(4);
            }
            4 if !st.get_applying() && st.get_results().row_count() == 3 => {
                let rows: Vec<(String, i32)> = st
                    .get_results()
                    .iter()
                    .map(|r| (r.name.to_string(), r.state))
                    .collect();
                if rows
                    != [
                        ("users".into(), 0),
                        ("orders".into(), 1),
                        ("order_items".into(), 2),
                    ]
                {
                    return bail(format!("per-table results were {:?}", rows));
                }
                let failed = st.get_results().row_data(1).unwrap();
                if !failed.message.contains("already exists") {
                    return bail(format!(
                        "the failure should carry the database's message: {:?}",
                        failed.message
                    ));
                }
                if !st
                    .get_results()
                    .row_data(2)
                    .unwrap()
                    .message
                    .contains("orders was not created")
                {
                    return bail("the skipped table should say why".into());
                }
                if st.get_results_summary() != "1 created, 1 failed, 1 skipped"
                    || !st.get_preview_open()
                {
                    return bail(format!("summary {:?}", st.get_results_summary()));
                }
                if tables(&db2) != ["orders", "users"] {
                    return bail(format!(
                        "only users should have been created: {:?}",
                        tables(&db2)
                    ));
                }
                // The sidebar learns about the new table.
                if st.get_apply_label() != "Create 2 tables" {
                    return bail(format!(
                        "users is done, two remain: {:?}",
                        st.get_apply_label()
                    ));
                }
                step2.set(5);
            }
            5 if w.get_workspace_tables().row_count() == 2 => {
                st.invoke_preview_close();
                // Fix the clash. The reference in order_items follows the rename.
                st.invoke_node_clicked(1);
                if st.get_table_name() != "orders" {
                    return bail(format!(
                        "selecting the table should load its name: {:?}",
                        st.get_table_name()
                    ));
                }
                st.set_table_name("customer_orders".into());
                st.invoke_table_name_edited("customer_orders".into());
                if !st.get_problems().is_empty() {
                    return bail(format!(
                        "the rename broke the design: {:?}",
                        st.get_problems()
                    ));
                }
                st.invoke_node_clicked(2);
                st.invoke_column_selected(1);
                if st.get_ref_table_value() != "customer_orders" {
                    return bail(format!(
                        "the reference did not follow the rename: {:?}",
                        st.get_ref_table_value()
                    ));
                }
                // A drag moves one table and snaps to the grid.
                let x0 = st.get_nodes().row_data(2).unwrap().x;
                st.invoke_node_dragged(2, 40.0, 0.0);
                st.invoke_node_released(2);
                let x1 = st.get_nodes().row_data(2).unwrap().x;
                if x1 == x0 || x1 % 16.0 != 0.0 {
                    return bail(format!("drag moved {x0} to {x1}"));
                }
                st.invoke_apply();
                step2.set(6);
            }
            6 if !st.get_applying() && st.get_results().row_count() == 2 => {
                let rows: Vec<(String, i32)> = st
                    .get_results()
                    .iter()
                    .map(|r| (r.name.to_string(), r.state))
                    .collect();
                if rows != [("customer_orders".into(), 0), ("order_items".into(), 0)] {
                    return bail(format!(
                        "the retry should create only the two outstanding tables: {:?}",
                        rows
                    ));
                }
                if tables(&db2) != ["customer_orders", "order_items", "orders", "users"] {
                    return bail(format!("tables are {:?}", tables(&db2)));
                }
                if st.get_can_apply() || st.get_apply_label() != "Nothing to create" {
                    return bail(format!(
                        "everything is created, got {:?}",
                        st.get_apply_label()
                    ));
                }
                // Created tables are tagged on the canvas.
                if !st
                    .get_nodes()
                    .iter()
                    .any(|n| n.title == "users" && n.tag == "created")
                {
                    return bail("users should be tagged as created".into());
                }
                st.invoke_preview_close();
                // The ER diagram reads the schema afresh: all four tables.
                st.invoke_open_erd();
                if st.get_visible() {
                    return bail("opening the diagram should close the designer".into());
                }
                step2.set(7);
            }
            7 => {
                let erd = w.global::<ErdState>();
                if erd.get_visible() && !erd.get_loading() && erd.get_nodes().row_count() == 4 {
                    let names: Vec<String> = erd
                        .get_nodes()
                        .iter()
                        .map(|n| n.title.to_string())
                        .collect();
                    for t in ["customer_orders", "order_items", "orders", "users"] {
                        if !names.iter().any(|n| n == t) {
                            return bail(format!("the diagram is missing {t}: {:?}", names));
                        }
                    }
                    if erd.get_edges().row_count() < 2 {
                        return bail("the designed foreign keys should be drawn".into());
                    }
                    step2.set(99);
                    slint::quit_event_loop().unwrap();
                }
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
