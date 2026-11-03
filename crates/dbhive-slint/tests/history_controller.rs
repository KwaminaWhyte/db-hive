//! The History / Snippets / Templates panel driven end to end: the real
//! window, the real controller, a real SQLite file.
//!
//! The main flow: run a query, see it in history, re-run it from history into
//! a NEW tab and watch it run. Then the guards that matter: re-running a
//! `DELETE` stops at the destructive-statement confirmation, and a query that
//! belongs to another connection (or failed last time) is opened but never
//! run. Then a snippet is created and inserted, a parameterised template is
//! created and run (with a quote in a value, to prove it is escaped), and
//! history is cleared behind a confirmation.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own history, snippets or connections.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver, QueryHistory};
use dbhive_core::state::AppState;
use dbhive_slint::{HistoryState, PanelMode, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn sqlite_count(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM widgets", [], |r| r.get(0))
        .unwrap()
}

/// The row of `rows` whose title starts with `prefix`, as (index, id).
fn find_row(rows: slint::ModelRc<dbhive_slint::PanelRow>, prefix: &str) -> Option<String> {
    rows.iter()
        .find(|r| r.title.starts_with(prefix))
        .map(|r| r.id.to_string())
}

fn toast_texts(w: &dbhive_slint::AppWindow) -> Vec<String> {
    w.get_toasts().iter().map(|t| t.text.to_string()).collect()
}

#[test]
fn run_a_query_find_it_in_history_and_rerun_it_into_a_new_tab() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-history-{}", std::process::id()));
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
    let db_name = db.to_string_lossy().to_string();

    // Saved connection, plus three history entries from an earlier session:
    // a DELETE that worked, a query that ran on another connection, and one
    // that failed.
    {
        let mut profile = ConnectionProfile::new(
            "widgets-db".to_string(),
            "Widgets".to_string(),
            DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        profile.database = Some(db_name.clone());
        let mut state = AppState::new();
        state.add_profile(profile);
        state.save_profiles_to_store().unwrap();

        let at = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
        let mk = |conn: &str, name: &str, sql: &str, mins: i64| {
            QueryHistory::new(
                conn.into(),
                name.into(),
                db_name.clone(),
                sql.into(),
                at(mins),
            )
        };
        let history = vec![
            mk("widgets-db", "Widgets", "DELETE FROM widgets", 30).with_success(2, Some(2)),
            mk(
                "other-db",
                "Elsewhere",
                "SELECT 'from another connection'",
                20,
            )
            .with_success(1, Some(1)),
            mk("widgets-db", "Widgets", "SELECT * FROM missing_table", 10)
                .with_error("no such table: missing_table".into(), Some(1)),
        ];
        AppState::save_history_to_store(&history).unwrap();
    }

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    const SELECT: &str = "SELECT name FROM widgets ORDER BY id";

    let (weak, fail, step2, db2, dir2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        db.clone(),
        dir.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let h = w.global::<HistoryState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(30) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            // Connect, run a query so there is something new in history.
            0 => {
                w.invoke_connect("widgets-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tabs().row_count() == 1 => {
                w.invoke_workspace_editor_edited(SELECT.into());
                w.invoke_workspace_run_query();
                step2.set(2);
            }

            // Open the panel: it is scoped to this connection and database, so
            // the other connection's entry is hidden and the new one is on top.
            2 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                h.invoke_open_requested();
                if !h.get_visible() || h.get_mode() != PanelMode::Lists || h.get_tab() != 0 {
                    return bail("the panel did not open on the history tab".into());
                }
                let rows = h.get_history();
                if rows.row_count() != 3 {
                    return bail(format!("scoped history should have 3 rows, got {}", rows.row_count()));
                }
                let first = rows.row_data(0).unwrap();
                if first.title != SELECT || !first.has_status || !first.ok {
                    return bail(format!("newest row should be the select, got {:?}", first.title));
                }
                if !first.subtitle.contains("Widgets") || !first.subtitle.contains("2 rows") {
                    return bail(format!("row should name the connection and rows: {:?}", first.subtitle));
                }
                if find_row(h.get_history(), "SELECT 'from another").is_some() {
                    return bail("the other connection's query leaked into the scoped list".into());
                }
                let failed = find_row(h.get_history(), "SELECT * FROM missing").unwrap();
                let failed_row = h.get_history().iter().find(|r| r.id == failed.as_str()).unwrap();
                if failed_row.ok || !failed_row.note.contains("no such table") {
                    return bail("the failed row should show its error".into());
                }

                // Re-run the select from history: a NEW tab, run through the
                // normal path.
                h.invoke_history_rerun(first.id.clone());
                if h.get_visible() {
                    return bail("re-run should close the panel".into());
                }
                if w.get_workspace_tabs().row_count() != 2 || w.get_workspace_active_tab() != 1 {
                    return bail(format!(
                        "re-run should open and focus a second tab, tabs={} active={}",
                        w.get_workspace_tabs().row_count(),
                        w.get_workspace_active_tab()
                    ));
                }
                if w.get_workspace_editor_text() != SELECT {
                    return bail("the new tab should hold the history entry's SQL".into());
                }
                step2.set(3);
            }
            3 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                if w.get_workspace_row_count() != 2 {
                    return bail(format!("re-run should have produced 2 rows, got {}", w.get_workspace_row_count()));
                }
                // The first tab is untouched.
                w.invoke_workspace_tab_selected(0);
                if w.get_workspace_editor_text() != SELECT {
                    return bail("the original tab lost its SQL".into());
                }
                w.invoke_workspace_tab_selected(1);

                // Unscope: every connection's history shows.
                h.invoke_open_requested();
                h.invoke_scope_toggled();
                if h.get_history().row_count() != 5 {
                    return bail(format!("unscoped history should have 5 rows, got {}", h.get_history().row_count()));
                }
                // Re-running the DELETE must stop at the destructive guard.
                let delete = find_row(h.get_history(), "DELETE FROM widgets").unwrap();
                h.invoke_history_rerun(delete.into());
                step2.set(4);
            }
            4 if w.get_show_confirm() => {
                if w.get_confirm_title() != "Run this statement?" {
                    return bail(format!("expected the guard, got {:?}", w.get_confirm_title()));
                }
                if w.get_workspace_tabs().row_count() != 3 {
                    return bail("the DELETE should have opened its own tab".into());
                }
                if sqlite_count(&db2) != 2 {
                    return bail("the DELETE ran before it was confirmed".into());
                }
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                if sqlite_count(&db2) != 2 {
                    return bail("cancelling the guard still deleted rows".into());
                }

                // Another connection's query, and a failed one: opened, not run.
                // The scope choice sticks, so the list is still unscoped.
                h.invoke_open_requested();
                if h.get_history().row_count() != 5 {
                    return bail(format!("unscoped history should still have 5 rows, got {}", h.get_history().row_count()));
                }
                let other = find_row(h.get_history(), "SELECT 'from another").unwrap();
                h.invoke_history_rerun(other.into());
                if w.get_workspace_tabs().row_count() != 4 {
                    return bail("the other connection's query should open in a tab".into());
                }
                if w.get_workspace_running() || w.get_workspace_has_result() {
                    return bail("a query from another connection must not run here".into());
                }
                if !toast_texts(&w).iter().any(|t| t.contains("Elsewhere")) {
                    return bail(format!("expected a toast naming the other connection, got {:?}", toast_texts(&w)));
                }
                h.invoke_open_requested();
                let failed = find_row(h.get_history(), "SELECT * FROM missing").unwrap();
                h.invoke_history_rerun(failed.into());
                if w.get_workspace_tabs().row_count() != 5 || w.get_workspace_has_result() {
                    return bail("a failed query should open without running".into());
                }
                step2.set(5);
            }

            // Snippets: create, insert into the active query tab, persist.
            5 => {
                h.invoke_open_requested();
                h.invoke_tab_selected(1);
                h.invoke_snippet_new();
                if h.get_mode() != PanelMode::SnippetForm {
                    return bail("New snippet should open the form".into());
                }
                // A form with no name does not save.
                h.invoke_form_save();
                if h.get_form_error().is_empty() || h.get_mode() != PanelMode::SnippetForm {
                    return bail("saving a nameless snippet should be refused".into());
                }
                h.set_form_name("Widget names".into());
                h.set_form_description("All the widgets".into());
                h.set_form_tags("reports, widgets".into());
                h.set_form_sql("SELECT name FROM widgets".into());
                h.invoke_form_save();
                if h.get_mode() != PanelMode::Lists || h.get_snippets().row_count() != 1 {
                    return bail(format!("the snippet did not save, mode ok={}", h.get_mode() == PanelMode::Lists));
                }
                let s = h.get_snippets().row_data(0).unwrap();
                if s.title != "Widget names" || s.tags.row_count() != 2 {
                    return bail("the snippet row lost its name or tags".into());
                }
                // Search and tag filtering.
                h.set_snippet_search("nothing like this".into());
                h.invoke_snippet_search_edited("nothing like this".into());
                if h.get_snippets().row_count() != 0 {
                    return bail("a search with no match should list nothing".into());
                }
                h.set_snippet_search("".into());
                h.invoke_snippet_search_edited("".into());
                h.invoke_snippet_tag_picked(1); // "widgets"
                if h.get_snippets().row_count() != 1 {
                    return bail("the tag filter should keep the snippet".into());
                }
                h.invoke_snippet_tag_picked(-1);

                // Insert into the active query tab: a fresh, blank sixth one.
                w.invoke_workspace_new_query();
                h.invoke_snippet_insert(s.id.clone());
                if h.get_visible() || w.get_workspace_tabs().row_count() != 6 {
                    return bail("insert should close the panel and reuse the active tab".into());
                }
                if w.get_workspace_editor_text() != "SELECT name FROM widgets" {
                    return bail(format!("editor text after insert: {:?}", w.get_workspace_editor_text()));
                }
                // Inserting again appends after a blank line.
                h.invoke_open_requested();
                h.invoke_snippet_insert(s.id.clone());
                if w.get_workspace_editor_text() != "SELECT name FROM widgets\n\nSELECT name FROM widgets" {
                    return bail(format!("second insert should append: {:?}", w.get_workspace_editor_text()));
                }
                step2.set(6);
            }
            6 => {
                let snippets = std::fs::read_to_string(dir2.join("snippets.json")).unwrap_or_default();
                if snippets.contains("Widget names") && snippets.contains("reports") {
                    step2.set(7);
                } else if started.elapsed() > Duration::from_secs(25) {
                    bail("snippets.json never received the snippet".into());
                }
            }

            // Templates: create with a typed placeholder, run it, quoting held.
            7 => {
                h.invoke_open_requested();
                h.invoke_tab_selected(2);
                h.invoke_template_new();
                if h.get_mode() != PanelMode::TemplateForm {
                    return bail("New template should open the form".into());
                }
                h.set_form_name("Widgets by name".into());
                h.set_form_category("Reports".into());
                let sql = "SELECT id, name FROM widgets WHERE name = {{name}} AND id >= {{min_id:integer}}";
                h.set_form_sql(sql.into());
                h.invoke_form_sql_edited(sql.into());
                if h.get_form_params().row_count() != 2 {
                    return bail(format!("expected 2 parameters, got {}", h.get_form_params().row_count()));
                }
                let p1 = h.get_form_params().row_data(1).unwrap();
                if p1.name != "min_id" || p1.type_label != "integer" {
                    return bail(format!("second parameter was {} {}", p1.name, p1.type_label));
                }
                h.invoke_form_param_default(1, "0".into());
                h.invoke_form_save();
                if h.get_mode() != PanelMode::Lists || h.get_templates().row_count() != 1 {
                    return bail("the template did not save".into());
                }
                let t = h.get_templates().row_data(0).unwrap();
                if !t.subtitle.contains("2 parameters") {
                    return bail(format!("template subtitle: {:?}", t.subtitle));
                }

                // Run: validation first, then a statement with the quote doubled.
                h.invoke_template_run(t.id.clone());
                if h.get_mode() != PanelMode::TemplateRun || h.get_run_params().row_count() != 2 {
                    return bail("Run should open the parameter form".into());
                }
                if h.get_run_params().row_data(1).unwrap().initial != "0" {
                    return bail("the default value should prefill".into());
                }
                h.invoke_run_edited(1, "abc".into());
                if h.get_run_errors().row_data(1).unwrap().is_empty() {
                    return bail("a non-integer should show an error as you type".into());
                }
                h.invoke_run_submit();
                if w.get_workspace_tabs().row_count() != 6 || !h.get_visible() {
                    return bail("an invalid form must not run".into());
                }
                h.invoke_run_edited(0, "O'Brien".into());
                h.invoke_run_edited(1, "7".into());
                if !h.get_run_preview().contains("'O''Brien'") || !h.get_run_preview().contains(">= 7") {
                    return bail(format!("preview was {:?}", h.get_run_preview()));
                }
                h.invoke_run_submit();
                if h.get_visible() || w.get_workspace_tabs().row_count() != 7 {
                    return bail("a valid run should open a new tab".into());
                }
                if w.get_workspace_editor_text() != "SELECT id, name FROM widgets WHERE name = 'O''Brien' AND id >= 7" {
                    return bail(format!("rendered SQL was {:?}", w.get_workspace_editor_text()));
                }
                step2.set(8);
            }
            8 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                if w.get_workspace_row_count() != 0 {
                    return bail("no widget is called O'Brien".into());
                }
                // Duplicate, then confirm-delete the copy.
                h.invoke_open_requested();
                h.invoke_tab_selected(2);
                let id = h.get_templates().row_data(0).unwrap().id;
                h.invoke_template_duplicate(id);
                if h.get_templates().row_count() != 2 || find_row(h.get_templates(), "Widgets by name copy").is_none() {
                    return bail("duplicate should add a copy".into());
                }
                let copy = find_row(h.get_templates(), "Widgets by name copy").unwrap();
                h.invoke_template_delete(copy.into());
                step2.set(9);
            }
            9 if w.get_show_confirm() => {
                if w.get_confirm_title() != "Delete template?" {
                    return bail(format!("delete should ask first, got {:?}", w.get_confirm_title()));
                }
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                if h.get_templates().row_count() != 1 {
                    return bail("the copy was not deleted".into());
                }
                step2.set(10);
            }
            10 => {
                let templates = std::fs::read_to_string(dir2.join("templates.json")).unwrap_or_default();
                if templates.contains("Widgets by name") && !templates.contains("Widgets by name copy") {
                    step2.set(11);
                } else if started.elapsed() > Duration::from_secs(25) {
                    bail(format!("templates.json did not settle: {templates}"));
                }
            }

            // Clear history behind a confirmation.
            11 => {
                h.invoke_history_clear();
                step2.set(12);
            }
            12 if w.get_show_confirm() => {
                if w.get_confirm_title() != "Clear query history?" {
                    return bail(format!("clear should ask first, got {:?}", w.get_confirm_title()));
                }
                // Cancelling keeps everything.
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                h.invoke_refresh();
                if h.get_history().row_count() == 0 {
                    return bail("cancelling the confirmation cleared history".into());
                }
                h.invoke_history_clear();
                step2.set(13);
            }
            13 if w.get_show_confirm() => {
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                if h.get_history().row_count() != 0 {
                    return bail(format!("history should be empty, has {}", h.get_history().row_count()));
                }
                step2.set(14);
            }
            14 => {
                let history = std::fs::read_to_string(dir2.join("history.json")).unwrap_or_default();
                if !history.contains("DELETE FROM widgets") {
                    step2.set(99);
                    slint::quit_event_loop().unwrap();
                } else if started.elapsed() > Duration::from_secs(25) {
                    bail("history.json still has the cleared entries".into());
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
