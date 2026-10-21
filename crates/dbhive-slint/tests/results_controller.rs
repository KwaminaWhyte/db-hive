//! The results area's extras driven end to end: the real window, the real
//! controller, a real SQLite file.
//!
//! Covers what `dbhive_ui_model::result_export`'s unit tests cannot: that the
//! Table / JSON / Text switch builds its lines from the rows the grid shows,
//! that the 2,000 row cap and its notice appear, that Export writes exactly
//! the bytes expected (nasty values included) for a query tab, a filtered
//! grid, an empty result and a table tab's page, that an existing file is
//! flagged and replaced, that a bad path writes nothing, and that a failed
//! query becomes a structured error that Retry re-runs.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, ResultsState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

const EXPECTED_CSV: &str = "id,name,note\n\
1,plain,ok\n\
2,\"a,b\",comma\n\
3,\"say \"\"hi\"\"\",\n\
4,\"line1\nline2\",multi\n\
5,=1+1,\"  padded  \"\n\
6,héllo 世界 🐝,\n";

fn settled(w: &AppWindow) -> bool {
    !w.get_workspace_running()
}

#[test]
fn view_modes_error_state_and_export_on_a_real_database() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-results-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, note TEXT);").unwrap();
        let rows: [(&str, Option<&str>); 6] = [
            ("plain", Some("ok")),
            ("a,b", Some("comma")),
            ("say \"hi\"", None),
            ("line1\nline2", Some("multi")),
            ("=1+1", Some("  padded  ")),
            ("héllo 世界 🐝", Some("")),
        ];
        for (name, note) in rows {
            conn.execute("INSERT INTO items (name, note) VALUES (?1, ?2)", rusqlite::params![name, note]).unwrap();
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

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, dir2) = (w.as_weak(), failure.clone(), step.clone(), dir.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let g = w.global::<ResultsState>();
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
        let out = |name: &str| dir2.join(name).to_string_lossy().to_string();
        let read = |name: &str| std::fs::read_to_string(dir2.join(name)).unwrap_or_default();
        let run_sql = |sql: &str| {
            w.invoke_workspace_editor_edited(sql.into());
            w.invoke_workspace_run_query();
        };
        let set_path = |p: &str| {
            g.set_export_path(p.into());
            g.invoke_export_path_edited(p.into());
        };

        match step2.get() {
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 1 => {
                run_sql("SELECT id, name, note FROM items ORDER BY id");
                step2.set(2);
            }

            // -- Table mode, then JSON ----------------------------------------
            2 if settled(&w) && w.get_workspace_has_result() && w.get_workspace_row_count() == 6 => {
                expect!(g.get_can_export(), "a result with columns can be exported");
                expect!(g.get_mode() == 0, "a new result starts in Table mode, got {}", g.get_mode());
                expect!(g.get_view_lines().row_count() == 0, "Table mode builds no lines");
                g.invoke_mode_picked(1);
                step2.set(3);
            }
            3 if g.get_mode() == 1 => {
                // [ + 6 objects of (open, 3 keys, close) + ].
                let lines = g.get_view_lines();
                expect!(lines.row_count() == 32, "JSON view should have 32 lines, got {}", lines.row_count());
                expect!(g.get_view_notice().is_empty(), "6 rows are not truncated");
                let line = lines.row_data(3).unwrap();
                let runs: Vec<(String, i32)> = line.runs.iter().map(|r| (r.text.to_string(), r.kind)).collect();
                expect!(runs.contains(&("\"name\"".into(), 1)), "the key should be coloured as a key: {runs:?}");
                expect!(runs.contains(&("\"plain\"".into(), 2)), "the value should be coloured as a string: {runs:?}");
                g.invoke_mode_picked(3);
                step2.set(30);
            }

            // -- Chart: defaults, kind, X column -----------------------------------
            30 if g.get_mode() == 3 && !g.get_chart_commands().is_empty() => {
                // id is the only numeric column; name is the first text one.
                expect!(g.get_chart_x() == "name" && g.get_chart_y() == "id", "defaults were {} / {}", g.get_chart_x(), g.get_chart_y());
                expect!(g.get_chart_caption() == "Sum of id by name, 6 groups", "caption was {:?}", g.get_chart_caption());
                expect!(g.get_chart_commands().matches('Z').count() == 6, "one bar per group: {}", g.get_chart_commands());
                expect!(g.get_chart_y_options().row_count() == 1, "only id can be summed");
                expect!(g.get_chart_x_options().row_count() == 3, "any column can be grouped by");
                expect!(g.get_chart_message().is_empty(), "there is something to draw");
                g.invoke_chart_kind_picked(1);
                step2.set(31);
            }
            31 if g.get_chart_kind() == 1 && !g.get_chart_area().is_empty() => {
                expect!(g.get_chart_commands().starts_with("M ") && !g.get_chart_commands().contains('Z'), "a line, not bars: {}", g.get_chart_commands());
                g.invoke_chart_x_picked(2);
                step2.set(32);
            }
            32 if g.get_chart_x() == "note" => {
                expect!(g.get_chart_caption() == "Sum of id by note, 6 groups", "caption was {:?}", g.get_chart_caption());
                g.invoke_mode_picked(2);
                step2.set(4);
            }

            // -- Text mode, then open the export dialog ----------------------------
            4 if g.get_mode() == 2 => {
                let lines = g.get_view_lines();
                expect!(lines.row_count() == 7, "Text view is a header plus 6 rows, got {}", lines.row_count());
                let header = lines.row_data(0).unwrap().runs.row_data(0).unwrap();
                expect!(header.text.starts_with("id"), "header line was {:?}", header.text);
                g.invoke_export_open();
                step2.set(5);
            }
            5 if g.get_export_visible() => {
                expect!(
                    g.get_export_scope_note() == "All 6 rows of this result, 3 columns.",
                    "scope note was {:?}",
                    g.get_export_scope_note()
                );
                let p = g.get_export_path().to_string();
                expect!(
                    p.ends_with(".csv") && p.contains(&format!("query_results_{today}")),
                    "the default path should be dated and end in .csv, got {p}"
                );
                expect!(!g.get_export_replaces(), "a new name does not replace anything");
                set_path(&out("results.csv"));
                g.invoke_export_run();
                step2.set(6);
            }

            // -- CSV bytes ---------------------------------------------------------
            6 if g.get_export_done() => {
                expect!(
                    g.get_export_done_message().starts_with("Exported 6 rows to "),
                    "done message was {:?}",
                    g.get_export_done_message()
                );
                let got = read("results.csv");
                expect!(got == EXPECTED_CSV, "CSV differs.\nexpected: {EXPECTED_CSV:?}\n     got: {got:?}");
                g.invoke_export_close();
                g.invoke_export_open();
                step2.set(7);
            }

            // -- JSON bytes: an array of objects in column order ------------------------
            7 if g.get_export_visible() && !g.get_export_done() => {
                g.invoke_export_format_picked(1);
                expect!(
                    g.get_export_format() == 1 && g.get_export_path().ends_with(".json"),
                    "picking JSON should retarget the extension, path {:?}",
                    g.get_export_path()
                );
                set_path(&out("results.json"));
                g.invoke_export_run();
                step2.set(8);
            }
            8 if g.get_export_done() => {
                let text = read("results.json");
                let parsed: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => return bail(format!("export is not valid JSON: {e}\n{text}")),
                };
                expect!(parsed.as_array().map(|a| a.len()) == Some(6), "6 objects expected: {text}");
                expect!(parsed[2]["name"] == "say \"hi\"" && parsed[2]["note"].is_null(), "quote and NULL: {text}");
                expect!(parsed[3]["name"] == "line1\nline2", "newline survives: {text}");
                expect!(parsed[4]["name"] == "=1+1", "formula text is untouched: {text}");
                expect!(parsed[5]["name"] == "héllo 世界 🐝" && parsed[5]["note"] == "", "unicode and empty string: {text}");
                let (a, b, c) = (text.find("\"id\"").unwrap(), text.find("\"name\"").unwrap(), text.find("\"note\"").unwrap());
                expect!(a < b && b < c, "keys must keep the column order");
                g.invoke_export_close();
                // A filter narrows what the grid shows, and so what is exported.
                w.invoke_workspace_filter_changed("say".into());
                step2.set(9);
            }
            9 if !g.get_export_visible() && w.get_workspace_visible_row_count() == 1 => {
                g.invoke_export_open();
                step2.set(10);
            }
            10 if g.get_export_visible() => {
                let note = g.get_export_scope_note().to_string();
                expect!(
                    note.contains("after your filter") && note.contains("1 row") && note.contains("of 6"),
                    "scope note should describe the filter: {note:?}"
                );
                g.invoke_export_format_picked(0);
                set_path(&out("filtered.csv"));
                g.invoke_export_run();
                step2.set(11);
            }
            11 if g.get_export_done() => {
                let got = read("filtered.csv");
                expect!(got == "id,name,note\n3,\"say \"\"hi\"\"\",\n", "filtered CSV was {got:?}");
                g.invoke_export_close();
                w.invoke_workspace_filter_changed("".into());
                step2.set(12);
            }

            // -- An existing file is flagged, then replaced --------------------------------
            12 if !g.get_export_visible() && w.get_workspace_visible_row_count() == 6 => {
                std::fs::write(dir2.join("results.json"), "OLD").unwrap();
                g.invoke_export_open();
                step2.set(13);
            }
            13 if g.get_export_visible() => {
                g.invoke_export_format_picked(1);
                set_path(&out("results.json"));
                expect!(g.get_export_replaces(), "an existing file should be flagged before anything is written");
                g.invoke_export_run();
                step2.set(14);
            }
            14 if g.get_export_done() => {
                let text = read("results.json");
                expect!(text != "OLD" && text.starts_with("[\n"), "the old file should be replaced, got {text:?}");
                g.invoke_export_close();
                g.invoke_export_open();
                step2.set(15);
            }

            // -- A bad path writes nothing and says why -----------------------------------------
            15 if g.get_export_visible() && !g.get_export_done() => {
                set_path(&out("missing-folder/x.json"));
                g.invoke_export_run();
                expect!(
                    g.get_export_path_error().contains("folder does not exist"),
                    "path error was {:?}",
                    g.get_export_path_error()
                );
                expect!(!g.get_export_done() && !g.get_export_running(), "a refused export is neither done nor running");
                expect!(!dir2.join("missing-folder").exists(), "nothing may be created");
                set_path(&out("wrong-extension.csv"));
                g.invoke_export_run();
                expect!(
                    g.get_export_path_error().contains(".json"),
                    "a .csv name under JSON should be refused, got {:?}",
                    g.get_export_path_error()
                );
                g.invoke_export_close();
                run_sql("SELEC 1");
                step2.set(16);
            }

            // -- A structured error, Retry re-runs it ----------------------------------------------
            16 if settled(&w) && !w.get_workspace_rows_error().is_empty() => {
                expect!(g.get_error_headline() == "Syntax error", "headline was {:?}", g.get_error_headline());
                expect!(!g.get_error_detail().is_empty(), "the driver's message is the detail");
                expect!(g.get_error_query() == "SELEC 1", "the failed query is shown, got {:?}", g.get_error_query());
                expect!(!g.get_error_query_open(), "the query starts folded away");
                expect!(!g.get_can_export() && g.get_mode() == 0, "an error has nothing to export or view");
                g.invoke_error_toggle_query();
                expect!(g.get_error_query_open(), "the disclosure should open");
                g.invoke_error_retry();
                step2.set(17);
            }
            17 if settled(&w) && !w.get_workspace_rows_error().is_empty() && !g.get_error_query_open() => {
                // Retry ran again (a new failure folds the query away); fix the SQL next.
                run_sql("SELECT id FROM items WHERE id < 0");
                step2.set(18);
            }

            // -- An empty result still exports its header ----------------------------------------------
            18 if settled(&w) && w.get_workspace_has_result() && w.get_workspace_row_count() == 0 => {
                expect!(g.get_error_headline().is_empty(), "the error clears once a query succeeds");
                expect!(g.get_can_export(), "an empty result with columns can be exported");
                g.invoke_export_open();
                step2.set(19);
            }
            19 if g.get_export_visible() => {
                g.invoke_export_format_picked(0);
                set_path(&out("empty.csv"));
                g.invoke_export_run();
                step2.set(20);
            }
            20 if g.get_export_done() => {
                expect!(read("empty.csv") == "id\n", "empty export should be just the header, got {:?}", read("empty.csv"));
                g.invoke_export_close();
                run_sql("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2500) SELECT i FROM n");
                step2.set(21);
            }

            // -- The 2,000 row cap, and Export writes all of them ------------------------------------------
            21 if settled(&w) && w.get_workspace_row_count() == 2500 => {
                g.invoke_mode_picked(1);
                step2.set(22);
            }
            22 if g.get_mode() == 1 && g.get_view_lines().row_count() > 0 => {
                expect!(
                    g.get_view_lines().row_count() == 2000 * 3 + 2,
                    "2,000 rows of 3 lines plus brackets, got {}",
                    g.get_view_lines().row_count()
                );
                expect!(
                    g.get_view_notice() == "Showing the first 2,000 of 2,500 rows. Export writes all of them.",
                    "notice was {:?}",
                    g.get_view_notice()
                );
                g.invoke_export_open();
                step2.set(23);
            }
            23 if g.get_export_visible() => {
                g.invoke_export_format_picked(0);
                set_path(&out("big.csv"));
                g.invoke_export_run();
                step2.set(24);
            }
            24 if g.get_export_done() => {
                let big = read("big.csv");
                expect!(big.lines().count() == 2501, "all 2,500 rows plus a header, got {} lines", big.lines().count());
                expect!(big.ends_with("\n2500\n"), "the last row must be exported, not just the first 2,000");
                g.invoke_export_close();
                w.invoke_workspace_select_table("items".into());
                step2.set(25);
            }

            // -- A table tab exports its page, and view mode is per tab ----------------------------------------
            25 if !w.get_workspace_active_is_query() && settled(&w) && w.get_workspace_has_result() && g.get_can_export() => {
                expect!(g.get_mode() == 0, "a table tab has no view modes");
                g.invoke_export_open();
                step2.set(26);
            }
            26 if g.get_export_visible() => {
                let note = g.get_export_scope_note().to_string();
                expect!(note.starts_with("This page only: 6 rows of the table"), "scope note was {note:?}");
                expect!(g.get_export_path().contains("items_"), "default name uses the table: {}", g.get_export_path());
                g.invoke_export_format_picked(0);
                set_path(&out("page.csv"));
                g.invoke_export_run();
                step2.set(27);
            }
            27 if g.get_export_done() => {
                let got = read("page.csv");
                expect!(got == EXPECTED_CSV, "the page export should match the table's rows, got {got:?}");
                g.invoke_export_close();
                w.invoke_workspace_tab_selected(0);
                step2.set(28);
            }
            28 if w.get_workspace_active_is_query() => {
                expect!(g.get_mode() == 1, "the query tab should still be in JSON mode, got {}", g.get_mode());
                run_sql("SELECT name FROM items");
                step2.set(40);
            }

            // -- A result with no number cannot be charted, and says so ----------------
            40 if settled(&w) && w.get_workspace_row_count() == 6 && w.get_workspace_has_result() => {
                g.invoke_mode_picked(3);
                step2.set(41);
            }
            41 if g.get_mode() == 3 => {
                expect!(g.get_chart_message().contains("no numeric column"), "message was {:?}", g.get_chart_message());
                expect!(g.get_chart_commands().is_empty(), "nothing is drawn without a number");
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
