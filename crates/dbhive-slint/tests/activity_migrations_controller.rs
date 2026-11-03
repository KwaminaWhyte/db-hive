//! The Activity Monitor and Schema Migrations driven end to end: the real
//! window, the real controller, two real SQLite files.
//!
//! Activity: queries run in a workspace (one of them failing) land in the
//! activity log; the monitor shows them, expands one, sorts, filters by text
//! and by status, exports to a file, and clears behind a confirmation. The
//! server tab says plainly that SQLite has no session views.
//!
//! Migrations: a source and a target database that differ (a table to add, a
//! table to drop, a column to add). The wizard computes the diff, generates the
//! SQL, refuses to apply until the box is ticked, stops at a confirmation that
//! names the destructive statements, leaves the target alone when cancelled,
//! applies when confirmed (and the target file really changes). Then a failing
//! migration is run inside a transaction (nothing is left behind) and outside
//! one (the earlier statement stays), and each reports it.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own connections, history or logs.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{ActivityState, MigrationsState, MonitorServerState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn tables(path: &std::path::Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn columns(path: &std::path::Path, table: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn profile(id: &str, name: &str, db: &std::path::Path) -> ConnectionProfile {
    let mut p = ConnectionProfile::new(
        id.to_string(),
        name.to_string(),
        DbDriver::Sqlite,
        String::new(),
        0,
        String::new(),
    );
    p.database = Some(db.to_string_lossy().to_string());
    p
}

fn exports(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("query-logs-") && n.ends_with(".csv"))
        })
        .collect()
}

#[test]
fn queries_reach_the_activity_log_and_a_migration_diffs_generates_and_applies() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-activity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    // Source: users(id, email, phone) and orders. Target: users(id, email) and
    // a table the source no longer has.
    let source = dir.join("source.sqlite");
    let target = dir.join("target.sqlite");
    {
        let c = rusqlite::Connection::open(&source).unwrap();
        c.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL, phone TEXT)",
            [],
        )
        .unwrap();
        c.execute(
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO users (email) VALUES ('a@example.com'), ('b@example.com')",
            [],
        )
        .unwrap();
        let c = rusqlite::Connection::open(&target).unwrap();
        c.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL)",
            [],
        )
        .unwrap();
        c.execute("CREATE TABLE old_stuff (id INTEGER PRIMARY KEY)", [])
            .unwrap();
    }
    {
        let mut state = AppState::new();
        state.add_profile(profile("source-db", "Source", &source));
        state.add_profile(profile("target-db", "Target", &target));
        state.save_profiles_to_store().unwrap();
    }

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, dir2, source2, target2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        dir.clone(),
        source.clone(),
        target.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let a = w.global::<ActivityState>();
        let m = w.global::<MigrationsState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(40) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        let idle = !w.get_workspace_running();
        match step2.get() {
            // -- Queries that will fill the log -------------------------------
            0 => {
                w.invoke_connect("source-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tabs().row_count() == 1 => {
                w.invoke_workspace_editor_edited("SELECT email FROM users ORDER BY id".into());
                w.invoke_workspace_run_query();
                step2.set(2);
            }
            2 if idle && w.get_workspace_has_result() => {
                w.invoke_workspace_editor_edited("SELECT * FROM table_that_is_not_there".into());
                w.invoke_workspace_run_query();
                step2.set(3);
            }
            3 if idle && !w.get_workspace_rows_error().is_empty() => {
                w.invoke_workspace_editor_edited("SELECT COUNT(*) FROM orders".into());
                w.invoke_workspace_run_query();
                step2.set(4);
            }

            // -- The monitor lists them ---------------------------------------
            4 if idle && w.get_workspace_has_result() && w.get_workspace_rows_error().is_empty() => {
                w.invoke_run_command("go-activity-monitor".into());
                if !a.get_visible() {
                    return bail("the palette command did not open the Activity Monitor".into());
                }
                if a.get_rows().row_count() != 3 || a.get_log_total() != 3 {
                    return bail(format!(
                        "expected 3 logged queries, rows {} total {}",
                        a.get_rows().row_count(),
                        a.get_log_total()
                    ));
                }
                if a.get_summary() != "3 queries in the log" {
                    return bail(format!("summary was {:?}", a.get_summary()));
                }
                // Newest first by default.
                if a.get_sort_column() != 0 || !a.get_sort_desc() {
                    return bail("the log should open newest first".into());
                }
                // Expand the failed one: it is the second oldest, so row 1 of 3.
                a.invoke_row_clicked(1);
                if !a.get_has_detail()
                    || a.get_detail_status() != "Failed"
                    || !a.get_detail_sql().contains("table_that_is_not_there")
                    || !a.get_detail_error().contains("table_that_is_not_there")
                {
                    return bail(format!(
                        "detail pane wrong: {:?} / {:?} / {:?}",
                        a.get_detail_status(),
                        a.get_detail_sql(),
                        a.get_detail_error()
                    ));
                }
                step2.set(5);
            }
            5 => {
                // Sorting: a header click on Duration sorts by it, a click on
                // the SQL column (not sortable) changes nothing.
                a.invoke_sort_clicked(5);
                if a.get_sort_column() != 5 || !a.get_sort_desc() {
                    return bail("clicking Duration should sort by it, descending".into());
                }
                a.invoke_sort_clicked(5);
                if a.get_sort_desc() {
                    return bail("a second click on Duration should flip to ascending".into());
                }
                a.invoke_sort_clicked(7);
                if a.get_sort_column() != 5 {
                    return bail("the SQL column is not sortable and must be ignored".into());
                }
                // Text filter.
                a.set_search("not_there".into());
                a.invoke_search_edited("not_there".into());
                if a.get_rows().row_count() != 1 || !a.get_filters_active() {
                    return bail(format!("text filter left {} rows", a.get_rows().row_count()));
                }
                if a.get_summary() != "1 of 3 queries match the filters" {
                    return bail(format!("filtered summary was {:?}", a.get_summary()));
                }
                // Reset, then the status chip (Running, Completed, Failed, Cancelled).
                a.invoke_filters_reset();
                if a.get_rows().row_count() != 3 || a.get_filters_active() || !a.get_search().is_empty() {
                    return bail("reset should bring every query back and clear the search box".into());
                }
                a.invoke_status_toggled(2);
                if a.get_rows().row_count() != 1 {
                    return bail(format!("Failed status left {} rows", a.get_rows().row_count()));
                }
                a.invoke_filters_reset();
                // Statistics and the server tab.
                a.invoke_tab_selected(1);
                let tiles: Vec<String> = a.get_tiles().iter().map(|t| format!("{}={}", t.label, t.value)).collect();
                if tiles.first().map(String::as_str) != Some("Queries=3") || !tiles.contains(&"Failed=1".to_string()) {
                    return bail(format!("statistics tiles were {tiles:?}"));
                }
                if a.get_status_bars().row_count() != 2 {
                    return bail("expected Completed and Failed breakdown bars".into());
                }
                a.invoke_tab_selected(2);
                step2.set(6);
            }
            // SQLite has no session views: said plainly, not as an error.
            6 if a.get_server_state() == MonitorServerState::Unsupported => {
                if !a.get_server_message().contains("Not supported") {
                    return bail(format!("unsupported message was {:?}", a.get_server_message()));
                }
                a.invoke_tab_selected(0);
                // Export to a file.
                if !exports(&dir2).is_empty() {
                    return bail("an export file exists before exporting".into());
                }
                a.invoke_export_picked(0);
                step2.set(7);
            }
            7 if !exports(&dir2).is_empty() && !a.get_exporting() => {
                let csv = std::fs::read_to_string(&exports(&dir2)[0]).unwrap();
                if !csv.contains("SELECT email FROM users") || !csv.contains("table_that_is_not_there") {
                    return bail(format!("export is missing queries:\n{csv}"));
                }
                // Clear asks first, and cancelling keeps the log.
                a.invoke_clear_requested();
                if !w.get_show_confirm() || w.get_confirm_title() != "Clear the activity log?" {
                    return bail(format!("clear confirmation was {:?}", w.get_confirm_title()));
                }
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                if a.get_log_total() != 3 {
                    return bail("cancelling the confirmation cleared the log".into());
                }
                a.invoke_clear_requested();
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                if a.get_log_total() != 0 || a.get_rows().row_count() != 0 {
                    return bail("confirming did not clear the log".into());
                }
                // Escape closes the panel.
                let esc: slint::SharedString = slint::platform::Key::Escape.into();
                w.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: esc.clone() });
                w.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text: esc });
                if a.get_visible() {
                    return bail("Escape did not hide the monitor".into());
                }
                // Now the second database, for the migration.
                w.invoke_connect("target-db".into());
                step2.set(8);
            }

            // -- Migrations ----------------------------------------------------
            8 if w.get_workspace_connection_name() == "Target" => {
                w.invoke_run_command("go-migrations".into());
                if !m.get_visible() || m.get_step() != 0 {
                    return bail("go-migrations should open the wizard at step one".into());
                }
                if m.get_connections().row_count() != 2 {
                    return bail("both saved connections should be offered".into());
                }
                // Nothing chosen yet: Compute diff is blocked and says why.
                if !m.get_blocked_reason().contains("both") {
                    return bail(format!("blocked reason was {:?}", m.get_blocked_reason()));
                }
                // Source is "Source" (first by name), target is "Target".
                m.invoke_source_picked(0);
                m.invoke_target_picked(1);
                if m.get_schema() != "main" {
                    return bail(format!("a SQLite target should suggest schema main, got {:?}", m.get_schema()));
                }
                if !m.get_blocked_reason().is_empty() {
                    return bail(format!("both are open, yet blocked: {:?}", m.get_blocked_reason()));
                }
                m.invoke_compute_diff();
                step2.set(9);
            }
            9 if m.get_step() == 1 && !m.get_busy() => {
                let labels: Vec<String> = m.get_diff_rows().iter().map(|r| r.label.to_string()).collect();
                for want in ["main.orders", "main.old_stuff", "main.users", "column phone"] {
                    if !labels.iter().any(|l| l == want) {
                        return bail(format!("diff is missing {want:?}: {labels:?}"));
                    }
                }
                m.invoke_generate();
                if m.get_step() != 2 {
                    return bail("Generate SQL should reach the SQL step".into());
                }
                let sql = m.get_sql().to_string();
                if !sql.contains("CREATE TABLE \"main\".\"orders\"")
                    || !sql.contains("ALTER TABLE \"main\".\"users\" ADD COLUMN \"phone\"")
                    || !sql.contains("DROP TABLE \"main\".\"old_stuff\"")
                {
                    return bail(format!("generated SQL is wrong:\n{sql}"));
                }
                if !m.get_sql_summary().contains("(1 destructive)") {
                    return bail(format!("summary should flag the DROP: {:?}", m.get_sql_summary()));
                }
                // Apply is refused until the box is ticked, even if the
                // callback is fired anyway.
                m.invoke_apply_requested();
                if w.get_show_confirm() {
                    return bail("Apply reached the confirmation without the acknowledgement".into());
                }
                m.invoke_acknowledge_toggled();
                if !m.get_blocked_reason().is_empty() {
                    return bail(format!("ticked, yet blocked: {:?}", m.get_blocked_reason()));
                }
                // The confirmation names the destructive statement; cancelling
                // leaves the target alone.
                m.invoke_apply_requested();
                if !w.get_show_confirm()
                    || w.get_confirm_title() != "Apply this migration?"
                    || !w.get_confirm_message().contains("1 of them drop or delete data")
                    || !w.get_confirm_message().contains("rolls everything back")
                {
                    return bail(format!(
                        "apply confirmation wrong: {:?} / {:?}",
                        w.get_confirm_title(),
                        w.get_confirm_message()
                    ));
                }
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                std::thread::sleep(Duration::from_millis(100));
                if tables(&target2) != vec!["old_stuff", "users"] {
                    return bail(format!("cancelling still changed the target: {:?}", tables(&target2)));
                }
                m.invoke_apply_requested();
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(10);
            }
            10 if m.get_has_outcome() && !m.get_busy() => {
                if !m.get_outcome_ok() || !m.get_outcome_headline().starts_with("Applied 3 statement") {
                    return bail(format!(
                        "apply outcome: ok {} / {:?} / {:?}",
                        m.get_outcome_ok(),
                        m.get_outcome_headline(),
                        m.get_outcome_detail()
                    ));
                }
                if tables(&target2) != vec!["orders", "users"] || !columns(&target2, "users").contains(&"phone".to_string()) {
                    return bail(format!(
                        "target after apply: {:?} users{:?}",
                        tables(&target2),
                        columns(&target2, "users")
                    ));
                }
                // Applying again needs a fresh acknowledgement.
                if m.get_acknowledged() || m.get_blocked_reason().is_empty() {
                    return bail("the acknowledgement should reset after an apply".into());
                }
                // A failing migration inside a transaction leaves nothing.
                m.invoke_sql_changed(
                    "CREATE TABLE kept_if_no_tx (id INTEGER);\n\nALTER TABLE not_a_table ADD COLUMN x TEXT;".into(),
                );
                m.invoke_acknowledge_toggled();
                m.invoke_apply_requested();
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(11);
            }
            11 if m.get_has_outcome() && !m.get_busy() && !m.get_outcome_ok() => {
                if !m.get_outcome_detail().contains("Rolled back") || !m.get_outcome_statement().contains("not_a_table") {
                    return bail(format!(
                        "transaction failure report: {:?} / {:?}",
                        m.get_outcome_detail(),
                        m.get_outcome_statement()
                    ));
                }
                if tables(&target2).contains(&"kept_if_no_tx".to_string()) {
                    return bail("a rolled back migration left its first table behind".into());
                }
                // Without a transaction the first statement stays applied.
                m.invoke_transaction_toggled();
                m.invoke_acknowledge_toggled();
                m.invoke_apply_requested();
                if !w.get_confirm_message().contains("No transaction") {
                    return bail(format!("confirmation should warn: {:?}", w.get_confirm_message()));
                }
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(12);
            }
            12 if !m.get_busy() && m.get_outcome_detail().contains("stay applied") => {
                if !tables(&target2).contains(&"kept_if_no_tx".to_string()) {
                    return bail("without a transaction the first statement should have stayed".into());
                }
                if tables(&source2) != vec!["orders", "users"] {
                    return bail("the source database must never be touched".into());
                }
                m.invoke_close();
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
