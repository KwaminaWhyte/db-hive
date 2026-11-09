//! The Settings dialog driven end to end: the real window, the real
//! controller, real files.
//!
//! Covers what the logic tests and snapshots cannot:
//!
//! * the title-bar gear (`open-settings`) opens the dialog, which was a dead
//!   control before;
//! * theme mode, accent colour and the editor font size and line numbers
//!   apply live, before Save, and an out-of-range value blocks Save without
//!   touching what is on screen;
//! * Save writes `settings.json`; the saved values are what the app obeys;
//! * rebinding a shortcut through real key events reaches the recorder via
//!   the dialog's focus scope, is persisted on its own (an unsaved edit does
//!   not ride along), and a duplicate warns;
//! * the destructive-query guard follows "Confirm destructive queries": off
//!   runs a DELETE with no prompt, on stops at the confirmation;
//! * Reset to defaults asks first, then restores everything on disk and on
//!   screen; closing with unsaved edits asks first and drops them.
//!
//! Every store is redirected to a temp directory, so this never touches the
//! developer's own settings, connections or history.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, Screen, Theme};
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model, Timer, TimerMode};

enum Step {
    /// Not ready yet; look again on the next tick.
    Wait,
    Next,
    Fail(String),
}

type StepFn = Box<dyn FnMut(&AppWindow) -> Step>;

fn read_settings(dir: &std::path::Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(dir.join("settings.json")).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&raw)
        .map(|v| v["settings"].clone())
        .unwrap_or(serde_json::Value::Null)
}

fn sqlite_count(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM widgets", [], |r| r.get(0))
        .unwrap()
}

fn key(w: &AppWindow, text: impl Into<slint::SharedString>) {
    let text: slint::SharedString = text.into();
    w.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    w.window().dispatch_event(WindowEvent::KeyReleased { text });
}

macro_rules! check {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Step::Fail(format!($($msg)+));
        }
    };
}

/// Steps that start recording shortcut row `row` and press Ctrl (Cmd on a
/// Mac) + J through the window's real key path. Recording moves focus to the
/// dialog's key scope on its own, which takes a turn of the event loop, so
/// the keys go out one tick after recording starts.
fn rebind(steps: &mut Vec<StepFn>, row: i32) {
    steps.push(Box::new(move |w| {
        w.invoke_settings_shortcut_record(row);
        check!(
            w.get_settings().recording,
            "starting a recording did not show it"
        );
        Step::Next
    }));
    steps.push(Box::new(|w| {
        // Modifier down, key, modifier up: what a keyboard sends.
        let modifier: slint::SharedString = char::from(Key::Control).to_string().into();
        w.window().dispatch_event(WindowEvent::KeyPressed {
            text: modifier.clone(),
        });
        key(w, "j");
        w.window()
            .dispatch_event(WindowEvent::KeyReleased { text: modifier });
        Step::Next
    }));
}

#[test]
fn settings_open_apply_live_save_rebind_guard_reset_and_close() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-settings-{}", std::process::id()));
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
        let mut profile = ConnectionProfile::new(
            "widgets-db".to_string(),
            "Widgets".to_string(),
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
    // Slint reports Command as `control` on macOS, which the recorder spells Cmd.
    let bound_j = if cfg!(target_os = "macos") {
        "Cmd+J"
    } else {
        "Ctrl+J"
    };

    let d = dir.clone();
    let db2 = db.clone();
    let blue = slint::Color::from_rgb_u8(0x3b, 0x82, 0xf6);

    let mut steps: Vec<StepFn> = Vec::new();

    // 1. The gear opens the dialog. Nothing is dirty yet.
    steps.push(Box::new(|w| {
        w.invoke_open_settings();
        check!(
            w.get_show_settings(),
            "open-settings did not show the dialog"
        );
        let s = w.get_settings();
        check!(
            s.state == 0 && s.section == 0 && !s.dirty,
            "fresh dialog should be clean"
        );
        check!(
            s.startup.as_str() == "Show Connection List",
            "startup was {:?}",
            s.startup
        );
        check!(
            !s.show_default_connection,
            "the default-connection row should only show for Open Default Connection"
        );
        Step::Next
    }));

    // 2. Edits apply live; a bad number blocks Save but not what is on screen.
    steps.push(Box::new(move |w| {
        let theme = w.global::<Theme>();
        w.invoke_settings_toggle_confirm_destructive();
        let s = w.get_settings();
        check!(
            !s.confirm_destructive && s.dirty && s.can_save,
            "toggle did not make the form dirty"
        );

        w.invoke_settings_theme_selected(1); // Light
        check!(
            !theme.get_dark(),
            "choosing Light did not switch the theme live"
        );

        w.invoke_settings_accent_preset_picked(1); // blue
        check!(
            theme.get_accent_custom(),
            "a non-brand accent should override the theme"
        );
        check!(
            theme.get_accent_custom_color() == blue,
            "accent colour was {:?}",
            theme.get_accent_custom_color()
        );

        w.invoke_settings_font_size_changed("99".into());
        let s = w.get_settings();
        check!(!s.can_save, "an out-of-range font size must block Save");
        check!(
            !s.font_size_error.is_empty(),
            "no error shown for font size 99"
        );
        check!(
            theme.get_editor_font_size() == 14.0,
            "an invalid size reached the editor: {}",
            theme.get_editor_font_size()
        );

        w.invoke_settings_font_size_changed("18".into());
        check!(
            w.get_settings().can_save,
            "fixing the size should re-enable Save"
        );
        check!(
            theme.get_editor_font_size() == 18.0,
            "font size was {}",
            theme.get_editor_font_size()
        );

        w.invoke_settings_toggle_line_numbers();
        check!(
            !theme.get_editor_line_numbers(),
            "line numbers did not turn off live"
        );
        Step::Next
    }));

    // 3. Save writes the file and clears the dirty flag.
    {
        let d = d.clone();
        let mut sent = false;
        steps.push(Box::new(move |w| {
            if !sent {
                w.invoke_settings_save();
                sent = true;
            }
            let s = w.get_settings();
            if s.saving || s.dirty {
                return Step::Wait;
            }
            let v = read_settings(&d);
            check!(
                v["query"]["confirmDestructive"] == false,
                "confirmDestructive on disk: {}",
                v["query"]
            );
            check!(
                v["theme"]["mode"] == "light",
                "mode on disk: {}",
                v["theme"]["mode"]
            );
            check!(
                v["theme"]["accentColor"] == "#3b82f6",
                "accent on disk: {}",
                v["theme"]["accentColor"]
            );
            check!(
                v["theme"]["editorFontSize"] == 18,
                "font size on disk: {}",
                v["theme"]["editorFontSize"]
            );
            check!(
                v["theme"]["editorLineNumbers"] == false,
                "line numbers on disk: {}",
                v["theme"]["editorLineNumbers"]
            );
            check!(w.get_toasts().row_count() > 0, "no 'Settings saved' toast");
            Step::Next
        }));
    }

    // 4. Rebind a shortcut with real key events while another edit is unsaved.
    steps.push(Box::new(|w| {
        w.invoke_settings_toggle_auto_save_history(); // an unsaved edit
        w.invoke_settings_section_selected(3);
        Step::Next
    }));
    rebind(&mut steps, 4); // New Tab
    {
        let d = d.clone();
        steps.push(Box::new(move |w| {
            let s = w.get_settings();
            let row = s.shortcuts.row_data(4).unwrap();
            check!(!s.recording, "recording did not stop after a key");
            check!(
                row.binding == bound_j,
                "New Tab shows {:?}, expected {:?}",
                row.binding,
                bound_j
            );
            // Saved on its own: wait for the file.
            let v = read_settings(&d);
            if v["shortcuts"]["newTab"] != bound_j {
                return Step::Wait;
            }
            check!(
                v["query"]["autoSaveHistory"] == true,
                "an unsaved edit rode along with the shortcut: {}",
                v["query"]
            );
            check!(
                s.dirty,
                "the unsaved history toggle should still be pending"
            );
            Step::Next
        }));
    }

    // 5. A duplicate binding is kept but warned about.
    rebind(&mut steps, 1); // Clear Editor, to the same keys as New Tab
    steps.push(Box::new(|w| {
        let s = w.get_settings();
        check!(
            s.banner.contains("already bound to: New Tab"),
            "no duplicate warning, banner was {:?}",
            s.banner
        );
        check!(
            s.shortcuts.row_data(1).unwrap().conflict,
            "the row is not marked as a duplicate"
        );
        Step::Next
    }));

    // 6. Discard throws away the pending history toggle.
    steps.push(Box::new(|w| {
        w.invoke_settings_discard();
        let s = w.get_settings();
        check!(
            s.auto_save_history && !s.dirty,
            "discard did not restore the saved values"
        );
        Step::Next
    }));

    // 7. With "Confirm destructive queries" off, a DELETE runs with no prompt.
    steps.push(Box::new(|w| {
        w.invoke_connect("widgets-db".into());
        Step::Next
    }));
    {
        let mut sent = false;
        steps.push(Box::new(move |w| {
            if w.get_screen() != Screen::Workspace || w.get_workspace_tabs().row_count() != 1 {
                return Step::Wait;
            }
            if !sent {
                w.invoke_workspace_editor_edited("DELETE FROM widgets".into());
                w.invoke_workspace_run_query();
                sent = true;
            }
            check!(
                !w.get_show_confirm(),
                "the guard still asked although the setting is off"
            );
            if w.get_workspace_running() || !w.get_workspace_result_message().contains("affected") {
                return Step::Wait;
            }
            check!(sqlite_count(&db2) == 0, "the DELETE did not run");
            Step::Next
        }));
    }

    // 8. Turn it back on and save: now the same statement stops at the confirmation.
    {
        let mut sent = false;
        steps.push(Box::new(move |w| {
            if !sent {
                w.invoke_settings_toggle_confirm_destructive();
                w.invoke_settings_save();
                sent = true;
            }
            let s = w.get_settings();
            if s.saving || s.dirty {
                return Step::Wait;
            }
            w.invoke_workspace_run_query();
            Step::Next
        }));
    }
    steps.push(Box::new(|w| {
        // The settings dialog is still open; the guard's confirmation shows over it.
        check!(
            w.get_show_confirm(),
            "with the setting on, the DELETE should stop at a confirmation"
        );
        check!(
            w.get_confirm_title() == "Run this statement?",
            "confirmation was {:?}",
            w.get_confirm_title()
        );
        w.invoke_confirm_cancelled();
        w.set_show_confirm(false);
        Step::Next
    }));

    // 9. Reset asks first, then restores disk and screen.
    steps.push(Box::new(|w| {
        w.invoke_settings_reset();
        check!(
            w.get_show_confirm() && w.get_confirm_label() == "Reset",
            "reset did not ask first"
        );
        check!(
            w.get_settings().section == 3,
            "reset should not change section"
        );
        w.invoke_confirm_accepted();
        w.set_show_confirm(false);
        Step::Next
    }));
    {
        let d = d.clone();
        steps.push(Box::new(move |w| {
            let theme = w.global::<Theme>();
            let s = w.get_settings();
            if s.saving || theme.get_accent_custom() {
                return Step::Wait;
            }
            let v = read_settings(&d);
            check!(
                v["theme"]["accentColor"] == "#f59e0b",
                "accent after reset: {}",
                v["theme"]["accentColor"]
            );
            check!(
                v["shortcuts"]["newTab"] == "Ctrl+T",
                "shortcut after reset: {}",
                v["shortcuts"]["newTab"]
            );
            check!(
                v["theme"]["mode"] == "system",
                "mode after reset: {}",
                v["theme"]["mode"]
            );
            check!(
                theme.get_editor_font_size() == 14.0,
                "font size after reset: {}",
                theme.get_editor_font_size()
            );
            check!(
                theme.get_editor_line_numbers(),
                "line numbers should be back on"
            );
            check!(!s.dirty, "dirty after reset");
            Step::Next
        }));
    }

    // 10. Closing with unsaved edits asks first, and drops them.
    steps.push(Box::new(|w| {
        w.invoke_settings_toggle_minimap();
        check!(w.get_settings().dirty, "minimap toggle should be pending");
        w.invoke_settings_close();
        check!(
            w.get_show_confirm() && w.get_confirm_title() == "Discard unsaved changes?",
            "close did not ask: {:?}",
            w.get_confirm_title()
        );
        check!(w.get_show_settings(), "the dialog closed before the answer");
        w.invoke_confirm_accepted();
        w.set_show_confirm(false);
        Step::Next
    }));
    steps.push(Box::new(|w| {
        check!(
            !w.get_show_settings(),
            "confirming did not close the dialog"
        );
        check!(
            !w.get_settings().minimap && !w.get_settings().dirty,
            "the unsaved edit survived the discard"
        );
        Step::Next
    }));

    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let index = Rc::new(Cell::new(0usize));
    let total = steps.len();
    let started = Instant::now();
    let timer = Timer::default();
    let steps = RefCell::new(steps);
    let (weak, fail, index2) = (w.as_weak(), failure.clone(), index.clone());
    let mut last_progress = Instant::now();
    let mut last_index = 0usize;

    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let i = index2.get();
        if i != last_index {
            last_index = i;
            last_progress = Instant::now();
        }
        if started.elapsed() > Duration::from_secs(60)
            || last_progress.elapsed() > Duration::from_secs(10)
        {
            *fail.borrow_mut() = Some(format!("timed out waiting at step {}", i + 1));
            slint::quit_event_loop().unwrap();
            return;
        }
        if i >= total {
            slint::quit_event_loop().unwrap();
            return;
        }
        // A window nobody draws never instantiates its `if` blocks or runs
        // its `changed` handlers; a real one does both every frame.
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        let outcome = (steps.borrow_mut()[i])(&w);
        match outcome {
            Step::Wait => {}
            Step::Next => index2.set(i + 1),
            Step::Fail(msg) => {
                *fail.borrow_mut() = Some(format!("step {}: {}", i + 1, msg));
                slint::quit_event_loop().unwrap();
            }
        }
    });

    w.run().unwrap();
    drop(timer);
    let _ = std::fs::remove_dir_all(&dir);

    if let Some(msg) = failure.borrow().as_ref() {
        panic!("{msg}");
    }
    assert_eq!(index.get(), total, "the scenario did not run to its end");
}
