//! Update checks end to end: the real window, the real controller and a local
//! stub standing in for the manifest server.
//!
//! Covers what the unit tests in `dbhive_ui_model::update_state` and the stub
//! tests in `dbhive_core::service_updater` cannot: that the automatic check
//! runs by itself and shows the banner for a newer version, that closing the
//! banner hides it while About still says so, that a check asked for in About
//! reports a failure (banner and About line) and can be closed, that an
//! up-to-date answer is said in About and not in the banner, and that the
//! automatic check does not repeat inside its interval.
//!
//! The stub is reached through `DBHIVE_UPDATE_URL`, which also makes the
//! automatic check run at once; without it, a process on a redirected data
//! directory never contacts the real update server. Every store is redirected
//! to a temp directory first.

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbhive_slint::{AppWindow, UpdateBridge};
use slint::{ComponentHandle, Timer, TimerMode};

/// What the stub answers: (status line, body). Changed between steps.
type Reply = Arc<Mutex<(&'static str, &'static str)>>;

fn stub() -> (String, Reply, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
    let reply: Reply = Arc::new(Mutex::new(("200 OK", r#"{"version":"99.0.0","notes":"new"}"#)));
    let hits = Arc::new(AtomicUsize::new(0));
    let (r2, h2) = (reply.clone(), hits.clone());
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut sock) = conn else { break };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            h2.fetch_add(1, Ordering::SeqCst);
            let (status, body) = *r2.lock().unwrap();
            let _ = write!(
                sock,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (url, reply, hits)
}

#[test]
fn the_automatic_and_manual_checks_report_through_the_banner_and_about() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-updates-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);
    let (url, reply, hits) = stub();
    std::env::set_var("DBHIVE_UPDATE_URL", &url);

    let w = dbhive_slint::app::build().unwrap();

    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, reply2, hits2) =
        (w.as_weak(), failure.clone(), step.clone(), reply.clone(), hits.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let w: AppWindow = weak.upgrade().unwrap();
        let u = w.global::<UpdateBridge>();
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

        match step2.get() {
            // -- the automatic check finds a newer version ----------------
            0 if u.get_banner_visible() => {
                expect!(
                    u.get_banner_message() == "Version 99.0.0 is available.",
                    "banner said {:?}",
                    u.get_banner_message()
                );
                expect!(u.get_has_update(), "a newer version should offer the release page");
                expect!(hits2.load(Ordering::SeqCst) == 1, "one request so far, got {}", hits2.load(Ordering::SeqCst));
                u.invoke_dismiss();
                step2.set(1);
            }
            1 => {
                expect!(!u.get_banner_visible(), "dismissing should hide the banner");
                expect!(
                    u.get_about_status() == "Version 99.0.0 is available.",
                    "About said {:?}",
                    u.get_about_status()
                );
                // The server goes down; the person asks for a check.
                *reply2.lock().unwrap() = ("500 Internal Server Error", "oops");
                u.invoke_check();
                step2.set(2);
            }

            // -- a check the person asked for says when it fails -----------
            2 if !u.get_checking() && u.get_about_status().starts_with("Could not check") => {
                expect!(u.get_banner_visible(), "a failed manual check should show a banner");
                expect!(
                    u.get_banner_message().contains("500"),
                    "banner said {:?}",
                    u.get_banner_message()
                );
                expect!(!u.get_has_update(), "no release page offered after a failed check");
                u.invoke_dismiss();
                step2.set(3);
            }
            3 => {
                expect!(!u.get_banner_visible(), "the failure banner should close");
                // Now the server answers with an older version: up to date.
                *reply2.lock().unwrap() = ("200 OK", r#"{"version":"0.0.1"}"#);
                u.invoke_check();
                step2.set(4);
            }
            4 if !u.get_checking() && u.get_about_status().starts_with("You are up to date") => {
                expect!(!u.get_banner_visible(), "up to date is not worth a banner");
                expect!(
                    u.get_about_status().contains(env!("CARGO_PKG_VERSION")),
                    "About said {:?}",
                    u.get_about_status()
                );
                step2.set(5);
            }
            // Two manual checks plus the one automatic check; the automatic
            // one must not repeat inside its 24 hour interval. Give the
            // 200 ms tick a few turns to wrongly repeat it.
            5 if started.elapsed() > Duration::from_millis(2000) => {
                let n = hits2.load(Ordering::SeqCst);
                expect!(n == 3, "expected 3 requests (1 automatic, 2 manual), got {n}");
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
