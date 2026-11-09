//! A shortcut rebound in Settings dispatches on its new key, stops firing on
//! the old one, and shows its new keys in the help and the menu.
//!
//! Its own test binary because the testing backend's platform can only be
//! initialised once per process, and the palette test owns that one.
//!
//! Every store is redirected to a temp directory, so this never touches the
//! developer's own settings or connections.

use std::cell::Cell;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use dbhive_slint::AppWindow;
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, SharedString, Timer, TimerMode};

async fn sleep(d: Duration) {
    let fired = Rc::new(Cell::new(false));
    let timer = Timer::default();
    let mut started = false;
    poll_fn(move |cx| {
        if fired.get() {
            return Poll::Ready(());
        }
        if !started {
            started = true;
            let (flag, waker) = (fired.clone(), cx.waker().clone());
            timer.start(TimerMode::SingleShot, d, move || {
                flag.set(true);
                waker.wake_by_ref();
            });
        }
        Poll::Pending
    })
    .await;
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    let started = Instant::now();
    while !cond() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "timed out waiting for: {what}"
        );
        sleep(Duration::from_millis(10)).await;
    }
}

async fn settle() {
    for _ in 0..5 {
        sleep(Duration::from_millis(10)).await;
    }
}

fn key(w: &AppWindow, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    w.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    w.window().dispatch_event(WindowEvent::KeyReleased { text });
}

/// A chord with the command modifier held (Ctrl; the controller accepts Cmd
/// too, which is what a Mac reports).
fn cmd(w: &AppWindow, ch: &str) {
    let ctrl: SharedString = Key::Control.into();
    w.window()
        .dispatch_event(WindowEvent::KeyPressed { text: ctrl.clone() });
    key(w, ch);
    w.window()
        .dispatch_event(WindowEvent::KeyReleased { text: ctrl });
}

fn press(w: &AppWindow, k: Key) {
    key(w, k);
}
async fn scenario(w: AppWindow, settings_raised: Rc<Cell<u32>>, finished: Rc<Cell<bool>>) {
    // Open Settings moved to Cmd+J: the new key fires, the old one does not.
    cmd(&w, "j");
    until("Cmd+J opened Settings after the rebind", || {
        settings_raised.get() == 1
    })
    .await;
    cmd(&w, ",");
    settle().await;
    assert_eq!(
        settings_raised.get(),
        1,
        "Cmd+, still fired after the rebind moved Open Settings off it"
    );

    // The menu's shortcut follows the rebind.
    let expected = slint::Keys::from_parts(["Control", "j"]).unwrap();
    assert_eq!(
        w.global::<dbhive_slint::ShellState>().get_key_settings(),
        expected,
        "the File > Settings shortcut did not follow the rebind"
    );

    // Show Shortcuts moved to Cmd+Shift+Y; the help shows the new caps.
    cmd(&w, "Y");
    until("the rebind opened the shortcuts help", || {
        w.get_show_shortcuts()
    })
    .await;
    let rows: Vec<dbhive_slint::ShortcutItem> = w.get_shortcuts_rows().iter().collect();
    let settings = rows
        .iter()
        .find(|r| r.title == "Open Settings")
        .expect("Open Settings is listed");
    assert_eq!(
        settings
            .keys
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>(),
        vec!["Ctrl", "J"],
        "the help still shows the old binding"
    );
    // The old help key does nothing now.
    press(&w, Key::Escape);
    until("help closed", || !w.get_show_shortcuts()).await;
    key(&w, "?");
    settle().await;
    assert!(
        !w.get_show_shortcuts(),
        "? still opened the help after it was rebound"
    );

    finished.set(true);
}

#[test]
fn a_rebound_shortcut_dispatches_and_shows_its_new_keys() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-rebind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let mut settings = dbhive_core::models::settings::AppSettings::default();
    settings.shortcuts.open_settings = "Ctrl+J".into();
    settings.shortcuts.show_shortcuts = "Ctrl+Shift+Y".into();
    dbhive_core::settings::save(&settings).unwrap();

    let w = dbhive_slint::app::build().unwrap();
    let settings_raised = Rc::new(Cell::new(0u32));
    {
        let n = settings_raised.clone();
        // Replaces the real Settings controller's handler; this one counts.
        w.on_open_settings(move || n.set(n.get() + 1));
    }
    w.show().unwrap();

    let finished = Rc::new(Cell::new(false));
    slint::spawn_local(scenario(
        w.clone_strong(),
        settings_raised,
        finished.clone(),
    ))
    .unwrap();
    w.run().unwrap();
    assert!(
        finished.get(),
        "the event loop ended before the scenario did"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
