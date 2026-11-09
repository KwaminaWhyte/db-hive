//! The password prompt, driven through the real window and controller.
//!
//! A saved Postgres profile with no stored password must open the prompt
//! instead of connecting with an empty password. Cancel goes back to the list
//! without trying. Submit tries with what was typed, and the typed password
//! never shows up in the error banner or a toast. A profile whose password is
//! in the (in-memory) keychain connects without asking, and a SQLite profile
//! never asks at all.
//!
//! The Postgres server is a closed local port, so every attempt fails fast;
//! what matters here is which path the controller takes. The keychain is an
//! in-memory store, so the developer's own keychain is never read.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::app::MemorySecrets;
use dbhive_slint::{PasswordPromptState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

const TYPED: &str = "pr0mpt-s3cret";

fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

fn toasts_text(w: &dbhive_slint::AppWindow) -> String {
    w.get_toasts()
        .iter()
        .map(|t| t.text.to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn prompt_when_no_password_is_stored_and_never_echo_it() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-pwprompt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("local.sqlite");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute("CREATE TABLE t (id INTEGER)", [])
        .unwrap();

    {
        let mut state = AppState::new();
        let pg = ConnectionProfile::new(
            "pg".to_string(),
            "Remote PG".to_string(),
            DbDriver::Postgres,
            "127.0.0.1".to_string(),
            closed_port(),
            "app".to_string(),
        );
        state.add_profile(pg);
        let mut lite = ConnectionProfile::new(
            "lite".to_string(),
            "Local file".to_string(),
            DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        lite.database = Some(db.to_string_lossy().to_string());
        state.add_profile(lite);
        state.save_profiles_to_store().unwrap();
    }

    let secrets = Arc::new(MemorySecrets::default());
    dbhive_slint::app::use_secret_store(secrets.clone());

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, secrets2) = (w.as_weak(), failure.clone(), step.clone(), secrets);
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let p = w.global::<PasswordPromptState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(30) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            0 => {
                w.invoke_connect("pg".into());
                step2.set(1);
            }
            // No stored password: the prompt, not a connection attempt.
            1 if p.get_visible() => {
                if p.get_title() != "Password for Remote PG" {
                    return bail(format!("title was {:?}", p.get_title()));
                }
                if !p.get_message().contains("for app") || !p.get_remember() {
                    return bail(format!(
                        "message {:?}, remember {}",
                        p.get_message(),
                        p.get_remember()
                    ));
                }
                p.invoke_cancel();
                step2.set(2);
            }
            2 => {
                if p.get_visible() {
                    return bail("cancel should close the prompt".into());
                }
                if !w.get_connection_error().is_empty() {
                    return bail("cancel should not have tried to connect".into());
                }
                // Again, and this time type a password, untick Remember and
                // submit.
                w.invoke_connect("pg".into());
                step2.set(3);
            }
            3 if p.get_visible() => {
                if !p.get_password().is_empty() {
                    return bail("a reopened prompt must start empty".into());
                }
                p.set_password(TYPED.into());
                p.invoke_password_edited(TYPED.into());
                p.invoke_toggle_remember();
                if p.get_remember() {
                    return bail("Remember should now be off".into());
                }
                p.invoke_submit();
                step2.set(4);
            }
            4 if !w.get_connection_error().is_empty() => {
                let err = w.get_connection_error().to_string();
                if p.get_visible() {
                    return bail("the prompt should close on submit".into());
                }
                if !p.get_password().is_empty() {
                    return bail("the typed password stayed in the window".into());
                }
                if err.contains(TYPED) || toasts_text(&w).contains(TYPED) {
                    return bail("the password leaked into a message".into());
                }
                if !secrets2.forgotten.lock().unwrap().is_empty() {
                    return bail("a failed connect has nothing to forget".into());
                }
                // Now the keychain "has" it: connect without asking.
                secrets2.stored.lock().unwrap().insert("pg".to_string());
                w.invoke_dismiss_connection_error();
                w.invoke_connect("pg".into());
                step2.set(5);
            }
            5 => {
                if p.get_visible() {
                    return bail("a stored password must not prompt".into());
                }
                if !w.get_connection_error().is_empty() {
                    // The attempt was made (and failed on the closed port).
                    w.invoke_dismiss_connection_error();
                    w.invoke_connect("lite".into());
                    step2.set(6);
                }
            }
            // SQLite has no password: straight into the workspace.
            6 => {
                if p.get_visible() {
                    return bail("SQLite must never prompt".into());
                }
                if w.get_screen() == Screen::Workspace {
                    step2.set(7);
                    slint::quit_event_loop().unwrap();
                }
            }
            _ => {}
        }
    });

    slint::run_event_loop().unwrap();
    drop(timer);
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(msg) = failure.borrow().clone() {
        panic!("{msg}");
    }
    assert_eq!(step.get(), 7);
}
