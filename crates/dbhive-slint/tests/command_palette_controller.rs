//! The command palette, the shortcuts help and global shortcuts, driven end
//! to end: the real window, the real controller, real key events, a real
//! SQLite file for the workspace half.
//!
//! What the logic tests (`dbhive-ui-model::palette_session`) cannot show is
//! that the wiring holds: that Cmd+K reaches the controller from wherever
//! focus is, that the arrows and Enter go to the palette and not its text
//! field, that a command run from the palette changes what the window shows,
//! that "?" typed into an input is text and not a shortcut, and that nothing
//! fires under an open dialog.
//!
//! Every store is redirected to a temp directory, so this never touches the
//! developer's own saved connections or history.

use std::cell::Cell;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, PaletteItem, Screen, Theme};
use dbhive_ui_model::palette_session::{HEADER_HEIGHT, ROW_HEIGHT};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, SharedString, Timer, TimerMode};

// -- helpers ---------------------------------------------------------------

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
    .await
}

/// Wait for `cond`, panicking with `what` if it never holds. The panic is the
/// test failure: it unwinds out of the event loop into `w.run()`.
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

/// Give the window a few turns of the event loop to react, for asserting that
/// something did NOT happen.
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

fn type_text(w: &AppWindow, text: &str) {
    for ch in text.chars() {
        key(w, ch.to_string());
    }
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

fn rows(w: &AppWindow) -> Vec<PaletteItem> {
    w.get_palette_rows().iter().collect()
}

fn titles(w: &AppWindow) -> Vec<String> {
    rows(w)
        .into_iter()
        .filter(|r| !r.header)
        .map(|r| r.title.to_string())
        .collect()
}

fn selected_title(w: &AppWindow) -> String {
    let sel: Vec<PaletteItem> = rows(w).into_iter().filter(|r| r.selected).collect();
    assert_eq!(
        sel.len(),
        1,
        "exactly one row is highlighted, got {}",
        sel.len()
    );
    sel[0].title.to_string()
}

/// Click at a fraction of the window's size. Slint's element lookup needs
/// debug info compiled into the app, which this build does not carry, so
/// fields are found by where the layout puts them.
fn click_at(w: &AppWindow, fx: f32, y: f32) {
    let size = w.window().size().to_logical(w.window().scale_factor());
    let at = LogicalPosition::new(size.width * fx, y);
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
}

/// The height of the menu bar this backend draws inside the window (the
/// snapshot tool shows the same 40px). On a Mac the real app uses the native
/// bar and has none.
const MENU_BAR: f32 = 40.0;

/// The centre of a palette row, from the same row heights Rust hands the
/// window. The palette sits `max(48, 12% of the content height)` below the
/// menu bar, under a 48px search field and its 1px divider.
fn palette_row_center(w: &AppWindow, row: usize) -> LogicalPosition {
    let size = w.window().size().to_logical(w.window().scale_factor());
    let content = size.height - MENU_BAR;
    let mut y = MENU_BAR + (content * 0.12).max(48.0) + 48.0 + 1.0;
    for (i, r) in rows(w).iter().enumerate() {
        let h = if r.header { HEADER_HEIGHT } else { ROW_HEIGHT };
        if i == row {
            return LogicalPosition::new(size.width / 2.0, y + h / 2.0);
        }
        y += h;
    }
    panic!("no row {row}");
}

fn row_of(w: &AppWindow, id: &str) -> usize {
    rows(w)
        .iter()
        .position(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn run_palette(w: &AppWindow, query: &str) {
    cmd(w, "k");
    assert!(w.get_show_palette(), "Cmd+K did not open the palette");
    type_text(w, query);
    press(w, Key::Return);
}

// -- the scenario ------------------------------------------------------------

async fn scenario(w: AppWindow, settings_raised: Rc<Cell<u32>>, finished: Rc<Cell<bool>>) {
    // ---- open, group, filter, navigate, close (connections screen) --------
    assert!(!w.get_show_palette());
    cmd(&w, "k");
    until("Cmd+K opens the palette", || w.get_show_palette()).await;

    let all = rows(&w);
    assert!(
        all[0].header && all[0].title == "NAVIGATION",
        "first row is the first group header: {:?}",
        all[0].title
    );
    assert!(all.iter().any(|r| r.header && r.title == "ACTIONS"));
    let t = titles(&w);
    for want in [
        "New Connection",
        "Go to About",
        "Open Settings",
        "Keyboard Shortcuts",
        "Toggle Theme",
    ] {
        assert!(t.contains(&want.to_string()), "{want} missing from {t:?}");
    }
    assert!(
        !t.contains(&"New Tab".to_string()),
        "workspace commands do not belong on the connection list"
    );
    // "Go to Connections" is listed but disabled, with its reason, and so is
    // New Window (no multi-window).
    let go = all.iter().find(|r| r.id == "go-connections").unwrap();
    assert!(!go.enabled && go.reason == "Already on the connections screen");
    let nw = all.iter().find(|r| r.id == "new-window").unwrap();
    assert!(!nw.enabled && !nw.reason.is_empty());
    // The first thing Enter would run is the first command that can run.
    assert_eq!(selected_title(&w), "Open Settings");

    // The pointer moves the highlight to the row it is over (enabled rows
    // only)...
    let about = row_of(&w, "go-about");
    w.window().dispatch_event(WindowEvent::PointerMoved {
        position: palette_row_center(&w, about),
    });
    until("hover selects the row", || {
        selected_title(&w) == "Go to About"
    })
    .await;
    let disabled = row_of(&w, "go-connections");
    w.window().dispatch_event(WindowEvent::PointerMoved {
        position: palette_row_center(&w, disabled),
    });
    settle().await;
    assert_eq!(
        selected_title(&w),
        "Go to About",
        "hovering a disabled row must not select it"
    );

    // Typing filters by substring; arrows move with wraparound; the field
    // does not swallow them.
    type_text(&w, "theme");
    until("filter applied", || titles(&w).len() == 3).await;
    assert!(
        titles(&w)
            .iter()
            .all(|x| x.to_lowercase().contains("theme")),
        "{:?}",
        titles(&w)
    );
    assert_eq!(w.get_palette_query(), "theme");
    let first = selected_title(&w);
    press(&w, Key::DownArrow);
    let second = selected_title(&w);
    assert_ne!(first, second, "Down did not move the highlight");
    press(&w, Key::UpArrow);
    assert_eq!(selected_title(&w), first);
    press(&w, Key::UpArrow);
    let wrapped = selected_title(&w);
    assert_ne!(
        wrapped, first,
        "Up from the first row should wrap to the last"
    );
    press(&w, Key::DownArrow);
    assert_eq!(
        selected_title(&w),
        first,
        "Down from the last row should wrap to the first"
    );
    assert_eq!(
        w.get_palette_query(),
        "theme",
        "arrows must not edit the query"
    );

    // No match: an empty state, and Enter does nothing.
    type_text(&w, "zzz");
    until("no matches", || rows(&w).is_empty()).await;
    press(&w, Key::Return);
    settle().await;
    assert!(
        w.get_show_palette(),
        "Enter with nothing selected must not close the palette"
    );

    // Escape closes, and reopening starts from a clean query.
    press(&w, Key::Escape);
    until("Escape closes", || !w.get_show_palette()).await;
    cmd(&w, "k");
    until("reopens", || w.get_show_palette()).await;
    assert_eq!(w.get_palette_query(), "");
    // Cmd+K while open closes it (the toggle).
    cmd(&w, "k");
    until("Cmd+K closes it again", || !w.get_show_palette()).await;

    // ---- run a command: theme -------------------------------------------
    assert!(w.global::<Theme>().get_dark());
    run_palette(&w, "toggle theme");
    until("theme command ran and palette closed", || {
        !w.get_show_palette()
    })
    .await;
    assert!(
        !w.global::<Theme>().get_dark(),
        "Toggle Theme from the palette did not flip the theme"
    );
    // And the one that is disabled now says so instead of running.
    cmd(&w, "k");
    type_text(&w, "light theme");
    until("light theme row", || !rows(&w).is_empty()).await;
    let light = rows(&w)
        .into_iter()
        .find(|r| r.id == "theme-light")
        .unwrap();
    assert!(!light.enabled && light.reason == "Already using the light theme");
    press(&w, Key::Escape);
    until("closed", || !w.get_show_palette()).await;

    // ---- the shortcuts help ----------------------------------------------
    // Nothing is focused, so "?" is a shortcut.
    key(&w, "?");
    until("? opens the shortcuts help", || w.get_show_shortcuts()).await;
    let help: Vec<String> = w
        .get_shortcuts_rows()
        .iter()
        .filter(|r| !r.header)
        .map(|r| r.title.to_string())
        .collect();
    for want in [
        "New Connection",
        "Command Palette",
        "Keyboard Shortcuts",
        "Run Query",
        "New Tab",
        "Close Tab",
    ] {
        assert!(
            help.contains(&want.to_string()),
            "{want} missing from the help: {help:?}"
        );
    }
    assert!(
        !help.contains(&"New Window".to_string()),
        "a binding that does nothing must not be listed"
    );
    assert!(!help.contains(&"Toggle Sidebar".to_string()));
    // Typing into its search box filters it (and the "?" inside is text).
    type_text(&w, "tab");
    until("help filtered", || {
        w.get_shortcuts_rows().iter().filter(|r| !r.header).count() < help.len()
    })
    .await;
    let help: Vec<String> = w
        .get_shortcuts_rows()
        .iter()
        .filter(|r| !r.header)
        .map(|r| r.title.to_string())
        .collect();
    assert!(help.contains(&"New Tab".to_string()) && !help.contains(&"Open Settings".to_string()));
    key(&w, "?");
    settle().await;
    assert_eq!(
        w.get_shortcuts_query(),
        "tab?",
        "a ? typed into the search box is text"
    );
    assert_eq!(
        w.get_shortcuts_rows().row_count(),
        0,
        "\"tab?\" matches nothing"
    );
    assert!(
        w.get_show_shortcuts(),
        "typing ? in the help must not close or reopen it"
    );
    press(&w, Key::Escape);
    until("Escape closes the help", || !w.get_show_shortcuts()).await;

    // ---- "?" and Cmd+K when focus is in an input ---------------------------
    // The connection list's search field.
    click_at(&w, 0.6, 182.0);
    type_text(&w, "?");
    settle().await;
    assert_eq!(
        w.get_connection_search(),
        "?",
        "? typed into an input is text"
    );
    assert!(
        !w.get_show_shortcuts(),
        "? typed into an input must not open the help"
    );
    cmd(&w, "k");
    until("Cmd+K works from inside an input", || w.get_show_palette()).await;
    press(&w, Key::Escape);
    until("closed", || !w.get_show_palette()).await;

    // ---- nothing fires under a dialog --------------------------------------
    run_palette(&w, "new conn");
    until("New Connection opened the editor", || {
        w.get_show_connection_editor()
    })
    .await;
    assert!(!w.get_show_palette());
    cmd(&w, "k");
    settle().await;
    assert!(
        !w.get_show_palette(),
        "Cmd+K must not open the palette over a dialog"
    );
    w.invoke_run_command("go-about".into());
    settle().await;
    assert!(
        !w.get_show_about(),
        "a menu command must not run under a dialog either"
    );
    w.invoke_editor_cancel();
    w.set_show_connection_editor(false);
    settle().await;

    // The dead controls are decided, not dead: New Window is refused cleanly.
    let toasts = w.get_toasts().row_count();
    w.invoke_run_command("new-window".into());
    settle().await;
    assert!(!w.get_show_connection_editor() && !w.get_show_about() && !w.get_show_palette());
    assert_eq!(w.get_toasts().row_count(), toasts);

    // Settings is another screen's job: the command raises its callback.
    run_palette(&w, "open settings");
    until("palette closed", || !w.get_show_palette()).await;
    assert_eq!(
        settings_raised.get(),
        1,
        "Open Settings did not reach the open-settings callback"
    );
    w.invoke_run_command("go-settings".into());
    assert_eq!(
        settings_raised.get(),
        2,
        "a menu command goes through the same dispatch"
    );

    // Clicking a row runs it (About, from the palette).
    cmd(&w, "k");
    until("palette", || w.get_show_palette()).await;
    let at = palette_row_center(&w, row_of(&w, "go-about"));
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    until("a click ran About", || {
        w.get_show_about() && !w.get_show_palette()
    })
    .await;
    w.set_show_about(false);
    // A click on a disabled row does nothing and keeps the palette open.
    cmd(&w, "k");
    until("palette", || w.get_show_palette()).await;
    let at = palette_row_center(&w, row_of(&w, "new-window"));
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    settle().await;
    assert!(
        w.get_show_palette(),
        "clicking a disabled row must not close the palette"
    );
    press(&w, Key::Escape);
    until("closed", || !w.get_show_palette()).await;

    // ---- the workspace half -------------------------------------------------
    w.invoke_connect("widgets-db".into());
    until("workspace is open", || {
        w.get_screen() == Screen::Workspace
            && w.get_workspace_tabs().row_count() == 1
            && w.get_workspace_tables().row_count() == 1
    })
    .await;

    // Workspace commands exist now, and Close Tab says what it needs.
    cmd(&w, "k");
    until("palette", || w.get_show_palette()).await;
    let t = titles(&w);
    for want in [
        "New Tab",
        "Close Tab",
        "Run Query",
        "Refresh Metadata",
        "Disconnect",
    ] {
        assert!(
            t.contains(&want.to_string()),
            "{want} missing in the workspace: {t:?}"
        );
    }
    let run = rows(&w).into_iter().find(|r| r.id == "run-query").unwrap();
    assert!(!run.enabled && run.reason == "Write some SQL first");
    press(&w, Key::Escape);
    until("closed", || !w.get_show_palette()).await;

    // Cmd+T and Cmd+W, then the palette doing the same.
    cmd(&w, "t");
    assert_eq!(
        w.get_workspace_tabs().row_count(),
        2,
        "Cmd+T did not open a tab"
    );
    run_palette(&w, "close tab");
    until("close tab ran", || !w.get_show_palette()).await;
    assert_eq!(
        w.get_workspace_tabs().row_count(),
        1,
        "Close Tab from the palette did not close a tab"
    );
    cmd(&w, "w");
    assert_eq!(
        w.get_workspace_tabs().row_count(),
        0,
        "Cmd+W did not close the last tab"
    );
    cmd(&w, "w");
    settle().await;
    assert_eq!(w.get_workspace_tabs().row_count(), 0);
    assert!(!w.get_show_confirm());
    // The workspace chords are not offered when there is nothing to close.
    cmd(&w, "k");
    until("palette", || w.get_show_palette()).await;
    let close = rows(&w).into_iter().find(|r| r.id == "close-tab").unwrap();
    assert!(!close.enabled && close.reason == "No tab is open");
    press(&w, Key::Escape);
    until("closed", || !w.get_show_palette()).await;

    // Run Query: Cmd+Enter from the root scope, with SQL in the tab.
    run_palette(&w, "new tab");
    until("new tab", || w.get_workspace_tabs().row_count() == 1).await;
    w.invoke_workspace_editor_edited("SELECT name FROM widgets ORDER BY id".into());
    press_chord_enter(&w);
    until("query ran", || {
        w.get_workspace_has_result() && !w.get_workspace_running()
    })
    .await;
    assert_eq!(w.get_workspace_row_count(), 2);

    // "?" typed into the SQL editor is SQL.
    // Inside the SQL editor.
    click_at(&w, 0.55, 300.0);
    key(&w, "?");
    settle().await;
    assert!(
        w.get_workspace_editor_text().ends_with('?'),
        "editor text was {:?}",
        w.get_workspace_editor_text()
    );
    assert!(
        !w.get_show_shortcuts(),
        "? typed into the editor must not open the help"
    );

    // Refresh Metadata reloads the table list.
    w.invoke_workspace_editor_edited("SELECT 1".into());
    run_palette(&w, "refresh meta");
    assert!(
        w.get_workspace_loading_tables(),
        "Refresh Metadata did not start a reload"
    );
    until("tables reloaded", || {
        !w.get_workspace_loading_tables() && w.get_workspace_tables().row_count() == 1
    })
    .await;

    // Go to Connections leaves the workspace.
    run_palette(&w, "go to conn");
    until("back on the connection list", || {
        w.get_screen() == Screen::Connections
    })
    .await;
    assert_eq!(w.get_workspace_tabs().row_count(), 0);

    finished.set(true);
    slint::quit_event_loop().unwrap();
}

/// Ctrl+Enter.
fn press_chord_enter(w: &AppWindow) {
    let ctrl: SharedString = Key::Control.into();
    w.window()
        .dispatch_event(WindowEvent::KeyPressed { text: ctrl.clone() });
    key(w, Key::Return);
    w.window()
        .dispatch_event(WindowEvent::KeyReleased { text: ctrl });
}

#[test]
fn palette_shortcuts_and_commands_work_end_to_end() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-palette-{}", std::process::id()));
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
    {
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
    // Nothing wires open-settings in this crate yet (another screen owns it),
    // so stand in for it and count.
    let settings_raised = Rc::new(Cell::new(0u32));
    {
        let n = settings_raised.clone();
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
    // A failed assertion in the scenario panics inside the event loop and
    // comes out of here.
    w.run().unwrap();
    assert!(
        finished.get(),
        "the event loop ended before the scenario did"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
