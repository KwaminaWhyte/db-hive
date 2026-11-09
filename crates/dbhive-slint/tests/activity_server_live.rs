//! The Activity Monitor's server tab against a real PostgreSQL.
//!
//! Ignored by default: it needs a PostgreSQL on localhost:5432 that accepts
//! the current OS user without a password, and a `psql` on the PATH.
//!
//!     cargo test -p dbhive-slint --test activity_server_live -- --ignored
//!
//! What it proves that the SQLite scenario cannot: live metrics arrive and
//! feed the chart, the process list shows another session's running
//! statement, cancelling it by pid (behind a confirmation) really stops it,
//! scheduled polling happens while the panel is open, and stops while it is
//! closed.

use std::cell::{Cell, RefCell};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{ActivityState, MonitorServerState};
use slint::{ComponentHandle, Model, Timer, TimerMode};

#[test]
#[ignore = "needs a local PostgreSQL on localhost:5432 (trust auth) and psql"]
fn live_metrics_process_list_cancel_and_polling_against_postgres() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let user = std::env::var("USER").unwrap_or_else(|_| "postgres".into());
    {
        let mut p = ConnectionProfile::new(
            "pg".to_string(),
            "Local PG".to_string(),
            DbDriver::Postgres,
            "localhost".to_string(),
            5432,
            user.clone(),
        );
        p.database = Some("postgres".to_string());
        let mut state = AppState::new();
        state.add_profile(p);
        state.save_profiles_to_store().unwrap();
    }

    // This server trusts local connections and needs no password; say one is
    // stored so the shell connects instead of opening the password prompt.
    let secrets = std::sync::Arc::new(dbhive_slint::app::MemorySecrets::default());
    secrets.stored.lock().unwrap().insert("pg".to_string());
    dbhive_slint::app::use_secret_store(secrets);

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let sleeper: Rc<RefCell<Option<Child>>> = Rc::new(RefCell::new(None));
    let label_when_closed = Rc::new(RefCell::new(String::new()));
    let closed_at = Rc::new(Cell::new(None::<Instant>));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, sleeper2, label2, closed2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        sleeper.clone(),
        label_when_closed.clone(),
        closed_at.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let w = weak.upgrade().unwrap();
        let a = w.global::<ActivityState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(60) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            0 => {
                w.invoke_connect("pg".into());
                step2.set(1);
            }
            1 if w.get_workspace_connection_name() == "Local PG" => {
                // Another session, running a statement we can find and cancel.
                let child = Command::new("psql")
                    .args([
                        "-h",
                        "localhost",
                        "-d",
                        "postgres",
                        "-c",
                        "SELECT pg_sleep(120) /* dbhive-live-test */",
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("psql must be on the PATH");
                *sleeper2.borrow_mut() = Some(child);
                // Poll interval 5s, auto-refresh on, then open the Server tab.
                a.invoke_interval_selected(1);
                a.invoke_auto_toggled();
                w.invoke_run_command("go-activity-monitor".into());
                a.invoke_tab_selected(2);
                step2.set(2);
            }
            // Metrics and the process list arrive.
            2 if a.get_server_state() == MonitorServerState::Ready
                && a.get_process_rows()
                    .iter()
                    .any(|r| r.cells.iter().any(|c| c.text.contains("dbhive-live-test"))) =>
            {
                let tiles: Vec<String> = a
                    .get_live_tiles()
                    .iter()
                    .map(|t| format!("{}={}", t.label, t.value))
                    .collect();
                if tiles[0].ends_with("=-") || tiles[1].ends_with("=-") {
                    return bail(format!("live tiles are empty: {tiles:?}"));
                }
                // Select the sleeper and ask to cancel it.
                let row = a
                    .get_process_rows()
                    .iter()
                    .position(|r| r.cells.iter().any(|c| c.text.contains("dbhive-live-test")))
                    .unwrap();
                a.invoke_process_clicked(row as i32);
                if !a.get_can_cancel() || !a.get_cancel_label().starts_with("Cancel query ") {
                    return bail(format!("cancel not offered: {:?}", a.get_cancel_label()));
                }
                a.invoke_cancel_requested();
                if !w.get_show_confirm() || w.get_confirm_title() != "Cancel this query?" {
                    return bail(format!(
                        "no cancel confirmation: {:?}",
                        w.get_confirm_title()
                    ));
                }
                // Declining leaves it running.
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                std::thread::sleep(Duration::from_millis(300));
                if sleeper2
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .unwrap()
                    .is_some()
                {
                    return bail("declining the confirmation still stopped the query".into());
                }
                a.invoke_cancel_requested();
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(3);
            }
            // The cancelled session's psql exits with an error.
            3 => {
                let mut guard = sleeper2.borrow_mut();
                let child = guard.as_mut().unwrap();
                if let Some(status) = child.try_wait().unwrap() {
                    if status.success() {
                        return bail(
                            "the sleeping query finished instead of being cancelled".into(),
                        );
                    }
                    drop(guard);
                    step2.set(4);
                }
            }
            // Scheduled polling: with a 5s interval the chart gets a second
            // sample on its own.
            4 if !a.get_sessions_chart().lines.iter().next().is_none() => {
                if a.get_updated_label().is_empty() {
                    return bail("no 'Updated' label after polling".into());
                }
                // Close the panel and note when: polling must stop.
                a.invoke_close();
                *label2.borrow_mut() = a.get_updated_label().to_string();
                closed2.set(Some(Instant::now()));
                step2.set(5);
            }
            5 => {
                let waited = closed2.get().unwrap().elapsed();
                // Longer than the interval plus a tick, with margin.
                if waited > Duration::from_secs(8) {
                    // Reopening polls again at once (the gate was reset).
                    w.invoke_run_command("go-activity-monitor".into());
                    step2.set(6);
                } else if a.get_updated_label() != label2.borrow().as_str() {
                    return bail(format!(
                        "polling continued while the panel was closed: {:?} -> {:?}",
                        label2.borrow(),
                        a.get_updated_label()
                    ));
                }
            }
            6 if a.get_updated_label() != label_when_closed.borrow().as_str() => {
                step2.set(99);
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });

    w.run().unwrap();
    drop(timer);
    if let Some(mut c) = sleeper.borrow_mut().take() {
        let _ = c.kill();
    }
    let _ = std::fs::remove_dir_all(&dir);

    if let Some(msg) = failure.borrow().as_ref() {
        panic!("{msg}");
    }
    assert_eq!(step.get(), 99, "the scenario did not run to its end");
}
