//! The Connection Lost overlay, driven through the real window and controller.
//!
//! A real connection is needed that can then die mid-session. The test runs a
//! tiny Redis-protocol server on a local port: it answers every command with
//! `+OK`, except `GET die`, where it drops the socket without replying, which
//! is exactly what a server that went away looks like to the driver.
//!
//! * A query that dies with the socket shows the overlay over the workspace,
//!   with the driver's error. An ordinary query error does not.
//! * Reconnect opens a fresh connection and leaves the workspace in place.
//! * Go to connections closes it and returns to the list.

use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{ConnectionLostState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

/// Read one RESP command (an array of bulk strings). `None` at EOF.
fn read_command(r: &mut BufReader<TcpStream>) -> Option<Vec<String>> {
    let mut line = String::new();
    if r.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let line = line.trim_end();
    let Some(n) = line.strip_prefix('*') else {
        // Inline command.
        return Some(line.split_whitespace().map(str::to_string).collect());
    };
    let n: usize = n.parse().ok()?;
    let mut args = Vec::with_capacity(n);
    for _ in 0..n {
        let mut len = String::new();
        r.read_line(&mut len).ok()?;
        let len: usize = len.trim_end().strip_prefix('$')?.parse().ok()?;
        let mut buf = vec![0u8; len + 2];
        std::io::Read::read_exact(r, &mut buf).ok()?;
        buf.truncate(len);
        args.push(String::from_utf8_lossy(&buf).into_owned());
    }
    Some(args)
}

fn serve(stream: TcpStream) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    while let Some(cmd) = read_command(&mut reader) {
        let name = cmd.first().map(|c| c.to_uppercase()).unwrap_or_default();
        if name == "GET" && cmd.get(1).map(String::as_str) == Some("die") {
            // Gone, mid-request.
            let _ = writer.shutdown(std::net::Shutdown::Both);
            return;
        }
        let reply: &[u8] = if name == "GET" && cmd.get(1).map(String::as_str) == Some("wrongtype") {
            b"-WRONGTYPE Operation against a key holding the wrong kind of value\r\n"
        } else {
            b"+OK\r\n"
        };
        if writer.write_all(reply).is_err() {
            return;
        }
    }
}

fn fake_redis() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
    port
}

#[test]
fn a_dead_connection_offers_reconnect_and_a_way_out() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-lost-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let port = fake_redis();
    {
        let mut state = AppState::new();
        state.add_profile(ConnectionProfile::new(
            "r".to_string(),
            "Cache".to_string(),
            DbDriver::Redis,
            "127.0.0.1".to_string(),
            port,
            String::new(),
        ));
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
        let lost = w.global::<ConnectionLostState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(30) {
            bail(format!(
                "timed out at step {} (error {:?}, rows error {:?})",
                step2.get(),
                w.get_connection_error(),
                w.get_workspace_rows_error()
            ));
            return;
        }
        let idle = w.get_workspace_tabs().row_count() > 0 && !w.get_workspace_running();
        match step2.get() {
            0 => {
                w.invoke_connect("r".into());
                step2.set(1);
            }
            // An ordinary error is the query's, not the connection's.
            1 if w.get_screen() == Screen::Workspace && idle => {
                w.invoke_workspace_editor_edited("GET wrongtype".into());
                w.invoke_workspace_run_query();
                step2.set(2);
            }
            2 if idle && !w.get_workspace_rows_error().is_empty() => {
                if lost.get_visible() {
                    return bail(format!(
                        "a query error showed Connection Lost: {:?}",
                        w.get_workspace_rows_error()
                    ));
                }
                w.invoke_workspace_editor_edited("GET die".into());
                w.invoke_workspace_run_query();
                step2.set(3);
            }
            3 if lost.get_visible() => {
                if lost.get_title() != "Connection to Cache lost" || lost.get_error().is_empty() {
                    return bail(format!(
                        "overlay said {:?} / {:?}",
                        lost.get_title(),
                        lost.get_error()
                    ));
                }
                lost.invoke_reconnect();
                step2.set(4);
            }
            // Reconnected: the overlay goes, the workspace and its tab stay.
            4 if !lost.get_visible() => {
                if w.get_screen() != Screen::Workspace {
                    return bail("reconnect should keep the workspace".into());
                }
                if w.get_workspace_editor_text() != "GET die" {
                    return bail("the tab's SQL should survive a reconnect".into());
                }
                if !w
                    .get_toasts()
                    .iter()
                    .any(|t| t.text.starts_with("Reconnected to Cache"))
                {
                    return bail("no reconnected toast".into());
                }
                step2.set(5);
            }
            // The new connection works; kill it again and leave.
            5 if idle => {
                w.invoke_workspace_run_query();
                step2.set(6);
            }
            6 if lost.get_visible() && !lost.get_reconnecting() => {
                lost.invoke_go_connections();
                step2.set(7);
            }
            7 if w.get_screen() == Screen::Connections => {
                if lost.get_visible() {
                    return bail("the overlay should close".into());
                }
                let still_connected = w.get_connections().iter().any(|c| c.connected);
                if still_connected {
                    return bail("the dead connection should not show as connected".into());
                }
                step2.set(8);
                slint::quit_event_loop().unwrap();
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
    assert_eq!(step.get(), 8);
}
