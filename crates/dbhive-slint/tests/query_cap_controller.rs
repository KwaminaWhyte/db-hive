//! Settings > Max rows on the query run path, end to end: the real window,
//! the real controller, a real SQLite file.
//!
//! A query that returns more rows than the setting allows (default 1,000) is
//! cut at the limit and the summary says so; a query within the limit is shown
//! whole with the plain row count.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own saved connections or settings.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

#[test]
fn a_result_over_max_rows_is_cut_at_the_limit_and_labelled() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("empty.sqlite");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY);")
        .unwrap();
    {
        let mut profile = ConnectionProfile::new(
            "cap-db".to_string(),
            "Cap".to_string(),
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
        let rows = || w.get_workspace_rows().row_count();

        match step2.get() {
            0 => {
                w.invoke_connect("cap-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 1 => {
                w.invoke_workspace_editor_edited(
                    "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1500) SELECT x FROM c"
                        .into(),
                );
                w.invoke_workspace_run_query();
                step2.set(2);
            }
            2 if !w.get_workspace_running() && w.get_workspace_has_result() => {
                expect!(rows() == 1000, "1,500 rows should be cut to 1,000, got {}", rows());
                let summary = w.get_workspace_result_summary().to_string();
                expect!(summary.starts_with("First 1,000 rows"), "summary was {summary:?}");
                expect!(summary.contains("Max rows"), "it should say where the limit is: {summary:?}");

                w.invoke_workspace_editor_edited(
                    "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 7) SELECT x FROM c".into(),
                );
                w.invoke_workspace_run_query();
                step2.set(3);
            }
            3 if !w.get_workspace_running() && w.get_workspace_has_result() && rows() == 7 => {
                let summary = w.get_workspace_result_summary().to_string();
                expect!(summary.starts_with("7 rows"), "a result within the limit is plain: {summary:?}");
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
